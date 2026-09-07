#include <Security/AuthorizationPlugin.h>
#include <SystemConfiguration/SystemConfiguration.h>
#include <bsm/audit.h>

#include "ipc_client.h"

#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

typedef struct repose_plugin {
    OSStatus (*set_result)(AuthorizationEngineRef, AuthorizationResult);
    OSStatus (*request_interrupt)(AuthorizationEngineRef);
    OSStatus (*did_deactivate)(AuthorizationEngineRef);
} repose_plugin_t;

typedef struct repose_mechanism {
    AuthorizationEngineRef engine;
    OSStatus (*set_result)(AuthorizationEngineRef, AuthorizationResult);
    OSStatus (*request_interrupt)(AuthorizationEngineRef);
    OSStatus (*did_deactivate)(AuthorizationEngineRef);
    pthread_mutex_t lock;
    pthread_cond_t changed;
    bool deactivated;
    bool destroying;
    bool deactivate_inflight;
    size_t deactivate_waiters;
    OSStatus deactivate_status;
    size_t invokes_inflight;
    bool watcher_running;
    bool watcher_joinable;
    bool watcher_joining;
    bool interrupt_sent;
    uint64_t watcher_generation;
    uint64_t active_watcher_generation;
    pthread_t watcher_thread;
    int cancel_write_socket;
#if defined(REPOSE_UNLOCK_TESTING)
    bool test_configured;
    repose_ipc_test_endpoint_t test_endpoint;
    repose_session_selector_t test_selector;
    ssize_t (*test_cancel_write)(int socket_fd,
                                 const void *bytes,
                                 size_t length,
                                 void *context);
    void *test_cancel_write_context;
    void (*test_after_invoke_admitted)(void *context);
    void *test_after_invoke_admitted_context;
#endif
} repose_mechanism_t;

typedef struct watcher_context {
    repose_mechanism_t *mechanism;
    int watch_socket;
    int cancel_read_socket;
    uint64_t generation;
    repose_ipc_correlation_t correlation;
#if defined(REPOSE_UNLOCK_TESTING)
    void (*after_ready_decoded)(void *context);
    void *after_ready_decoded_context;
#endif
} watcher_context_t;

static OSStatus plugin_destroy(AuthorizationPluginRef reference) {
    if (reference == NULL) {
        return errAuthorizationInvalidPointer;
    }
    free(reference);
    return errAuthorizationSuccess;
}

static OSStatus mechanism_create(AuthorizationPluginRef plugin_reference,
                                 AuthorizationEngineRef engine,
                                 AuthorizationMechanismId mechanism_id,
                                 AuthorizationMechanismRef *out_mechanism) {
    if (out_mechanism != NULL) {
        *out_mechanism = NULL;
    }
    if (plugin_reference == NULL || engine == NULL || mechanism_id == NULL ||
        out_mechanism == NULL || strcmp(mechanism_id, "unlock") != 0) {
        return errAuthorizationInvalidPointer;
    }
    repose_mechanism_t *mechanism = calloc(1, sizeof(*mechanism));
    if (mechanism == NULL) {
        return errAuthorizationInternal;
    }
    if (pthread_mutex_init(&mechanism->lock, NULL) != 0) {
        free(mechanism);
        return errAuthorizationInternal;
    }
    if (pthread_cond_init(&mechanism->changed, NULL) != 0) {
        pthread_mutex_destroy(&mechanism->lock);
        free(mechanism);
        return errAuthorizationInternal;
    }
    repose_plugin_t *plugin = plugin_reference;
    mechanism->engine = engine;
    mechanism->set_result = plugin->set_result;
    mechanism->request_interrupt = plugin->request_interrupt;
    mechanism->did_deactivate = plugin->did_deactivate;
    mechanism->cancel_write_socket = -1;
    *out_mechanism = mechanism;
    return errAuthorizationSuccess;
}

