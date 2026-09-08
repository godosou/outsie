/*
 * Drive the built plugin through a fake authorization engine.
 *
 * WHY THIS EXISTS
 * ---------------
 * Milestone A asks "does macOS load and call our plugin?" and reads the answer
 * off a log file. If no line appears, three very different causes look
 * identical: macOS refused to load the bundle, macOS loaded it and our code
 * crashed before logging, or our code is simply wrong. Only the first is an
 * answer; the other two are our bugs wearing the answer's clothes.
 *
 * This test removes the last two. It dlopens the same signed binary that gets
 * installed, hands it a fake AuthorizationCallbacks, and asserts what it does
 * with SetResult. If this passes and milestone A still logs nothing, the
 * silence really is macOS.
 *
 * It touches nothing outside /tmp and never modifies the authorization
 * database, so it is safe to run on a normal machine.
 *
 * On DidDeactivate: it is the reply to a Deactivate request from the engine,
 * not an announcement that Invoke has finished. An earlier version of both the
 * plugin and this test had Invoke call it, which would hand the engine two
 * replies to one request. The assertions below pin the correct split.
 */

#include <Security/AuthorizationPlugin.h>
#include <Security/AuthorizationTags.h>

#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define LOG_PATH "/tmp/repose-plugin.log"
#define PERMIT_PATH "/tmp/repose-permit"

static int g_pass = 0;
static int g_fail = 0;

/* Kept volatile so -Wnonnull cannot see through it; see the NULL-id case. */
static volatile const char *g_nullMechanismId = NULL;

static void check(const char *name, int ok, const char *detail)
{
    if (ok) {
        printf("  ok   %s\n", name);
        g_pass++;
    } else {
        printf("  FAIL %s%s%s\n", name, detail ? " -- " : "", detail ? detail : "");
        g_fail++;
    }
}

/* ---- fake engine ------------------------------------------------------- */

static int g_setResultCalls;
static AuthorizationResult g_lastResult;
static int g_didDeactivateCalls;
static int g_requestInterruptCalls;
/* DidDeactivate count sampled between Invoke returning and Deactivate being
 * called, so the two can be told apart. */
static int g_ddAfterInvoke;

static void reset_engine(void)
{
    g_setResultCalls = 0;
    g_lastResult = (AuthorizationResult)0xFFFF;
    g_didDeactivateCalls = 0;
    g_requestInterruptCalls = 0;
}

static OSStatus fakeSetResult(AuthorizationEngineRef engine, AuthorizationResult result)
{
    (void)engine;
    g_setResultCalls++;
    g_lastResult = result;
    return errAuthorizationSuccess;
}

static OSStatus fakeDidDeactivate(AuthorizationEngineRef engine)
{
    (void)engine;
    g_didDeactivateCalls++;
    return errAuthorizationSuccess;
}

static OSStatus fakeRequestInterrupt(AuthorizationEngineRef engine)
{
    (void)engine;
    g_requestInterruptCalls++;
    return errAuthorizationSuccess;
}

static OSStatus fakeGetContextValue(AuthorizationEngineRef e, AuthorizationString k,
                                    AuthorizationContextFlags *f,
                                    const AuthorizationValue **v)
{
    (void)e; (void)k; (void)f; (void)v;
    return errAuthorizationInternal;
}

static OSStatus fakeSetContextValue(AuthorizationEngineRef e, AuthorizationString k,
                                    AuthorizationContextFlags f,
                                    const AuthorizationValue *v)
{
    (void)e; (void)k; (void)f; (void)v;
    return errAuthorizationSuccess;
}

static OSStatus fakeGetHintValue(AuthorizationEngineRef e, AuthorizationString k,
                                 const AuthorizationValue **v)
{
    (void)e; (void)k; (void)v;
    return errAuthorizationInternal;
}

static OSStatus fakeSetHintValue(AuthorizationEngineRef e, AuthorizationString k,
                                 const AuthorizationValue *v)
{
    (void)e; (void)k; (void)v;
    return errAuthorizationSuccess;
}

static OSStatus fakeGetArguments(AuthorizationEngineRef e,
                                 const AuthorizationValueVector **v)
{
    (void)e; (void)v;
    return errAuthorizationInternal;
}

static OSStatus fakeGetSessionId(AuthorizationEngineRef e, AuthorizationSessionId *s)
{
    (void)e; (void)s;
    return errAuthorizationInternal;
}

