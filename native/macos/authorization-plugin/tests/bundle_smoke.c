#include <Security/AuthorizationPlugin.h>
#include <dlfcn.h>
#include <stdio.h>

typedef OSStatus (*authorization_plugin_create_fn)(
    const AuthorizationCallbacks *callbacks,
    AuthorizationPluginRef *out_plugin,
    const AuthorizationPluginInterface **out_interface);

static OSStatus unused_set_result(AuthorizationEngineRef engine,
                                  AuthorizationResult result) {
    (void)engine;
    (void)result;
    return errAuthorizationSuccess;
}

static OSStatus unused_engine_callback(AuthorizationEngineRef engine) {
    (void)engine;
    return errAuthorizationSuccess;
}

int main(int argument_count, char **arguments) {
    if (argument_count != 2) {
        fputs("usage: bundle_smoke <bundle-executable>\n", stderr);
        return 2;
    }
    void *bundle = dlopen(arguments[1], RTLD_LOCAL | RTLD_NOW);
    if (bundle == NULL) {
        fprintf(stderr, "dlopen failed: %s\n", dlerror());
        return 1;
    }
    authorization_plugin_create_fn create =
        (authorization_plugin_create_fn)dlsym(bundle, "AuthorizationPluginCreate");
    if (create == NULL) {
        fputs("AuthorizationPluginCreate is not exported\n", stderr);
        dlclose(bundle);
        return 1;
    }
    const AuthorizationCallbacks callbacks = {
        .version = kAuthorizationCallbacksVersion,
        .SetResult = unused_set_result,
        .RequestInterrupt = unused_engine_callback,
        .DidDeactivate = unused_engine_callback,
    };
    AuthorizationPluginRef plugin = NULL;
    const AuthorizationPluginInterface *interface = NULL;
    if (create(&callbacks, &plugin, &interface) != errAuthorizationSuccess ||
        plugin == NULL || interface == NULL ||
        interface->version != kAuthorizationPluginInterfaceVersion ||
        interface->PluginDestroy == NULL || interface->MechanismCreate == NULL ||
        interface->MechanismInvoke == NULL ||
        interface->MechanismDeactivate == NULL ||
        interface->MechanismDestroy == NULL) {
        fputs("plugin create returned an invalid interface\n", stderr);
        dlclose(bundle);
        return 1;
    }
    AuthorizationMechanismRef mechanism = NULL;
    if (interface->MechanismCreate(plugin,
                                   (AuthorizationEngineRef)&callbacks,
                                   "unlock",
                                   &mechanism) != errAuthorizationSuccess ||
        mechanism == NULL) {
        fputs("mechanism create failed\n", stderr);
        interface->PluginDestroy(plugin);
        dlclose(bundle);
        return 1;
    }
    if (interface->MechanismDestroy(mechanism) != errAuthorizationSuccess ||
        interface->PluginDestroy(plugin) != errAuthorizationSuccess) {
        fputs("destroy failed\n", stderr);
        dlclose(bundle);
        return 1;
    }
    if (dlclose(bundle) != 0) {
        fputs("dlclose failed\n", stderr);
        return 1;
    }
    puts("authorization bundle smoke: ok");
    return 0;
}
