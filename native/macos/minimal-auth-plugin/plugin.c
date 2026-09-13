/*
 * Outsie spike: the smallest Authorization Plugin that can answer one question --
 * does macOS let a third-party plugin take part in screensaver unlock?
 *
 * Two mechanisms live in this one binary:
 *   ReposeSpike:log        milestone A -- log the invocation, then Allow.
 *   ReposeSpike:permit     milestone B -- poll for a root-owned, fresh permit in
 *                          a root-only-writable directory; Allow if it is there,
 *                          Deny (fall back to the password field) if it is not.
 *                          Built for the k-of-n=1 shape, where Deny means "try
 *                          the next subrule (password)".
 *
 * (A third mode, ReposeSpike:credential, injected credentials for a trailing
 * builtin:authenticate. E10/E11 proved it a dead end -- at the real lock screen
 * the first mechanism's verdict is final, so the mechanism's Allow unlocks
 * before builtin:authenticate ever runs, and a chain that denies gives no
 * password fallback. It was removed rather than left as a latent footgun; see
 * docs/validation/2026-09-09-e11-lockscreen-grant-model.md.)
 *
 * This is a throwaway experiment. Do not run it anywhere you cannot roll back.
 */

#include <Security/AuthorizationPlugin.h>
#include <Security/AuthorizationTags.h>

/* Daemon-backed "consume" mode: ask repose-permitd over the unix socket instead
 * of reading the permit file directly. The client is fail-closed and bounded --
 * see ../permit-daemon/repose_permit_client.{c,h}. */
#include "repose_permit_client.h"

#include <fcntl.h>
#include <os/log.h>
#include <sys/stat.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

#define LOG_PATH "/tmp/repose-plugin.log"
/* The permit lives in a root-only-writable directory, NOT /tmp.
 *
 * /private/tmp is 1777: any local uid can create a file there. A permit whose
 * mere existence unlocks the screen, sitting in a world-writable directory, is
 * an unlock switch anyone on the machine can flip. Moving it to a directory
 * only root can write (root:wheel 0755 -- others may traverse and read, none
 * but root may create) means only a root process can assert "the phone is
 * here". /var/run is also cleared on boot, so a permit never survives a reboot.
 *
 * This is the file-based interim. The endgame is a request/response to a root
 * daemon over a socket (docs/plans/2026-09-09-permit-design.md); this closes
 * the world-writable hole and adds expiry without waiting for that. */
#define PERMIT_DIR "/var/run/repose-spike"
#define PERMIT_PATH "/var/run/repose-spike/permit"
/* The permit must be recent, not merely present. A root writer that has crashed
 * or lost the phone stops refreshing it; once it ages past this it stops
 * unlocking -- the failure direction is "ask for the password", not "stay
 * open". The writer (the BLE daemon) must re-touch it well inside this window. */
#define PERMIT_FRESHNESS_S 15
/* Tolerance for a permit written a moment ago against a slightly-behind clock. */
#define PERMIT_SKEW_S 5
/* 1.5s, not 10s. Two reasons, both real:
 *
 * The mechanism runs ahead of the password path, so this timeout is how long a
 * user stares at a frozen screen before the password field appears when the
 * phone is not there. Ten seconds of that is worse than no feature at all.
 *
 * And it has to be shorter than the acceptance test's "stays locked while the
 * phone is away" window. Otherwise a mechanism still polling from that step is
 * alive when the next step creates the permit, sees it, and allows -- an unlock
 * credited to the phone returning that was really a leftover poll. */
#define PERMIT_TIMEOUT_MS 1500
#define PERMIT_POLL_MS 200

typedef struct {
    const AuthorizationCallbacks *callbacks;
} PluginRecord;

typedef enum {
    kModeUnknown = 0, /* fail closed: an id we do not recognise denies */
    kModeLog,         /* milestone A */
    kModePermit,      /* milestone B: file-based permit poll */
    kModeConsume,     /* gap #3: ask repose-permitd, single-consume over the socket */
} MechanismMode;