static bool configure_control_socket(int socket_fd) {
    int descriptor_flags = fcntl(socket_fd, F_GETFD);
    int status_flags = fcntl(socket_fd, F_GETFL);
    int enabled = 1;
    return descriptor_flags >= 0 && status_flags >= 0 &&
           fcntl(socket_fd, F_SETFD, descriptor_flags | FD_CLOEXEC) == 0 &&
           fcntl(socket_fd, F_SETFL, status_flags | O_NONBLOCK) == 0 &&
           setsockopt(socket_fd,
                      SOL_SOCKET,
                      SO_NOSIGPIPE,
                      &enabled,
                      (socklen_t)sizeof(enabled)) == 0;
}

static void *watch_for_ready(void *opaque) {
    watcher_context_t *context = opaque;
    repose_ipc_watch_result_t result;
#if defined(REPOSE_UNLOCK_TESTING)
    result = repose_ipc_test_watch_wait(context->watch_socket,
                                        context->cancel_read_socket,
                                        &context->correlation,
                                        context->after_ready_decoded,
                                        context->after_ready_decoded_context);
#else
    result = repose_ipc_watch_wait(
        context->watch_socket, context->cancel_read_socket, &context->correlation);
#endif
    close(context->watch_socket);
    close(context->cancel_read_socket);

    repose_mechanism_t *mechanism = context->mechanism;
    pthread_mutex_lock(&mechanism->lock);
    bool interrupt = result == REPOSE_IPC_WATCH_RESULT_READY &&
                     !mechanism->deactivated &&
                     mechanism->watcher_generation == context->generation &&
                     !mechanism->interrupt_sent;
    if (interrupt) {
        mechanism->interrupt_sent = true;
    }
    pthread_mutex_unlock(&mechanism->lock);
    if (interrupt) {
        (void)mechanism->request_interrupt(mechanism->engine);
    }
    pthread_mutex_lock(&mechanism->lock);
    mechanism->watcher_running = false;
    pthread_cond_broadcast(&mechanism->changed);
    pthread_mutex_unlock(&mechanism->lock);
    free(context);
    return NULL;
}

static void signal_watcher_cancel_locked(repose_mechanism_t *mechanism) {
    if (!mechanism->watcher_running || mechanism->cancel_write_socket < 0) {
        return;
    }
    const uint8_t cancel = 1;
    for (;;) {
        ssize_t count;
#if defined(REPOSE_UNLOCK_TESTING)
        if (mechanism->test_cancel_write != NULL) {
            count = mechanism->test_cancel_write(mechanism->cancel_write_socket,
                                                  &cancel,
                                                  sizeof(cancel),
                                                  mechanism->test_cancel_write_context);
        } else {
            count = write(mechanism->cancel_write_socket, &cancel, sizeof(cancel));
        }
#else
        count = write(mechanism->cancel_write_socket, &cancel, sizeof(cancel));
#endif
        if (count == (ssize_t)sizeof(cancel)) {
            return;
        }
        if (count < 0 && errno == EINTR) {
            continue;
        }
        if (count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
            /* A full one-byte control stream already contains a wakeup. */
            return;
        }
        /* A half-closed control stream wakes poll even if the write path failed. */
        if (shutdown(mechanism->cancel_write_socket, SHUT_RDWR) != 0) {
            close(mechanism->cancel_write_socket);
            mechanism->cancel_write_socket = -1;
        }
        return;
    }
}

static bool watcher_is_cancel_target(const repose_mechanism_t *mechanism,
                                     uint64_t generation_cutoff,
                                     bool cancel_all) {
    return mechanism->watcher_joinable &&
           (cancel_all ||
            mechanism->active_watcher_generation < generation_cutoff);
}