static OSStatus fakeGetImmutableHintValue(AuthorizationEngineRef e, AuthorizationString k,
                                          const AuthorizationValue **v)
{
    (void)e; (void)k; (void)v;
    return errAuthorizationInternal;
}

static AuthorizationCallbacks make_callbacks(UInt32 version)
{
    AuthorizationCallbacks cb;
    memset(&cb, 0, sizeof cb);
    cb.version = version;
    cb.SetResult = fakeSetResult;
    cb.RequestInterrupt = fakeRequestInterrupt;
    cb.DidDeactivate = fakeDidDeactivate;
    cb.GetContextValue = fakeGetContextValue;
    cb.SetContextValue = fakeSetContextValue;
    cb.GetHintValue = fakeGetHintValue;
    cb.SetHintValue = fakeSetHintValue;
    cb.GetArguments = fakeGetArguments;
    cb.GetSessionId = fakeGetSessionId;
    cb.GetImmutableHintValue = fakeGetImmutableHintValue;
    return cb;
}

/* ---- helpers ----------------------------------------------------------- */

typedef OSStatus (*CreateFn)(const AuthorizationCallbacks *,
                             AuthorizationPluginRef *,
                             const AuthorizationPluginInterface **);

static long long now_ms(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}

static off_t log_size(void)
{
    struct stat st;
    if (stat(LOG_PATH, &st) != 0) {
        return -1;
    }
    return st.st_size;
}

/* Run one mechanism end to end and report what the engine observed. */
static void run_mechanism(const AuthorizationPluginInterface *iface,
                          AuthorizationPluginRef plugin,
                          const char *mechanismId,
                          long long *elapsed_ms)
{
    reset_engine();
    AuthorizationMechanismRef mech = NULL;
    long long t0 = now_ms();
    OSStatus err = iface->MechanismCreate(plugin, (AuthorizationEngineRef)0x1234,
                                          (AuthorizationMechanismId)mechanismId, &mech);
    if (err != errAuthorizationSuccess || mech == NULL) {
        printf("  FAIL MechanismCreate(%s) err=%d\n", mechanismId, (int)err);
        g_fail++;
        return;
    }
    iface->MechanismInvoke(mech);
    *elapsed_ms = now_ms() - t0;
    /* Sample before Deactivate: DidDeactivate is the reply to a Deactivate
     * request, so Invoke must not have called it yet. */
    g_ddAfterInvoke = g_didDeactivateCalls;
    iface->MechanismDeactivate(mech);
    iface->MechanismDestroy(mech);
}