typedef struct {
    const PluginRecord *plugin;
    AuthorizationEngineRef engine;
    MechanismMode mode;
} MechanismRecord;

static MechanismMode mode_for(AuthorizationMechanismId mechanismId)
{
    if (mechanismId == NULL) {
        return kModeUnknown;
    }
    if (strcmp(mechanismId, "permit") == 0) {
        return kModePermit;
    }
    if (strcmp(mechanismId, "consume") == 0) {
        return kModeConsume;
    }
    if (strcmp(mechanismId, "log") == 0) {
        return kModeLog;
    }
    return kModeUnknown;
}

static const char *mode_name(MechanismMode mode)
{
    switch (mode) {
    case kModeLog: return "log";
    case kModePermit: return "permit";
    case kModeConsume: return "consume";
    default: return "unknown";
    }
}

/* Two independent channels on purpose.
 *
 * Milestone A's whole verdict is "did the plugin get called", read off a file in
 * /tmp. If the host process cannot write there -- sandboxing, a read-only
 * volume, a permissions surprise on a future macOS -- the file stays empty and
 * looks exactly like a plugin macOS refused to load. os_log goes through the
 * system's own logging and does not depend on the filesystem at all, so the two
 * together can tell "never ran" apart from "ran but could not write".
 *
 * Read it with:
 *   log show --last 10m --predicate 'subsystem == "ai.repose.spike"' --info
 */
static void repose_log(const char *fmt, ...)
{
    va_list oslog_args;
    va_start(oslog_args, fmt);
    char line[512];
    vsnprintf(line, sizeof line, fmt, oslog_args);
    va_end(oslog_args);
    static os_log_t logger;
    if (logger == NULL) {
        logger = os_log_create("ai.repose.spike", "plugin");
    }
    os_log_info(logger, "uid=%d %{public}s", (int)getuid(), line);

    /* This runs as root and the log lives in a world-writable directory, so a
     * plain fopen would happily follow a symlink a local user planted there and
     * append root-owned output to a file of their choosing. O_NOFOLLOW refuses
     * that, and 0600 keeps the log itself from being readable by whoever wants
     * to know when the screen was unlocked. */
    /* O_NONBLOCK here too: a FIFO planted at this path would otherwise block
     * a root mechanism forever. Losing the log is far better than hanging. */
    int fd = open(LOG_PATH, O_WRONLY | O_APPEND | O_CREAT | O_NOFOLLOW | O_NONBLOCK, 0600);
    if (fd < 0) {
        return;
    }
    struct stat lst;
    if (fstat(fd, &lst) != 0 || !S_ISREG(lst.st_mode)) {
        close(fd);
        return;
    }
    FILE *f = fdopen(fd, "a");
    if (f == NULL) {
        close(fd);
        return;
    }

    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    struct tm parts;
    localtime_r(&ts.tv_sec, &parts);
    char stamp[32];
    strftime(stamp, sizeof stamp, "%Y-%m-%dT%H:%M:%S", &parts);
    fprintf(f, "%s.%03ld pid=%d uid=%d ", stamp, ts.tv_nsec / 1000000,
            (int)getpid(), (int)getuid());

    va_list args;
    va_start(args, fmt);
    vfprintf(f, fmt, args);
    va_end(args);

    fputc('\n', f);
    fclose(f);
}

/* The permit path. Fixed at PERMIT_PATH in production; REPOSE_PERMIT_PATH lets
 * the contract test point it at a file it can create as an ordinary user, so the
 * deny paths (not root-owned, stale, absent, FIFO) can be exercised without
 * root. This does not weaken production: a mechanism's environment in
 * SecurityAgent is set by the system, not by any user, and the default is the
 * root-only-writable path -- strictly safer than the world-writable /tmp it
 * replaced, override or not. */