static void cancel_and_join_watcher(repose_mechanism_t *mechanism,
                                    uint64_t generation_cutoff,
                                    bool cancel_all) {
    pthread_mutex_lock(&mechanism->lock);
    bool cancel_signaled = false;
    for (;;) {
        if (!watcher_is_cancel_target(mechanism, generation_cutoff, cancel_all)) {
            pthread_mutex_unlock(&mechanism->lock);
            return;
        }
        if (!cancel_signaled) {
            signal_watcher_cancel_locked(mechanism);
            cancel_signaled = true;
        }
        if (mechanism->watcher_running || mechanism->watcher_joining) {
            pthread_cond_wait(&mechanism->changed, &mechanism->lock);
            continue;
        }
        break;
    }
    uint64_t target_generation = mechanism->active_watcher_generation;
    pthread_t thread = mechanism->watcher_thread;
    int cancel_write_socket = mechanism->cancel_write_socket;
    mechanism->watcher_joining = true;
    pthread_mutex_unlock(&mechanism->lock);
    (void)pthread_join(thread, NULL);
    pthread_mutex_lock(&mechanism->lock);
    if (mechanism->watcher_joinable &&
        mechanism->active_watcher_generation == target_generation) {
        mechanism->watcher_joinable = false;
        mechanism->watcher_joining = false;
        mechanism->active_watcher_generation = 0;
        mechanism->cancel_write_socket = -1;
        pthread_cond_broadcast(&mechanism->changed);
    }
    pthread_mutex_unlock(&mechanism->lock);
    if (cancel_write_socket >= 0) {
        close(cancel_write_socket);
    }
}

static bool start_watcher(repose_mechanism_t *mechanism,
                          repose_ipc_initial_result_t *result,
                          uint64_t invoke_generation
#if defined(REPOSE_UNLOCK_TESTING)
                          ,
                          void (*after_ready_decoded)(void *context),
                          void *after_ready_decoded_context
#endif
) {
    int control[2] = {-1, -1};
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, control) != 0 ||
        !configure_control_socket(control[0]) || !configure_control_socket(control[1])) {
        if (control[0] >= 0) {
            close(control[0]);
        }
        if (control[1] >= 0) {
            close(control[1]);
        }
        return false;
    }
    watcher_context_t *context = calloc(1, sizeof(*context));
    if (context == NULL) {
        close(control[0]);
        close(control[1]);
        return false;
    }
    context->mechanism = mechanism;
    context->watch_socket = result->watch_socket;
    context->cancel_read_socket = control[0];
    context->correlation = result->reply.correlation;
#if defined(REPOSE_UNLOCK_TESTING)
    context->after_ready_decoded = after_ready_decoded;
    context->after_ready_decoded_context = after_ready_decoded_context;
#endif

    pthread_mutex_lock(&mechanism->lock);
    if (mechanism->deactivated || mechanism->watcher_running ||
        mechanism->watcher_joinable ||
        mechanism->watcher_generation != invoke_generation) {
        pthread_mutex_unlock(&mechanism->lock);
        free(context);
        close(control[0]);
        close(control[1]);
        return false;
    }
    mechanism->watcher_generation += 1;
    pthread_cond_broadcast(&mechanism->changed);
    mechanism->interrupt_sent = false;
    context->generation = mechanism->watcher_generation;
    mechanism->active_watcher_generation = context->generation;
    mechanism->cancel_write_socket = control[1];
    mechanism->watcher_running = true;
    int status =
        pthread_create(&mechanism->watcher_thread, NULL, watch_for_ready, context);
    if (status != 0) {
        mechanism->watcher_running = false;
        mechanism->active_watcher_generation = 0;
        mechanism->cancel_write_socket = -1;
        pthread_mutex_unlock(&mechanism->lock);
        free(context);
        close(control[0]);
        close(control[1]);
        return false;
    }
    mechanism->watcher_joinable = true;
    result->watch_socket = -1;
    pthread_mutex_unlock(&mechanism->lock);
    return true;
}

