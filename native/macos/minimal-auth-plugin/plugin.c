/*
 * Repose spike: the smallest Authorization Plugin that can answer one question --
 * does macOS let a third-party plugin take part in screensaver unlock?
 *
 * Three mechanisms live in this one binary:
 *   ReposeSpike:log        milestone A -- log the invocation, then Allow.
 *   ReposeSpike:permit     milestone B -- poll /tmp/repose-permit briefly;
 *                          Allow if it appears, Deny (fall back to the password
 *                          field) if it does not. Built for the k-of-n=1 shape,
 *                          where Deny means "try the next subrule (password)".
 *   ReposeSpike:credential E10 -- the fail-closed shape. Meant to sit in a
 *                          REQUIRED chain in front of builtin:authenticate. It
 *                          never denies: when the phone is present it injects
 *                          the user's credentials into the authorization context
 *                          so the following builtin:authenticate passes without
 *                          prompting; otherwise it passes through and lets the
 *                          password field appear. A missing bundle is skipped by
 *                          authd and also falls through to the password, which is
 *                          the whole reason this shape is safe (see
 *                          docs/validation/2026-09-09-e8-failopen-is-intrinsic.md).
 *
 * This is a throwaway experiment. Do not run it anywhere you cannot roll back.
 */

#include <Security/AuthorizationPlugin.h>
#include <Security/AuthorizationTags.h>

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
#define PERMIT_PATH "/tmp/repose-permit"
/* PROTOTYPE ONLY. E10 asks whether a mechanism can hand builtin:authenticate a
 * credential so it passes silently. Where that credential comes from -- and how
 * it is stored and protected -- is the real product problem this spike exists to
 * scope, NOT to solve. A plaintext file in a world-readable /tmp is exactly the
 * thing production must not do; it stands in here for a request to a root daemon
 * that holds the credential in the keychain. Two lines: username, then password. */
#define CREDENTIAL_PATH "/tmp/repose-credential"
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
    kModePermit,      /* milestone B */
    kModeCredential,  /* E10: inject credentials for a trailing builtin:authenticate */
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
    if (strcmp(mechanismId, "log") == 0) {
        return kModeLog;
    }
    if (strcmp(mechanismId, "credential") == 0) {
        return kModeCredential;
    }
    return kModeUnknown;
}

static const char *mode_name(MechanismMode mode)
{
    switch (mode) {
    case kModeLog: return "log";
    case kModePermit: return "permit";
    case kModeCredential: return "credential";
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

/* Returns 1 if the permit file showed up before the timeout. */
static int wait_for_permit(void)
{
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
         * Deliberately NOT checking st_uid: in the walking skeleton the permit
         * is written over ssh by the ordinary account, not by root. Requiring
         * root ownership here would make the mechanism deny every time and look
         * like a transport failure. Ownership is the product's problem to solve
         * by moving the file somewhere only root can write; see README. */
        int fd = open(PERMIT_PATH, O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
        if (fd >= 0) {
            struct stat st;
            int regular = (fstat(fd, &st) == 0) && S_ISREG(st.st_mode);
            close(fd);
            if (regular) {
                repose_log("permit: %s present after %dms", PERMIT_PATH, waited_ms);
                return 1;
            }
            repose_log("permit: %s exists but is not a regular file; ignoring",
                       PERMIT_PATH);
        }
        if (waited_ms >= PERMIT_TIMEOUT_MS) {
            repose_log("permit: %s absent after %dms, giving up", PERMIT_PATH,
                       waited_ms);
            return 0;
        }
        usleep(PERMIT_POLL_MS * 1000);
        waited_ms += PERMIT_POLL_MS;
    }
}

/* Read the prototype credential: line 1 username, line 2 password. Returns 1 on
 * success. Hardened exactly like the permit read -- O_NOFOLLOW and a regular-file
 * check, because this runs as a privileged host reading a path in a world-
 * writable directory. The password never touches the log; only its length does. */
static int read_credential(char *user, size_t user_sz, char *pass, size_t pass_sz)
{
    int fd = open(CREDENTIAL_PATH, O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) {
        return 0;
    }
    struct stat st;
    if (fstat(fd, &st) != 0 || !S_ISREG(st.st_mode)) {
        close(fd);
        return 0;
    }
    char buf[1024];
    ssize_t n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0) {
        return 0;
    }
    buf[n] = '\0';

    char *nl = strchr(buf, '\n');
    if (nl == NULL) {           /* need both a username and a password line */
        memset(buf, 0, sizeof buf);
        return 0;
    }
    *nl = '\0';
    char *pw = nl + 1;
    char *nl2 = strchr(pw, '\n');
    if (nl2 != NULL) {
        *nl2 = '\0';
    }
    if (buf[0] == '\0') {       /* empty username is not a credential */
        memset(buf, 0, sizeof buf);
        return 0;
    }
    strlcpy(user, buf, user_sz);
    strlcpy(pass, pw, pass_sz);
    memset(buf, 0, sizeof buf); /* scrub the plaintext copy on the stack */
    return 1;
}

/* Put a string into the authorization context under `key`. The following
 * mechanism (builtin:authenticate) reads username/password from the context;
 * this is how a mechanism authenticates a user without drawing the password
 * field. Extractable so a privileged mechanism later in the chain can read it. */
static OSStatus set_context_string(MechanismRecord *mech,
                                   AuthorizationString key, const char *s)
{
    AuthorizationValue val;
    val.length = strlen(s);
    val.data = (void *)s;
    return mech->plugin->callbacks->SetContextValue(
        mech->engine, key, kAuthorizationContextFlagExtractable, &val);
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
    case kModeCredential: {
        /* The fail-closed shape. This mechanism NEVER denies: denying here would
         * stop the chain before builtin:authenticate could offer the password
         * field, locking out a user whose phone is simply not there. Instead it
         * decides only whether to pre-fill the credential:
         *
         *   phone present + credential available -> inject username/password,
         *       then Allow; builtin:authenticate consumes them and passes with no
         *       prompt -> passwordless unlock.
         *   phone absent, or no credential        -> inject nothing, then Allow;
         *       builtin:authenticate draws the password field as usual.
         *
         * A missing bundle never reaches this code at all -- authd skips the
         * mechanism and the chain still runs builtin:authenticate, so the failure
         * mode is "type your password", never "unlock for free". */
        char user[256], pass[256];
        if (wait_for_permit() && read_credential(user, sizeof user, pass, sizeof pass)) {
            OSStatus su = set_context_string(mech, kAuthorizationEnvironmentUsername, user);
            OSStatus sp = set_context_string(mech, kAuthorizationEnvironmentPassword, pass);
            repose_log("credential: injected username(len=%zu) password(len=%zu) setctx u=%d p=%d",
                       strlen(user), strlen(pass), (int)su, (int)sp);
        } else {
            repose_log("credential: no phone/credential; passing through to the password field");
        }
        memset(user, 0, sizeof user); /* do not leave the plaintext on the stack */
        memset(pass, 0, sizeof pass);
        result = kAuthorizationResultAllow; /* Allow == "continue to the next mechanism" */
        break;
    }
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