int main(int argc, char **argv)
{
    const char *binary = (argc > 1)
        ? argv[1]
        : "build/ReposeSpike.bundle/Contents/MacOS/ReposeSpike";

    printf("plugin contract test\n");
    printf("  binary %s\n\n", binary);

    void *handle = dlopen(binary, RTLD_NOW | RTLD_LOCAL);
    if (handle == NULL) {
        printf("  FAIL dlopen -- %s\n", dlerror());
        return 1;
    }
    check("bundle loads with dlopen", 1, NULL);

    CreateFn create = (CreateFn)dlsym(handle, "AuthorizationPluginCreate");
    check("exports AuthorizationPluginCreate", create != NULL, dlerror());
    if (create == NULL) {
        return 1;
    }

    /* 1. Version negotiation: a host older than the plugin expects is refused
     *    rather than crashed into. */
    {
        AuthorizationCallbacks cb = make_callbacks(0);
        AuthorizationPluginRef plugin = NULL;
        const AuthorizationPluginInterface *iface = NULL;
        OSStatus err = create(&cb, &plugin, &iface);
        check("refuses a callbacks version below its minimum",
              err != errAuthorizationSuccess, NULL);
    }

    /* 2. Normal creation. */
    AuthorizationCallbacks cb = make_callbacks(kAuthorizationCallbacksVersion);
    AuthorizationPluginRef plugin = NULL;
    const AuthorizationPluginInterface *iface = NULL;
    OSStatus err = create(&cb, &plugin, &iface);
    check("accepts the current callbacks version", err == errAuthorizationSuccess, NULL);
    if (err != errAuthorizationSuccess || iface == NULL) {
        return 1;
    }
    check("reports the interface version the host expects",
          iface->version == kAuthorizationPluginInterfaceVersion, NULL);
    check("fills in every interface entry point",
          iface->PluginDestroy && iface->MechanismCreate && iface->MechanismInvoke
              && iface->MechanismDeactivate && iface->MechanismDestroy, NULL);

    off_t before = log_size();

    /* 3. Milestone A: the log mechanism allows unconditionally. */
    {
        long long elapsed = 0;
        run_mechanism(iface, plugin, "log", &elapsed);
        check("log mechanism calls SetResult exactly once", g_setResultCalls == 1, NULL);
        check("log mechanism allows", g_lastResult == kAuthorizationResultAllow, NULL);
        check("log mechanism does not call DidDeactivate from Invoke",
              g_ddAfterInvoke == 0, NULL);
        check("MechanismDeactivate replies with exactly one DidDeactivate",
              g_didDeactivateCalls == 1, NULL);
        check("log mechanism returns promptly", elapsed < 1000, NULL);
    }

    /* 4. Milestone B, permit present: allow, and quickly. A slow allow would
     *    show up as unlock latency in the acceptance test. */
    {
        FILE *f = fopen(PERMIT_PATH, "w");
        if (f) { fclose(f); }
        long long elapsed = 0;
        run_mechanism(iface, plugin, "permit", &elapsed);
        check("permit mechanism allows when the permit exists",
              g_lastResult == kAuthorizationResultAllow, NULL);
        check("permit mechanism calls SetResult exactly once", g_setResultCalls == 1, NULL);
        check("permit mechanism allows without polling delay", elapsed < 500, NULL);
        unlink(PERMIT_PATH);
    }

    /* 5. Milestone B, permit absent: deny, after roughly the advertised
     *    timeout. Denying instantly would make "stays locked while the phone is
     *    away" prove nothing; never denying would freeze the login UI. */
    {
        unlink(PERMIT_PATH);
        long long elapsed = 0;
        run_mechanism(iface, plugin, "permit", &elapsed);
        check("permit mechanism denies when the permit never appears",
              g_lastResult == kAuthorizationResultDeny, NULL);
        check("permit mechanism does not call DidDeactivate from Invoke on deny",
              g_ddAfterInvoke == 0, NULL);
        check("denied mechanism still answers a Deactivate request",
              g_didDeactivateCalls == 1, NULL);
        char detail[64];
        snprintf(detail, sizeof detail, "waited %lldms", elapsed);
        check("permit mechanism waits roughly the advertised timeout",
              elapsed >= 1200 && elapsed <= 3000, detail);
        /* The timeout must stay below the window the acceptance test keeps the
         * phone away for, or a poll from that step survives into the next one
         * and allows on a permit it was never meant to see. */
        check("permit timeout leaves room inside the stay-locked window",
              elapsed < 5000, detail);
    }

    /* 6. An unrecognised mechanism id must deny. Defaulting it to allow is how
     *    a typo in the authorization database, or a stale rule left by a
     *    half-finished install, turns into a machine that unlocks without a
     *    password. This started out allowing; the test is what caught it. */
    {
        long long elapsed = 0;
        run_mechanism(iface, plugin, "definitely-not-a-real-mechanism", &elapsed);
        check("unrecognised mechanism id denies rather than allows",
              g_lastResult == kAuthorizationResultDeny, NULL);
        check("unrecognised mechanism id does not call DidDeactivate from Invoke",
              g_ddAfterInvoke == 0, NULL);
        check("unrecognised mechanism id does not sit through the permit timeout",
              elapsed < 1000, NULL);
    }

    /* 7. A NULL mechanism id is the same class of mistake. */
    {
        reset_engine();
        AuthorizationMechanismRef mech = NULL;
        /* volatile so the compiler cannot fold this back into a literal NULL
         * and reject the call under -Wnonnull. A real host handing us a null
         * id is exactly the case we want to survive. */
        AuthorizationMechanismId nullId = (AuthorizationMechanismId)g_nullMechanismId;
        if (iface->MechanismCreate(plugin, (AuthorizationEngineRef)0x1234, nullId, &mech)
                == errAuthorizationSuccess && mech != NULL) {
            iface->MechanismInvoke(mech);
            check("NULL mechanism id denies",
                  g_lastResult == kAuthorizationResultDeny, NULL);
            iface->MechanismDestroy(mech);
        } else {
            check("NULL mechanism id handled without crashing", 1, NULL);
        }
    }

    check("writes to its log file", log_size() > before, NULL);

    /* 8. Teardown must not crash. */
    iface->PluginDestroy(plugin);
    check("PluginDestroy completes", 1, NULL);

    printf("\n%d passed, %d failed\n", g_pass, g_fail);
    return g_fail == 0 ? 0 : 1;
}