#if !defined(REPOSE_UNLOCK_TESTING)
static bool current_session_selector(repose_session_selector_t *out_selector) {
    if (out_selector == NULL) {
        return false;
    }
    uid_t console_uid = (uid_t)-1;
    gid_t console_gid = (gid_t)-1;
    CFStringRef console_user =
        SCDynamicStoreCopyConsoleUser(NULL, &console_uid, &console_gid);
    if (console_user == NULL) {
        return false;
    }
    CFRelease(console_user);
    struct auditinfo_addr audit_info;
    memset(&audit_info, 0, sizeof(audit_info));
    if (getaudit_addr(&audit_info, (int)sizeof(audit_info)) != 0 || console_uid == 0 ||
        console_uid == (uid_t)-1 || console_uid > UINT32_MAX ||
        audit_info.ai_asid <= 0) {
        return false;
    }
    *out_selector = (repose_session_selector_t){
        .console_uid = (uint32_t)console_uid,
        .audit_session_id = (uint32_t)audit_info.ai_asid,
    };
    return true;
}
#endif

static OSStatus mechanism_invoke(AuthorizationMechanismRef reference) {
    if (reference == NULL) {
        return errAuthorizationInvalidPointer;
    }
    repose_deadline_t deadline;
    bool have_deadline = repose_deadline_start(&deadline, 100);
    repose_mechanism_t *mechanism = reference;
    repose_session_selector_t selector = {0};
#if defined(REPOSE_UNLOCK_TESTING)
    repose_ipc_test_endpoint_t test_endpoint;
    memset(&test_endpoint, 0, sizeof(test_endpoint));
    bool test_configured = false;
    void (*after_invoke_admitted)(void *context) = NULL;
    void *after_invoke_admitted_context = NULL;
#endif
    pthread_mutex_lock(&mechanism->lock);
    /*
     * Authorization Services must not Invoke after Deactivate/Destroy begins.
     * Calls admitted while active get exactly one SetResult; a lifecycle-late
     * call is rejected without touching callbacks so Deactivate/Destroy can
     * guarantee that no callback occurs after they return.
     */
    if (mechanism->deactivated || mechanism->destroying) {
        pthread_mutex_unlock(&mechanism->lock);
        return errAuthorizationDenied;
    }
    mechanism->invokes_inflight += 1;
    mechanism->watcher_generation += 1;
    uint64_t invoke_generation = mechanism->watcher_generation;
#if defined(REPOSE_UNLOCK_TESTING)
    test_configured = mechanism->test_configured;
    test_endpoint = mechanism->test_endpoint;
    if (test_configured) {
        selector = mechanism->test_selector;
    }
    after_invoke_admitted = mechanism->test_after_invoke_admitted;
    after_invoke_admitted_context =
        mechanism->test_after_invoke_admitted_context;
#endif
    pthread_cond_broadcast(&mechanism->changed);
    pthread_mutex_unlock(&mechanism->lock);
#if defined(REPOSE_UNLOCK_TESTING)
    if (after_invoke_admitted != NULL) {
        after_invoke_admitted(after_invoke_admitted_context);
    }
#endif
    cancel_and_join_watcher(mechanism, invoke_generation, false);
    pthread_mutex_lock(&mechanism->lock);
    bool may_exchange = !mechanism->deactivated && !mechanism->destroying &&
                        mechanism->watcher_generation == invoke_generation;
    pthread_mutex_unlock(&mechanism->lock);

    repose_ipc_initial_result_t result;
    memset(&result, 0, sizeof(result));
    result.watch_socket = -1;
    bool exchanged = false;
    if (have_deadline && may_exchange) {
#if defined(REPOSE_UNLOCK_TESTING)
        if (test_configured) {
            exchanged =
                repose_ipc_test_exchange(&test_endpoint, selector, &deadline, &result);
        }
#else
        if (current_session_selector(&selector)) {
            exchanged = repose_ipc_exchange(selector, &deadline, &result);
        }
#endif
    }
    pthread_mutex_lock(&mechanism->lock);
    bool still_active = !mechanism->deactivated && !mechanism->destroying &&
                        mechanism->watcher_generation == invoke_generation;
    pthread_mutex_unlock(&mechanism->lock);
    AuthorizationResult authorization_result =
        still_active && exchanged && result.reply.kind == REPOSE_IPC_REPLY_CONSUMED
            ? kAuthorizationResultAllow
            : kAuthorizationResultDeny;
    bool should_watch = still_active && exchanged &&
                        result.reply.kind == REPOSE_IPC_REPLY_WATCHING;
    OSStatus callback_status =
        mechanism->set_result(mechanism->engine, authorization_result);
    if (callback_status == errAuthorizationSuccess && should_watch) {
        (void)start_watcher(mechanism,
                            &result,
                            invoke_generation
#if defined(REPOSE_UNLOCK_TESTING)
                            ,
                            test_endpoint.after_watch_ready_decoded,
                            test_endpoint.after_watch_ready_decoded_context
#endif
        );
    }
    repose_ipc_initial_result_close(&result);
    pthread_mutex_lock(&mechanism->lock);
    if (mechanism->invokes_inflight > 0) {
        mechanism->invokes_inflight -= 1;
    } else {
        callback_status = errAuthorizationInternal;
    }
    pthread_cond_broadcast(&mechanism->changed);
    pthread_mutex_unlock(&mechanism->lock);
    return callback_status;
}

