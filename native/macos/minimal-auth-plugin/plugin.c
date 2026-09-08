/*
 * Repose spike: the smallest Authorization Plugin that can answer one question --
 * does macOS let a third-party plugin take part in screensaver unlock?
 *
 * Two mechanisms live in this one binary:
 *   ReposeSpike:log      milestone A -- log the invocation, then Allow.
 *   ReposeSpike:permit   milestone B -- poll /tmp/repose-permit for 10s;
 *                        Allow if it appears, Deny (fall back to the password
 *                        field) if it does not.
 *
 * This is a throwaway experiment. Do not run it anywhere you cannot roll back.
 */

#include <Security/AuthorizationPlugin.h>
#include <Security/AuthorizationTags.h>

#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

#define LOG_PATH "/tmp/repose-plugin.log"
#define PERMIT_PATH "/tmp/repose-permit"
#define PERMIT_TIMEOUT_MS 10000
#define PERMIT_POLL_MS 200

typedef struct {
    const AuthorizationCallbacks *callbacks;
} PluginRecord;

typedef enum {
    kModeUnknown = 0, /* fail closed: an id we do not recognise denies */
    kModeLog,         /* milestone A */
    kModePermit,      /* milestone B */
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
    return kModeUnknown;
}

static const char *mode_name(MechanismMode mode)
{
    switch (mode) {
    case kModeLog: return "log";
    case kModePermit: return "permit";
    default: return "unknown";
    }
}

static void repose_log(const char *fmt, ...)
{
    /* This runs as root and the log lives in a world-writable directory, so a
     * plain fopen would happily follow a symlink a local user planted there and
     * append root-owned output to a file of their choosing. O_NOFOLLOW refuses
     * that, and 0600 keeps the log itself from being readable by whoever wants
     * to know when the screen was unlocked. */
    int fd = open(LOG_PATH, O_WRONLY | O_APPEND | O_CREAT | O_NOFOLLOW, 0600);
    if (fd < 0) {
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
        /* open rather than access: access() checks with the real uid, which
         * is not necessarily the one this privileged mechanism runs as, and it
         * follows symlinks. Neither subtlety belongs in the one check that
         * decides whether a screen unlocks. */
        int fd = open(PERMIT_PATH, O_RDONLY | O_NOFOLLOW);
        if (fd >= 0) {
            close(fd);
            repose_log("permit: %s present after %dms", PERMIT_PATH, waited_ms);
            return 1;
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