static const char *permit_path(void)
{
    const char *p = getenv("REPOSE_PERMIT_PATH");
    return (p != NULL && p[0] != '\0') ? p : PERMIT_PATH;
}

/* Returns 1 if the permit file showed up before the timeout. */
static int wait_for_permit(void)
{
    const char *path = permit_path();
    int waited_ms = 0;
    for (;;) {
        /* O_NONBLOCK and a regular-file check, not just O_NOFOLLOW.
         *
         * O_NOFOLLOW stops a symlink but does nothing about a FIFO, and opening
         * a FIFO for reading blocks until somebody opens the write end. Any
         * local user can `mkfifo /tmp/repose-permit` -- it needs no privilege --
         * and this open, running as root inside authorizationhost, would then
         * never return. The timeout below could not save it: the block happens
         * inside open(), before any of this loop runs. The unlock UI would hang
         * with a root mechanism stuck behind it, and the symptom is
         * indistinguishable from macOS refusing to load the plugin at all --
         * which is the one question this whole experiment exists to answer.
         *
         * The checks that follow -- regular file, root-owned, fresh -- are the
         * hardening the early walking skeleton deliberately skipped (it let the
         * ssh account write the permit to /tmp). The permit now lives in a
         * root-only-writable directory and must be owned by root and recent, so
         * only the trusted writer can assert presence and a stale assertion
         * stops unlocking on its own. */
        int fd = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
        if (fd >= 0) {
            struct stat st;
            int accept = 0;
            if (fstat(fd, &st) != 0 || !S_ISREG(st.st_mode)) {
                repose_log("permit: %s exists but is not a regular file; ignoring",
                           path);
            } else if (st.st_uid != 0) {
                /* Defence in depth behind the directory's permissions: even if
                 * the directory were somehow writable, a permit not owned by
                 * root was not placed by the trusted writer, so it does not
                 * count. This is the check the walking skeleton deliberately
                 * skipped; it is the whole point of the hardening. */
                repose_log("permit: %s is not root-owned (uid=%d); ignoring",
                           path, (int)st.st_uid);
            } else {
                double age = difftime(time(NULL), st.st_mtime);
                if (age > PERMIT_FRESHNESS_S) {
                    repose_log("permit: %s is stale (%.0fs old > %ds); ignoring",
                               path, age, PERMIT_FRESHNESS_S);
                } else if (age < -PERMIT_SKEW_S) {
                    repose_log("permit: %s mtime is %.0fs in the future; ignoring",
                               path, -age);
                } else {
                    accept = 1;
                }
            }
            close(fd);
            if (accept) {
                repose_log("permit: %s present, root-owned and fresh after %dms",
                           path, waited_ms);
                return 1;
            }
        }
        if (waited_ms >= PERMIT_TIMEOUT_MS) {
            repose_log("permit: %s absent after %dms, giving up", path,
                       waited_ms);
            return 0;
        }
        usleep(PERMIT_POLL_MS * 1000);
        waited_ms += PERMIT_POLL_MS;
    }
}

static OSStatus MechanismCreate(AuthorizationPluginRef inPlugin,
                                AuthorizationEngineRef inEngine,
                                AuthorizationMechanismId mechanismId,
                                AuthorizationMechanismRef *outMechanism)
{
    MechanismRecord *mech = calloc(1, sizeof(MechanismRecord));
    if (mech == NULL) {
        return errAuthorizationInternal;
    }

    mech->plugin = (const PluginRecord *)inPlugin;
    mech->engine = inEngine;
    mech->mode = mode_for(mechanismId);

    repose_log("MechanismCreate id=%s mode=%s",
               mechanismId ? mechanismId : "(null)", mode_name(mech->mode));

    *outMechanism = (AuthorizationMechanismRef)mech;
    return errAuthorizationSuccess;
}