static OSStatus mechanism_deactivate(AuthorizationMechanismRef reference) {
    if (reference == NULL) {
        return errAuthorizationInvalidPointer;
    }
    repose_mechanism_t *mechanism = reference;
    pthread_mutex_lock(&mechanism->lock);
    if (mechanism->destroying) {
        pthread_mutex_unlock(&mechanism->lock);
        return errAuthorizationDenied;
    }
    bool notify = !mechanism->deactivated;
    if (!notify) {
        mechanism->deactivate_waiters += 1;
        pthread_cond_broadcast(&mechanism->changed);
        while (mechanism->deactivate_inflight) {
            pthread_cond_wait(&mechanism->changed, &mechanism->lock);
        }
        OSStatus status = mechanism->deactivate_status;
        mechanism->deactivate_waiters -= 1;
        pthread_cond_broadcast(&mechanism->changed);
        pthread_mutex_unlock(&mechanism->lock);
        return status;
    }
    mechanism->deactivated = true;
    mechanism->deactivate_inflight = true;
    mechanism->watcher_generation += 1;
    pthread_cond_broadcast(&mechanism->changed);
    pthread_mutex_unlock(&mechanism->lock);
    cancel_and_join_watcher(mechanism, 0, true);
    pthread_mutex_lock(&mechanism->lock);
    while (mechanism->invokes_inflight > 0) {
        pthread_cond_wait(&mechanism->changed, &mechanism->lock);
    }
    pthread_mutex_unlock(&mechanism->lock);
    OSStatus status = mechanism->did_deactivate(mechanism->engine);
    pthread_mutex_lock(&mechanism->lock);
    mechanism->deactivate_status = status;
    mechanism->deactivate_inflight = false;
    pthread_cond_broadcast(&mechanism->changed);
    pthread_mutex_unlock(&mechanism->lock);
    return status;
}

#if defined(REPOSE_UNLOCK_TESTING)
OSStatus repose_plugin_test_configure_mechanism(
    AuthorizationMechanismRef reference,
    const repose_ipc_test_endpoint_t *endpoint,
    repose_session_selector_t selector) {
    if (reference == NULL || endpoint == NULL || endpoint->socket_path == NULL ||
        endpoint->verify_peer == NULL || selector.console_uid == 0 ||
        selector.audit_session_id == 0) {
        return errAuthorizationInvalidPointer;
    }
    repose_mechanism_t *mechanism = reference;
    pthread_mutex_lock(&mechanism->lock);
    if (mechanism->deactivated) {
        pthread_mutex_unlock(&mechanism->lock);
        return errAuthorizationDenied;
    }
    mechanism->test_endpoint = *endpoint;
    mechanism->test_selector = selector;
    mechanism->test_configured = true;
    pthread_mutex_unlock(&mechanism->lock);
    return errAuthorizationSuccess;
}

