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

typedef struct {
    const PluginRecord *plugin;
    AuthorizationEngineRef engine;
    int waitsForPermit; /* 0 = milestone A, 1 = milestone B */
} MechanismRecord;

static void repose_log(const char *fmt, ...)
{
    FILE *f = fopen(LOG_PATH, "a");
    if (f == NULL) {
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
        if (access(PERMIT_PATH, F_OK) == 0) {
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
    mech->waitsForPermit = (mechanismId != NULL && strcmp(mechanismId, "permit") == 0);

    repose_log("MechanismCreate id=%s mode=%s",
               mechanismId ? mechanismId : "(null)",
               mech->waitsForPermit ? "permit" : "log");

    *outMechanism = (AuthorizationMechanismRef)mech;
    return errAuthorizationSuccess;
}

static OSStatus MechanismInvoke(AuthorizationMechanismRef inMechanism)
{
    MechanismRecord *mech = (MechanismRecord *)inMechanism;
    AuthorizationResult result = kAuthorizationResultAllow;

    repose_log("MechanismInvoke enter mode=%s",
               mech->waitsForPermit ? "permit" : "log");

    if (mech->waitsForPermit && !wait_for_permit()) {
        result = kAuthorizationResultDeny;
    }

    repose_log("MechanismInvoke result=%s",
               result == kAuthorizationResultAllow ? "Allow" : "Deny");

    OSStatus err = mech->plugin->callbacks->SetResult(mech->engine, result);
    if (err != errAuthorizationSuccess) {
        repose_log("SetResult failed err=%d", (int)err);
        return err;
    }

    return mech->plugin->callbacks->DidDeactivate(mech->engine);
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