static OSStatus MechanismInvoke(AuthorizationMechanismRef inMechanism)
{
    MechanismRecord *mech = (MechanismRecord *)inMechanism;
    AuthorizationResult result;

    repose_log("MechanismInvoke enter mode=%s", mode_name(mech->mode));

    switch (mech->mode) {
    case kModeLog:
        result = kAuthorizationResultAllow;
        break;
    case kModePermit:
        result = wait_for_permit() ? kAuthorizationResultAllow : kAuthorizationResultDeny;
        break;
    case kModeConsume:
        /* Daemon-backed: repose_request_permit() is fail-closed and bounded --
         * it denies on connect failure, timeout, short read, bad frame, an
         * unmatched nonce, or any non-ALLOW verdict, and never blocks
         * indefinitely (its deadline is under PERMIT_TIMEOUT_MS). Allow only on
         * a CONSUMED verdict, which closes gap #3: a single distinct presence
         * assertion authorises at most one unlock. If the daemon is down or the
         * socket is missing the call returns 0 here, so this denies and the
         * password field appears -- it does not hang. */
        result = repose_request_permit() ? kAuthorizationResultAllow
                                         : kAuthorizationResultDeny;
        break;
    default:
        /* A mechanism id we do not recognise means the authorization database
         * names something this binary does not implement -- a typo, a stale
         * rule, a half-finished install. Allowing there would turn a
         * configuration mistake into a machine that unlocks without a
         * password, so the unrecognised case denies and says why. */
        repose_log("MechanismInvoke unknown mechanism id, denying");
        result = kAuthorizationResultDeny;
        break;
    }

    repose_log("MechanismInvoke result=%s",
               result == kAuthorizationResultAllow ? "Allow" : "Deny");

    OSStatus err = mech->plugin->callbacks->SetResult(mech->engine, result);
    if (err != errAuthorizationSuccess) {
        repose_log("SetResult failed err=%d", (int)err);
        return err;
    }

    /* DidDeactivate does NOT belong here. It is the reply to a Deactivate
     * request from the engine, not a way to announce that Invoke has finished.
     * Calling it from Invoke means the engine gets two DidDeactivate calls for
     * one Deactivate. SetResult is what tells the engine this mechanism has
     * decided; returning success is what tells it Invoke is done. */
    return errAuthorizationSuccess;
}

static OSStatus MechanismDeactivate(AuthorizationMechanismRef inMechanism)
{
    MechanismRecord *mech = (MechanismRecord *)inMechanism;
    repose_log("MechanismDeactivate");
    return mech->plugin->callbacks->DidDeactivate(mech->engine);
}

static OSStatus MechanismDestroy(AuthorizationMechanismRef inMechanism)
{
    repose_log("MechanismDestroy");
    free(inMechanism);
    return errAuthorizationSuccess;
}

static OSStatus PluginDestroy(AuthorizationPluginRef inPlugin)
{
    repose_log("PluginDestroy");
    free(inPlugin);
    return errAuthorizationSuccess;
}

static const AuthorizationPluginInterface gPluginInterface = {
    kAuthorizationPluginInterfaceVersion,
    PluginDestroy,
    MechanismCreate,
    MechanismInvoke,
    MechanismDeactivate,
    MechanismDestroy
};

OSStatus AuthorizationPluginCreate(const AuthorizationCallbacks *callbacks,
                                   AuthorizationPluginRef *outPlugin,
                                   const AuthorizationPluginInterface **outPluginInterface)
{
    repose_log("AuthorizationPluginCreate hostVersion=%u",
               (unsigned)callbacks->version);

    if (callbacks->version < kAuthorizationCallbacksVersion) {
        return errAuthorizationInternal;
    }

    PluginRecord *plugin = calloc(1, sizeof(PluginRecord));
    if (plugin == NULL) {
        return errAuthorizationInternal;
    }
    plugin->callbacks = callbacks;

    *outPlugin = (AuthorizationPluginRef)plugin;
    *outPluginInterface = &gPluginInterface;
    return errAuthorizationSuccess;
}