bool repose_plugin_test_wait_for_generation(AuthorizationMechanismRef reference,
                                            uint64_t minimum_generation,
                                            uint32_t timeout_ms) {
    if (reference == NULL) {
        return false;
    }
    struct timespec deadline;
    if (clock_gettime(CLOCK_REALTIME, &deadline) != 0) {
        return false;
    }
    deadline.tv_sec += (time_t)(timeout_ms / 1000);
    deadline.tv_nsec += (long)(timeout_ms % 1000) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_sec += 1;
        deadline.tv_nsec -= 1000000000L;
    }
    repose_mechanism_t *mechanism = reference;
    pthread_mutex_lock(&mechanism->lock);
    while (mechanism->watcher_generation < minimum_generation) {
        int status = pthread_cond_timedwait(&mechanism->changed,
                                            &mechanism->lock,
                                            &deadline);
        if (status == ETIMEDOUT) {
            pthread_mutex_unlock(&mechanism->lock);
            return false;
        }
        if (status != 0) {
            pthread_mutex_unlock(&mechanism->lock);
            return false;
        }
    }
    pthread_mutex_unlock(&mechanism->lock);
    return true;
}

typedef enum repose_test_lifecycle_state {
    REPOSE_TEST_STATE_DEACTIVATED,
    REPOSE_TEST_STATE_DESTROYING,
    REPOSE_TEST_STATE_DEACTIVATE_WAITERS,
} repose_test_lifecycle_state_t;

static bool test_lifecycle_state_reached(const repose_mechanism_t *mechanism,
                                         repose_test_lifecycle_state_t state,
                                         size_t minimum_waiters) {
    if (state == REPOSE_TEST_STATE_DEACTIVATED) {
        return mechanism->deactivated;
    }
    if (state == REPOSE_TEST_STATE_DESTROYING) {
        return mechanism->destroying;
    }
    return mechanism->deactivate_waiters >= minimum_waiters;
}

static bool wait_for_test_lifecycle_state(AuthorizationMechanismRef reference,
                                          repose_test_lifecycle_state_t state,
                                          size_t minimum_waiters,
                                          uint32_t timeout_ms) {
    if (reference == NULL) {
        return false;
    }
    struct timespec deadline;
    if (clock_gettime(CLOCK_REALTIME, &deadline) != 0) {
        return false;
    }
    deadline.tv_sec += (time_t)(timeout_ms / 1000);
    deadline.tv_nsec += (long)(timeout_ms % 1000) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_sec += 1;
        deadline.tv_nsec -= 1000000000L;
    }
    repose_mechanism_t *mechanism = reference;
    pthread_mutex_lock(&mechanism->lock);
    while (!test_lifecycle_state_reached(mechanism, state, minimum_waiters)) {
        int status = pthread_cond_timedwait(&mechanism->changed,
                                            &mechanism->lock,
                                            &deadline);
        if (status != 0) {
            pthread_mutex_unlock(&mechanism->lock);
            return false;
        }
    }
    pthread_mutex_unlock(&mechanism->lock);
    return true;
}

bool repose_plugin_test_wait_for_deactivated(AuthorizationMechanismRef reference,
                                             uint32_t timeout_ms) {
    return wait_for_test_lifecycle_state(
        reference, REPOSE_TEST_STATE_DEACTIVATED, 0, timeout_ms);
}

bool repose_plugin_test_wait_for_destroying(AuthorizationMechanismRef reference,
                                            uint32_t timeout_ms) {
    return wait_for_test_lifecycle_state(
        reference, REPOSE_TEST_STATE_DESTROYING, 0, timeout_ms);
}

bool repose_plugin_test_wait_for_deactivate_waiters(
    AuthorizationMechanismRef reference,
    size_t minimum_waiters,
    uint32_t timeout_ms) {
    return minimum_waiters > 0 &&
           wait_for_test_lifecycle_state(reference,
                                         REPOSE_TEST_STATE_DEACTIVATE_WAITERS,
                                         minimum_waiters,
                                         timeout_ms);
}

OSStatus repose_plugin_test_set_cancel_writer(
    AuthorizationMechanismRef reference,
    ssize_t (*writer)(int socket_fd,
                      const void *bytes,
                      size_t length,
                      void *context),
    void *context) {
    if (reference == NULL || writer == NULL) {
        return errAuthorizationInvalidPointer;
    }
    repose_mechanism_t *mechanism = reference;
    pthread_mutex_lock(&mechanism->lock);
    if (mechanism->deactivated || mechanism->destroying ||
        mechanism->watcher_running || mechanism->watcher_joinable) {
        pthread_mutex_unlock(&mechanism->lock);
        return errAuthorizationDenied;
    }
    mechanism->test_cancel_write = writer;
    mechanism->test_cancel_write_context = context;
    pthread_mutex_unlock(&mechanism->lock);
    return errAuthorizationSuccess;
}

OSStatus repose_plugin_test_set_after_invoke_admitted_hook(
    AuthorizationMechanismRef reference,
    void (*hook)(void *context),
    void *context) {
    if (reference == NULL || hook == NULL) {
        return errAuthorizationInvalidPointer;
    }
    repose_mechanism_t *mechanism = reference;
    pthread_mutex_lock(&mechanism->lock);
    if (mechanism->deactivated || mechanism->destroying) {
        pthread_mutex_unlock(&mechanism->lock);
        return errAuthorizationDenied;
    }
    mechanism->test_after_invoke_admitted = hook;
    mechanism->test_after_invoke_admitted_context = context;
    pthread_mutex_unlock(&mechanism->lock);
    return errAuthorizationSuccess;
}
#endif

static OSStatus mechanism_destroy(AuthorizationMechanismRef reference) {
    if (reference == NULL) {
        return errAuthorizationInvalidPointer;
    }
    repose_mechanism_t *mechanism = reference;
    pthread_mutex_lock(&mechanism->lock);
    mechanism->destroying = true;
    mechanism->deactivated = true;
    mechanism->watcher_generation += 1;
    pthread_cond_broadcast(&mechanism->changed);
    while (mechanism->invokes_inflight > 0 || mechanism->deactivate_inflight ||
           mechanism->deactivate_waiters > 0) {
        pthread_cond_wait(&mechanism->changed, &mechanism->lock);
    }
    pthread_mutex_unlock(&mechanism->lock);
    cancel_and_join_watcher(mechanism, 0, true);
    if (pthread_cond_destroy(&mechanism->changed) != 0) {
        return errAuthorizationInternal;
    }
    if (pthread_mutex_destroy(&mechanism->lock) != 0) {
        return errAuthorizationInternal;
    }
    free(mechanism);
    return errAuthorizationSuccess;
}

static const AuthorizationPluginInterface PLUGIN_INTERFACE = {
    .version = kAuthorizationPluginInterfaceVersion,
    .PluginDestroy = plugin_destroy,
    .MechanismCreate = mechanism_create,
    .MechanismInvoke = mechanism_invoke,
    .MechanismDeactivate = mechanism_deactivate,
    .MechanismDestroy = mechanism_destroy,
};

__attribute__((visibility("default")))
OSStatus AuthorizationPluginCreate(const AuthorizationCallbacks *callbacks,
                                   AuthorizationPluginRef *out_plugin,
                                   const AuthorizationPluginInterface **out_interface) {
    if (out_plugin != NULL) {
        *out_plugin = NULL;
    }
    if (out_interface != NULL) {
        *out_interface = NULL;
    }
    if (callbacks == NULL || out_plugin == NULL || out_interface == NULL ||
        callbacks->SetResult == NULL || callbacks->RequestInterrupt == NULL ||
        callbacks->DidDeactivate == NULL) {
        return errAuthorizationInvalidPointer;
    }
    repose_plugin_t *plugin = calloc(1, sizeof(*plugin));
    if (plugin == NULL) {
        return errAuthorizationInternal;
    }
    plugin->set_result = callbacks->SetResult;
    plugin->request_interrupt = callbacks->RequestInterrupt;
    plugin->did_deactivate = callbacks->DidDeactivate;
    *out_plugin = plugin;
    *out_interface = &PLUGIN_INTERFACE;
    return errAuthorizationSuccess;
}
