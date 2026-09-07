#include "fake_authorization_engine.h"
#include "plugin_test_support.h"
#include "test_server.h"

#include <Security/AuthorizationPlugin.h>
#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

static struct timespec realtime_deadline(uint32_t timeout_ms);

static uint64_t monotonic_milliseconds(void) {
    struct timespec now;
    assert(clock_gettime(CLOCK_MONOTONIC, &now) == 0);
    return (uint64_t)now.tv_sec * 1000 + (uint64_t)now.tv_nsec / 1000000;
}

static void sleep_milliseconds(uint32_t milliseconds) {
    struct timespec duration = {
        .tv_sec = (time_t)(milliseconds / 1000),
        .tv_nsec = (long)(milliseconds % 1000) * 1000000L,
    };
    while (nanosleep(&duration, &duration) != 0) {
        assert(errno == EINTR);
    }
}

typedef struct plugin_fixture {
    repose_fake_authorization_engine_t engine;
    AuthorizationPluginRef plugin;
    const AuthorizationPluginInterface *interface;
    AuthorizationMechanismRef mechanism;
} plugin_fixture_t;

typedef struct plugin_peer_context {
    atomic_bool verified;
} plugin_peer_context_t;

static bool accept_test_peer(int socket_fd, void *opaque) {
    (void)socket_fd;
    plugin_peer_context_t *context = opaque;
    atomic_store_explicit(&context->verified, true, memory_order_release);
    return true;
}

static void fixture_init(plugin_fixture_t *fixture) {
    memset(fixture, 0, sizeof(*fixture));
    repose_fake_engine_init(&fixture->engine);
    AuthorizationCallbacks newer_callbacks = repose_fake_authorization_callbacks;
    newer_callbacks.version = kAuthorizationCallbacksVersion + 7;
    assert(AuthorizationPluginCreate(&newer_callbacks,
                                     &fixture->plugin,
                                     &fixture->interface) == errAuthorizationSuccess);
    assert(fixture->plugin != NULL);
    assert(fixture->interface != NULL);
    assert(fixture->interface->version == kAuthorizationPluginInterfaceVersion);
    assert(fixture->interface->PluginDestroy != NULL);
    assert(fixture->interface->MechanismCreate != NULL);
    assert(fixture->interface->MechanismInvoke != NULL);
    assert(fixture->interface->MechanismDeactivate != NULL);
    assert(fixture->interface->MechanismDestroy != NULL);
    assert(fixture->interface->MechanismCreate(fixture->plugin,
                                               repose_fake_engine_ref(&fixture->engine),
                                               "unlock",
                                               &fixture->mechanism) ==
           errAuthorizationSuccess);
    assert(fixture->mechanism != NULL);
}

static void fixture_configure(plugin_fixture_t *fixture,
                              const repose_ipc_test_endpoint_t *endpoint) {
    assert(repose_plugin_test_configure_mechanism(
               fixture->mechanism,
               endpoint,
               (repose_session_selector_t){.console_uid = 501,
                                            .audit_session_id = 77}) ==
           errAuthorizationSuccess);
}

static void fixture_destroy(plugin_fixture_t *fixture) {
    assert(fixture->interface->MechanismDeactivate(fixture->mechanism) ==
           errAuthorizationSuccess);
    assert(fixture->interface->MechanismDeactivate(fixture->mechanism) ==
           errAuthorizationSuccess);
    assert(repose_fake_engine_deactivate_count(&fixture->engine) == 1);
    assert(fixture->interface->MechanismDestroy(fixture->mechanism) ==
           errAuthorizationSuccess);
    assert(fixture->interface->PluginDestroy(fixture->plugin) ==
           errAuthorizationSuccess);
    repose_fake_engine_destroy(&fixture->engine);
}

static void test_create_failure_clears_outputs(void) {
    AuthorizationPluginRef rejected_plugin = (AuthorizationPluginRef)1;
    const AuthorizationPluginInterface *rejected_interface =
        (const AuthorizationPluginInterface *)1;
    typedef OSStatus (*plugin_create_fn)(const AuthorizationCallbacks *,
                                         AuthorizationPluginRef *,
                                         const AuthorizationPluginInterface **);
    plugin_create_fn create_plugin = AuthorizationPluginCreate;
    assert(create_plugin(NULL, &rejected_plugin, &rejected_interface) ==
           errAuthorizationInvalidPointer);
    assert(rejected_plugin == NULL);
    assert(rejected_interface == NULL);
}

static void serve_consumed(int socket_fd, size_t connection_index, void *opaque) {
    plugin_peer_context_t *context = opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(atomic_load_explicit(&context->verified, memory_order_acquire));
    assert(request_bytes == REPOSE_UNLOCK_IPC_FRAME_LEN);
    assert(saw_eof);
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_CONSUMED, 42, 0x5a, 0);
    assert(repose_test_write_frame(socket_fd, &request, 3));
}

static void test_consumed_maps_to_exactly_one_allow(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    plugin_peer_context_t peer = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_consumed, &peer);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
    };
    fixture_configure(&fixture, &endpoint);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    repose_test_server_join(&server);
    assert(repose_fake_engine_total_result_count(&fixture.engine) == 1);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultAllow) == 1);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultDeny) == 0);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultUndefined) == 0);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultUserCanceled) == 0);
    fixture_destroy(&fixture);
}

static void test_unconfigured_test_build_denies_without_production_fallback(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(repose_fake_engine_total_result_count(&fixture.engine) == 1);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultDeny) == 1);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultAllow) == 0);
    fixture_destroy(&fixture);
}

static void test_invoke_after_deactivate_is_rejected_without_callback(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    assert(fixture.interface->MechanismDeactivate(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(repose_fake_engine_deactivate_count(&fixture.engine) == 1);
    assert(repose_fake_engine_total_result_count(&fixture.engine) == 0);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationDenied);
    assert(repose_fake_engine_total_result_count(&fixture.engine) == 0);
    assert(fixture.interface->MechanismDestroy(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(fixture.interface->PluginDestroy(fixture.plugin) ==
           errAuthorizationSuccess);
    repose_fake_engine_destroy(&fixture.engine);
}

typedef enum failure_reply_mode {
    FAILURE_REPLY_TIMEOUT,
    FAILURE_REPLY_MALFORMED,
    FAILURE_REPLY_STALE_NONCE,
    FAILURE_REPLY_STALE_UID,
    FAILURE_REPLY_STALE_AUDIT_SESSION,
    FAILURE_REPLY_NO_PERMIT,
} failure_reply_mode_t;

typedef struct failure_reply_context {
    plugin_peer_context_t peer;
    failure_reply_mode_t mode;
} failure_reply_context_t;

static void serve_failure_reply(int socket_fd,
                                size_t connection_index,
                                void *opaque) {
    failure_reply_context_t *context = opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(atomic_load_explicit(&context->peer.verified, memory_order_acquire));
    if (context->mode == FAILURE_REPLY_TIMEOUT) {
        sleep_milliseconds(200);
        return;
    }
    uint8_t status = context->mode == FAILURE_REPLY_NO_PERMIT
                         ? REPOSE_UNLOCK_IPC_STATUS_DENIED
                         : REPOSE_UNLOCK_IPC_STATUS_CONSUMED;
    repose_test_make_reply(&request, status, 42, 0x5a, 0);
    switch (context->mode) {
        case FAILURE_REPLY_MALFORMED:
            request.bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET] ^= 1;
            break;
        case FAILURE_REPLY_STALE_NONCE:
            request.bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET] ^= 1;
            break;
        case FAILURE_REPLY_STALE_UID:
            request.bytes[REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET + 3] ^= 1;
            break;
        case FAILURE_REPLY_STALE_AUDIT_SESSION:
            request.bytes[REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET + 3] ^= 1;
            break;
        case FAILURE_REPLY_NO_PERMIT:
        case FAILURE_REPLY_TIMEOUT:
            break;
    }
    assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
}

static void assert_only_denies(repose_fake_authorization_engine_t *engine,
                               size_t expected) {
    assert(repose_fake_engine_total_result_count(engine) == expected);
    assert(repose_fake_engine_result_count(engine, kAuthorizationResultDeny) ==
           expected);
    assert(repose_fake_engine_result_count(engine, kAuthorizationResultAllow) == 0);
    assert(repose_fake_engine_result_count(engine,
                                           kAuthorizationResultUndefined) == 0);
    assert(repose_fake_engine_result_count(engine,
                                           kAuthorizationResultUserCanceled) == 0);
}

static void test_transport_and_correlation_failures_deny_exactly_once(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    char directory_template[] = "/tmp/repose-plugin-missing.XXXXXX";
    char *directory = mkdtemp(directory_template);
    assert(directory != NULL);
    char missing_path[104];
    int length = snprintf(missing_path,
                          sizeof(missing_path),
                          "%s/consume.sock",
                          directory);
    assert(length > 0 && (size_t)length < sizeof(missing_path));
    failure_reply_context_t missing_context = {0};
    repose_ipc_test_endpoint_t missing_endpoint = {
        .socket_path = missing_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &missing_context.peer,
    };
    fixture_configure(&fixture, &missing_endpoint);
    uint64_t started = monotonic_milliseconds();
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(monotonic_milliseconds() - started < 100);
    assert(rmdir(directory) == 0);
    assert_only_denies(&fixture.engine, 1);

    const failure_reply_mode_t modes[] = {
        FAILURE_REPLY_TIMEOUT,
        FAILURE_REPLY_MALFORMED,
        FAILURE_REPLY_STALE_NONCE,
        FAILURE_REPLY_STALE_UID,
        FAILURE_REPLY_STALE_AUDIT_SESSION,
        FAILURE_REPLY_NO_PERMIT,
    };
    for (size_t index = 0; index < sizeof(modes) / sizeof(modes[0]); index += 1) {
        failure_reply_context_t context = {.mode = modes[index]};
        repose_test_server_t server;
        repose_test_server_start(&server, 1, serve_failure_reply, &context);
        repose_ipc_test_endpoint_t endpoint = {
            .socket_path = server.socket_path,
            .verify_peer = accept_test_peer,
            .verify_peer_context = &context.peer,
        };
        fixture_configure(&fixture, &endpoint);
        started = monotonic_milliseconds();
        assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
               errAuthorizationSuccess);
        uint64_t elapsed = monotonic_milliseconds() - started;
        if (modes[index] == FAILURE_REPLY_TIMEOUT) {
            assert(elapsed >= 80 && elapsed <= 180);
        } else {
            assert(elapsed < 100);
        }
        repose_test_server_join(&server);
        assert_only_denies(&fixture.engine, index + 2);
    }
    fixture_destroy(&fixture);
}

typedef struct ready_before_context {
    plugin_peer_context_t peer;
    pthread_mutex_t lock;
    pthread_cond_t changed;
    bool event_written;
} ready_before_context_t;

static void serve_watching_ready_before_deny(int socket_fd,
                                             size_t connection_index,
                                             void *opaque) {
    ready_before_context_t *context = opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(atomic_load_explicit(&context->peer.verified, memory_order_acquire));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
    assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
    request.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] =
        REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
    request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
    assert(repose_test_write_frame(socket_fd, &request, 1));
    assert(repose_test_write_frame(socket_fd, &request, 2));
    pthread_mutex_lock(&context->lock);
    context->event_written = true;
    pthread_cond_broadcast(&context->changed);
    pthread_mutex_unlock(&context->lock);
}

static void wait_until_event_is_written(void *opaque) {
    ready_before_context_t *context = opaque;
    pthread_mutex_lock(&context->lock);
    while (!context->event_written) {
        assert(pthread_cond_wait(&context->changed, &context->lock) == 0);
    }
    pthread_mutex_unlock(&context->lock);
}

static void test_ready_buffered_before_deny_interrupts_once_after_deny(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    ready_before_context_t context;
    memset(&context, 0, sizeof(context));
    assert(pthread_mutex_init(&context.lock, NULL) == 0);
    assert(pthread_cond_init(&context.changed, NULL) == 0);
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_watching_ready_before_deny, &context);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context.peer,
        .after_reply_decoded = wait_until_event_is_written,
        .after_reply_decoded_context = &context,
    };
    fixture_configure(&fixture, &endpoint);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultDeny) == 1);
    assert(repose_fake_engine_wait_for_interrupts(&fixture.engine, 1, 500));
    assert(repose_fake_engine_interrupt_count(&fixture.engine) == 1);
    assert(repose_fake_engine_event_at(&fixture.engine, 0) == REPOSE_FAKE_EVENT_DENY);
    assert(repose_fake_engine_event_at(&fixture.engine, 1) ==
           REPOSE_FAKE_EVENT_INTERRUPT);
    repose_test_server_join(&server);
    fixture_destroy(&fixture);
    assert(pthread_cond_destroy(&context.changed) == 0);
    assert(pthread_mutex_destroy(&context.lock) == 0);
}

static void test_set_result_error_prevents_watcher_spawn(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    ready_before_context_t context;
    memset(&context, 0, sizeof(context));
    assert(pthread_mutex_init(&context.lock, NULL) == 0);
    assert(pthread_cond_init(&context.changed, NULL) == 0);
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_watching_ready_before_deny, &context);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context.peer,
        .after_reply_decoded = wait_until_event_is_written,
        .after_reply_decoded_context = &context,
    };
    fixture_configure(&fixture, &endpoint);
    repose_fake_engine_set_result_status(&fixture.engine, errAuthorizationInternal);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationInternal);
    repose_test_server_join(&server);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultDeny) == 1);
    assert(repose_fake_engine_interrupt_count(&fixture.engine) == 0);
    fixture_destroy(&fixture);
    assert(pthread_cond_destroy(&context.changed) == 0);
    assert(pthread_mutex_destroy(&context.lock) == 0);
}

typedef struct ready_after_context {
    plugin_peer_context_t peer;
    repose_fake_authorization_engine_t *engine;
} ready_after_context_t;

static void serve_watching_ready_after_deny(int socket_fd,
                                            size_t connection_index,
                                            void *opaque) {
    ready_after_context_t *context = opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(atomic_load_explicit(&context->peer.verified, memory_order_acquire));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
    assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
    assert(repose_fake_engine_wait_for_results(context->engine, 1, 500));
    request.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] =
        REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
    request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
    assert(repose_test_write_frame(socket_fd, &request, 4));
}

static void test_ready_after_deny_interrupts_once(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    ready_after_context_t context = {.engine = &fixture.engine};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_watching_ready_after_deny, &context);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context.peer,
    };
    fixture_configure(&fixture, &endpoint);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(repose_fake_engine_wait_for_interrupts(&fixture.engine, 1, 500));
    assert(repose_fake_engine_event_at(&fixture.engine, 0) == REPOSE_FAKE_EVENT_DENY);
    assert(repose_fake_engine_event_at(&fixture.engine, 1) ==
           REPOSE_FAKE_EVENT_INTERRUPT);
    repose_test_server_join(&server);
    fixture_destroy(&fixture);
}

static void test_request_interrupt_error_is_not_retried(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    ready_after_context_t context = {.engine = &fixture.engine};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_watching_ready_after_deny, &context);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context.peer,
    };
    fixture_configure(&fixture, &endpoint);
    repose_fake_engine_set_interrupt_status(&fixture.engine, errAuthorizationInternal);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(repose_fake_engine_wait_for_interrupts(&fixture.engine, 1, 500));
    assert(!repose_fake_engine_wait_for_interrupts(&fixture.engine, 2, 100));
    assert(repose_fake_engine_interrupt_count(&fixture.engine) == 1);
    repose_test_server_join(&server);
    fixture_destroy(&fixture);
}

static void serve_watch_then_consumed(int socket_fd,
                                      size_t connection_index,
                                      void *opaque) {
    plugin_peer_context_t *peer = opaque;
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(atomic_load_explicit(&peer->verified, memory_order_acquire));
    if (connection_index == 0) {
        repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
        assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
        request.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] =
            REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
        request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
        assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
    } else {
        assert(connection_index == 1);
        repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_CONSUMED, 42, 0x5a, 0);
        assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
    }
}

static void test_ready_edge_never_allows_until_second_invoke_consumes(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    plugin_peer_context_t peer = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 2, serve_watch_then_consumed, &peer);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
    };
    fixture_configure(&fixture, &endpoint);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(repose_fake_engine_wait_for_interrupts(&fixture.engine, 1, 500));
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultAllow) == 0);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultDeny) == 1);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultAllow) == 1);
    assert(repose_fake_engine_interrupt_count(&fixture.engine) == 1);
    repose_test_server_join(&server);
    fixture_destroy(&fixture);
}

typedef struct decoded_ready_gate {
    pthread_mutex_t lock;
    pthread_cond_t changed;
    bool entered;
    bool released;
} decoded_ready_gate_t;

static void block_after_ready_decode(void *opaque) {
    decoded_ready_gate_t *gate = opaque;
    pthread_mutex_lock(&gate->lock);
    gate->entered = true;
    pthread_cond_broadcast(&gate->changed);
    while (!gate->released) {
        assert(pthread_cond_wait(&gate->changed, &gate->lock) == 0);
    }
    pthread_mutex_unlock(&gate->lock);
}

static bool wait_for_ready_decode(decoded_ready_gate_t *gate, uint32_t timeout_ms) {
    struct timespec deadline = realtime_deadline(timeout_ms);
    pthread_mutex_lock(&gate->lock);
    while (!gate->entered) {
        int status = pthread_cond_timedwait(&gate->changed, &gate->lock, &deadline);
        if (status == ETIMEDOUT) {
            pthread_mutex_unlock(&gate->lock);
            return false;
        }
        assert(status == 0);
    }
    pthread_mutex_unlock(&gate->lock);
    return true;
}

static void release_ready_decode(decoded_ready_gate_t *gate) {
    pthread_mutex_lock(&gate->lock);
    gate->released = true;
    pthread_cond_broadcast(&gate->changed);
    pthread_mutex_unlock(&gate->lock);
}

static void serve_watch_then_denied(int socket_fd,
                                    size_t connection_index,
                                    void *opaque) {
    plugin_peer_context_t *peer = opaque;
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(atomic_load_explicit(&peer->verified, memory_order_acquire));
    if (connection_index == 0) {
        repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
        assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
        request.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] =
            REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
        request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
        assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
    } else {
        assert(connection_index == 1);
        repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_DENIED, 42, 0x5a, 0);
        assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
    }
}

typedef struct async_interface_call {
    pthread_mutex_t lock;
    pthread_cond_t changed;
    const AuthorizationPluginInterface *interface;
    AuthorizationMechanismRef mechanism;
    bool started;
    bool done;
    OSStatus status;
} async_interface_call_t;

static struct timespec realtime_deadline(uint32_t timeout_ms) {
    struct timespec deadline;
    assert(clock_gettime(CLOCK_REALTIME, &deadline) == 0);
    deadline.tv_sec += (time_t)(timeout_ms / 1000);
    deadline.tv_nsec += (long)(timeout_ms % 1000) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_sec += 1;
        deadline.tv_nsec -= 1000000000L;
    }
    return deadline;
}

static void async_call_init(async_interface_call_t *call,
                            const AuthorizationPluginInterface *interface,
                            AuthorizationMechanismRef mechanism) {
    memset(call, 0, sizeof(*call));
    assert(pthread_mutex_init(&call->lock, NULL) == 0);
    assert(pthread_cond_init(&call->changed, NULL) == 0);
    call->interface = interface;
    call->mechanism = mechanism;
}

static void async_call_destroy(async_interface_call_t *call) {
    assert(pthread_cond_destroy(&call->changed) == 0);
    assert(pthread_mutex_destroy(&call->lock) == 0);
}

static bool async_call_wait_done(async_interface_call_t *call, uint32_t timeout_ms) {
    struct timespec deadline = realtime_deadline(timeout_ms);
    pthread_mutex_lock(&call->lock);
    while (!call->done) {
        int status = pthread_cond_timedwait(&call->changed, &call->lock, &deadline);
        if (status == ETIMEDOUT) {
            pthread_mutex_unlock(&call->lock);
            return false;
        }
        assert(status == 0);
    }
    pthread_mutex_unlock(&call->lock);
    return true;
}

static void async_mark_started(async_interface_call_t *call) {
    pthread_mutex_lock(&call->lock);
    call->started = true;
    pthread_cond_broadcast(&call->changed);
    pthread_mutex_unlock(&call->lock);
}

static void async_mark_done(async_interface_call_t *call, OSStatus status) {
    pthread_mutex_lock(&call->lock);
    call->status = status;
    call->done = true;
    pthread_cond_broadcast(&call->changed);
    pthread_mutex_unlock(&call->lock);
}

static void *invoke_async(void *opaque) {
    async_interface_call_t *call = opaque;
    async_mark_started(call);
    OSStatus status = call->interface->MechanismInvoke(call->mechanism);
    async_mark_done(call, status);
    return NULL;
}

static void *deactivate_async(void *opaque) {
    async_interface_call_t *call = opaque;
    async_mark_started(call);
    OSStatus status = call->interface->MechanismDeactivate(call->mechanism);
    async_mark_done(call, status);
    return NULL;
}

static void *destroy_async(void *opaque) {
    async_interface_call_t *call = opaque;
    async_mark_started(call);
    OSStatus status = call->interface->MechanismDestroy(call->mechanism);
    async_mark_done(call, status);
    return NULL;
}

typedef struct first_reply_gate {
    pthread_mutex_t lock;
    pthread_cond_t changed;
    size_t calls;
    bool first_entered;
    bool release_first;
} first_reply_gate_t;

static void block_first_decoded_reply(void *opaque) {
    first_reply_gate_t *gate = opaque;
    pthread_mutex_lock(&gate->lock);
    gate->calls += 1;
    if (gate->calls == 1) {
        gate->first_entered = true;
        pthread_cond_broadcast(&gate->changed);
        while (!gate->release_first) {
            assert(pthread_cond_wait(&gate->changed, &gate->lock) == 0);
        }
    }
    pthread_mutex_unlock(&gate->lock);
}

static bool wait_for_first_decoded_reply(first_reply_gate_t *gate,
                                         uint32_t timeout_ms) {
    struct timespec deadline = realtime_deadline(timeout_ms);
    pthread_mutex_lock(&gate->lock);
    while (!gate->first_entered) {
        int status = pthread_cond_timedwait(&gate->changed, &gate->lock, &deadline);
        if (status == ETIMEDOUT) {
            pthread_mutex_unlock(&gate->lock);
            return false;
        }
        assert(status == 0);
    }
    pthread_mutex_unlock(&gate->lock);
    return true;
}

static void release_first_decoded_reply(first_reply_gate_t *gate) {
    pthread_mutex_lock(&gate->lock);
    gate->release_first = true;
    pthread_cond_broadcast(&gate->changed);
    pthread_mutex_unlock(&gate->lock);
}

static void test_older_invoke_cannot_revive_watcher_after_newer_invoke(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    plugin_peer_context_t peer = {0};
    first_reply_gate_t gate;
    memset(&gate, 0, sizeof(gate));
    assert(pthread_mutex_init(&gate.lock, NULL) == 0);
    assert(pthread_cond_init(&gate.changed, NULL) == 0);
    repose_test_server_t server;
    repose_test_server_start(&server, 2, serve_watch_then_denied, &peer);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
        .after_reply_decoded = block_first_decoded_reply,
        .after_reply_decoded_context = &gate,
    };
    fixture_configure(&fixture, &endpoint);

    async_interface_call_t older;
    async_call_init(&older, fixture.interface, fixture.mechanism);
    pthread_t older_thread;
    assert(pthread_create(&older_thread, NULL, invoke_async, &older) == 0);
    assert(wait_for_first_decoded_reply(&gate, 500));

    async_interface_call_t newer;
    async_call_init(&newer, fixture.interface, fixture.mechanism);
    pthread_t newer_thread;
    assert(pthread_create(&newer_thread, NULL, invoke_async, &newer) == 0);
    assert(async_call_wait_done(&newer, 500));
    assert(pthread_join(newer_thread, NULL) == 0);
    assert(newer.status == errAuthorizationSuccess);

    release_first_decoded_reply(&gate);
    assert(pthread_join(older_thread, NULL) == 0);
    assert(older.status == errAuthorizationSuccess);
    assert(repose_fake_engine_wait_for_results(&fixture.engine, 2, 500));
    assert(!repose_fake_engine_wait_for_interrupts(&fixture.engine, 1, 200));
    assert(repose_fake_engine_interrupt_count(&fixture.engine) == 0);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultDeny) == 2);
    repose_test_server_join(&server);
    fixture_destroy(&fixture);
    async_call_destroy(&newer);
    async_call_destroy(&older);
    assert(pthread_cond_destroy(&gate.changed) == 0);
    assert(pthread_mutex_destroy(&gate.lock) == 0);
}

static void test_new_invoke_invalidates_ready_decoded_by_old_watcher(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    plugin_peer_context_t peer = {0};
    decoded_ready_gate_t gate;
    memset(&gate, 0, sizeof(gate));
    assert(pthread_mutex_init(&gate.lock, NULL) == 0);
    assert(pthread_cond_init(&gate.changed, NULL) == 0);
    repose_test_server_t server;
    repose_test_server_start(&server, 2, serve_watch_then_denied, &peer);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
        .after_watch_ready_decoded = block_after_ready_decode,
        .after_watch_ready_decoded_context = &gate,
    };
    fixture_configure(&fixture, &endpoint);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(wait_for_ready_decode(&gate, 500));

    async_interface_call_t second_invoke;
    async_call_init(&second_invoke, fixture.interface, fixture.mechanism);
    pthread_t second_thread;
    assert(pthread_create(&second_thread, NULL, invoke_async, &second_invoke) == 0);
    assert(repose_plugin_test_wait_for_generation(fixture.mechanism, 3, 500));
    release_ready_decode(&gate);
    assert(pthread_join(second_thread, NULL) == 0);
    assert(second_invoke.status == errAuthorizationSuccess);
    assert(repose_fake_engine_interrupt_count(&fixture.engine) == 0);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultDeny) == 2);
    repose_test_server_join(&server);
    fixture_destroy(&fixture);
    async_call_destroy(&second_invoke);
    assert(pthread_cond_destroy(&gate.changed) == 0);
    assert(pthread_mutex_destroy(&gate.lock) == 0);
}

static void test_deactivate_waits_for_synchronous_invoke_callback(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    plugin_peer_context_t peer = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_consumed, &peer);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
    };
    fixture_configure(&fixture, &endpoint);
    repose_fake_engine_block_result(&fixture.engine);

    async_interface_call_t invoke;
    async_call_init(&invoke, fixture.interface, fixture.mechanism);
    pthread_t invoke_thread;
    assert(pthread_create(&invoke_thread, NULL, invoke_async, &invoke) == 0);
    assert(repose_fake_engine_wait_for_result_entry(&fixture.engine, 500));

    async_interface_call_t deactivate;
    async_call_init(&deactivate, fixture.interface, fixture.mechanism);
    pthread_t deactivate_thread;
    assert(pthread_create(&deactivate_thread, NULL, deactivate_async, &deactivate) == 0);
    assert(repose_plugin_test_wait_for_deactivated(fixture.mechanism, 500));
    assert(!async_call_wait_done(&deactivate, 100));

    repose_fake_engine_release_result(&fixture.engine);
    assert(pthread_join(invoke_thread, NULL) == 0);
    assert(pthread_join(deactivate_thread, NULL) == 0);
    assert(invoke.status == errAuthorizationSuccess);
    assert(deactivate.status == errAuthorizationSuccess);
    repose_test_server_join(&server);
    assert(repose_fake_engine_deactivate_count(&fixture.engine) == 1);
    assert(fixture.interface->MechanismDestroy(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(fixture.interface->PluginDestroy(fixture.plugin) ==
           errAuthorizationSuccess);
    async_call_destroy(&deactivate);
    async_call_destroy(&invoke);
    repose_fake_engine_destroy(&fixture.engine);
}

static void test_destroy_waits_for_synchronous_invoke_callback(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    plugin_peer_context_t peer = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_consumed, &peer);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
    };
    fixture_configure(&fixture, &endpoint);
    repose_fake_engine_block_result(&fixture.engine);

    async_interface_call_t invoke;
    async_call_init(&invoke, fixture.interface, fixture.mechanism);
    pthread_t invoke_thread;
    assert(pthread_create(&invoke_thread, NULL, invoke_async, &invoke) == 0);
    assert(repose_fake_engine_wait_for_result_entry(&fixture.engine, 500));

    async_interface_call_t destroy;
    async_call_init(&destroy, fixture.interface, fixture.mechanism);
    pthread_t destroy_thread;
    assert(pthread_create(&destroy_thread, NULL, destroy_async, &destroy) == 0);
    assert(repose_plugin_test_wait_for_destroying(fixture.mechanism, 500));
    assert(!async_call_wait_done(&destroy, 100));

    repose_fake_engine_release_result(&fixture.engine);
    assert(pthread_join(invoke_thread, NULL) == 0);
    assert(pthread_join(destroy_thread, NULL) == 0);
    assert(invoke.status == errAuthorizationSuccess);
    assert(destroy.status == errAuthorizationSuccess);
    repose_test_server_join(&server);
    assert(fixture.interface->PluginDestroy(fixture.plugin) ==
           errAuthorizationSuccess);
    async_call_destroy(&destroy);
    async_call_destroy(&invoke);
    repose_fake_engine_destroy(&fixture.engine);
}

static void test_destroy_waits_for_pending_ready_callback(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    ready_before_context_t context;
    memset(&context, 0, sizeof(context));
    assert(pthread_mutex_init(&context.lock, NULL) == 0);
    assert(pthread_cond_init(&context.changed, NULL) == 0);
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_watching_ready_before_deny, &context);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context.peer,
        .after_reply_decoded = wait_until_event_is_written,
        .after_reply_decoded_context = &context,
    };
    fixture_configure(&fixture, &endpoint);
    repose_fake_engine_block_interrupt(&fixture.engine);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(repose_fake_engine_wait_for_interrupt_entry(&fixture.engine, 500));

    async_interface_call_t destroy;
    async_call_init(&destroy, fixture.interface, fixture.mechanism);
    pthread_t destroy_thread;
    assert(pthread_create(&destroy_thread, NULL, destroy_async, &destroy) == 0);
    assert(repose_plugin_test_wait_for_destroying(fixture.mechanism, 500));
    assert(!async_call_wait_done(&destroy, 100));
    repose_fake_engine_release_interrupt(&fixture.engine);
    assert(pthread_join(destroy_thread, NULL) == 0);
    assert(destroy.status == errAuthorizationSuccess);
    assert(repose_fake_engine_interrupt_count(&fixture.engine) == 1);
    repose_test_server_join(&server);
    assert(fixture.interface->PluginDestroy(fixture.plugin) ==
           errAuthorizationSuccess);
    async_call_destroy(&destroy);
    repose_fake_engine_destroy(&fixture.engine);
    assert(pthread_cond_destroy(&context.changed) == 0);
    assert(pthread_mutex_destroy(&context.lock) == 0);
}

static void test_concurrent_deactivate_calls_both_wait_for_single_callback(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    repose_fake_engine_set_deactivate_status(&fixture.engine,
                                             errAuthorizationInternal);
    repose_fake_engine_block_deactivate(&fixture.engine);

    async_interface_call_t first;
    async_call_init(&first, fixture.interface, fixture.mechanism);
    pthread_t first_thread;
    assert(pthread_create(&first_thread, NULL, deactivate_async, &first) == 0);
    assert(repose_fake_engine_wait_for_deactivate_entry(&fixture.engine, 500));

    async_interface_call_t second;
    async_call_init(&second, fixture.interface, fixture.mechanism);
    pthread_t second_thread;
    assert(pthread_create(&second_thread, NULL, deactivate_async, &second) == 0);
    assert(repose_plugin_test_wait_for_deactivate_waiters(
        fixture.mechanism, 1, 500));
    assert(!async_call_wait_done(&second, 100));

    repose_fake_engine_release_deactivate(&fixture.engine);
    assert(pthread_join(first_thread, NULL) == 0);
    assert(pthread_join(second_thread, NULL) == 0);
    assert(first.status == errAuthorizationInternal);
    assert(second.status == errAuthorizationInternal);
    assert(repose_fake_engine_deactivate_count(&fixture.engine) == 1);
    assert(fixture.interface->MechanismDestroy(fixture.mechanism) ==
           errAuthorizationSuccess);
    assert(fixture.interface->PluginDestroy(fixture.plugin) ==
           errAuthorizationSuccess);
    async_call_destroy(&second);
    async_call_destroy(&first);
    repose_fake_engine_destroy(&fixture.engine);
}

typedef struct held_watch_context {
    plugin_peer_context_t peer;
    pthread_mutex_t lock;
    pthread_cond_t changed;
    bool release;
} held_watch_context_t;

static void serve_held_watch(int socket_fd,
                             size_t connection_index,
                             void *opaque) {
    held_watch_context_t *context = opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(atomic_load_explicit(&context->peer.verified, memory_order_acquire));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
    assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));

    struct timespec deadline = realtime_deadline(2000);
    pthread_mutex_lock(&context->lock);
    while (!context->release) {
        int status = pthread_cond_timedwait(&context->changed,
                                            &context->lock,
                                            &deadline);
        if (status == ETIMEDOUT) {
            break;
        }
        assert(status == 0);
    }
    pthread_mutex_unlock(&context->lock);
}

static void held_watch_release(held_watch_context_t *context) {
    pthread_mutex_lock(&context->lock);
    context->release = true;
    pthread_cond_broadcast(&context->changed);
    pthread_mutex_unlock(&context->lock);
}

typedef struct cancel_write_context {
    atomic_uint calls;
    bool fail_first_with_eintr;
    bool always_fail;
    bool drop_first_success;
} cancel_write_context_t;

static ssize_t injected_cancel_write(int socket_fd,
                                     const void *bytes,
                                     size_t length,
                                     void *opaque) {
    cancel_write_context_t *context = opaque;
    unsigned int call = atomic_fetch_add_explicit(
        &context->calls, 1, memory_order_relaxed);
    if (context->drop_first_success && call == 0) {
        return (ssize_t)length;
    }
    if (context->always_fail || (context->fail_first_with_eintr && call == 0)) {
        errno = context->always_fail ? EIO : EINTR;
        return -1;
    }
    return write(socket_fd, bytes, length);
}

static void test_cancel_retries_eintr_and_wakes_watcher(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    held_watch_context_t watch;
    memset(&watch, 0, sizeof(watch));
    assert(pthread_mutex_init(&watch.lock, NULL) == 0);
    assert(pthread_cond_init(&watch.changed, NULL) == 0);
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_held_watch, &watch);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &watch.peer,
    };
    fixture_configure(&fixture, &endpoint);
    cancel_write_context_t writer = {.fail_first_with_eintr = true};
    assert(repose_plugin_test_set_cancel_writer(
               fixture.mechanism, injected_cancel_write, &writer) ==
           errAuthorizationSuccess);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);

    async_interface_call_t deactivate;
    async_call_init(&deactivate, fixture.interface, fixture.mechanism);
    pthread_t thread;
    assert(pthread_create(&thread, NULL, deactivate_async, &deactivate) == 0);
    assert(async_call_wait_done(&deactivate, 500));
    assert(pthread_join(thread, NULL) == 0);
    assert(deactivate.status == errAuthorizationSuccess);
    assert(atomic_load_explicit(&writer.calls, memory_order_relaxed) == 2);

    held_watch_release(&watch);
    repose_test_server_join(&server);
    fixture_destroy(&fixture);
    async_call_destroy(&deactivate);
    assert(pthread_cond_destroy(&watch.changed) == 0);
    assert(pthread_mutex_destroy(&watch.lock) == 0);
}

static void test_cancel_write_failure_uses_shutdown_fallback(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    held_watch_context_t watch;
    memset(&watch, 0, sizeof(watch));
    assert(pthread_mutex_init(&watch.lock, NULL) == 0);
    assert(pthread_cond_init(&watch.changed, NULL) == 0);
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_held_watch, &watch);
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &watch.peer,
    };
    fixture_configure(&fixture, &endpoint);
    cancel_write_context_t writer = {.always_fail = true};
    assert(repose_plugin_test_set_cancel_writer(
               fixture.mechanism, injected_cancel_write, &writer) ==
           errAuthorizationSuccess);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);

    async_interface_call_t deactivate;
    async_call_init(&deactivate, fixture.interface, fixture.mechanism);
    pthread_t thread;
    assert(pthread_create(&thread, NULL, deactivate_async, &deactivate) == 0);
    assert(async_call_wait_done(&deactivate, 500));
    assert(pthread_join(thread, NULL) == 0);
    assert(deactivate.status == errAuthorizationSuccess);
    assert(atomic_load_explicit(&writer.calls, memory_order_relaxed) == 1);

    held_watch_release(&watch);
    repose_test_server_join(&server);
    fixture_destroy(&fixture);
    async_call_destroy(&deactivate);
    assert(pthread_cond_destroy(&watch.changed) == 0);
    assert(pthread_mutex_destroy(&watch.lock) == 0);
}

static void serve_denied(int socket_fd,
                         size_t connection_index,
                         void *opaque) {
    (void)connection_index;
    plugin_peer_context_t *peer = opaque;
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(atomic_load_explicit(&peer->verified, memory_order_acquire));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_DENIED, 42, 0x5a, 0);
    assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
}

static void test_concurrent_invokes_share_single_watcher_join(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    held_watch_context_t held;
    memset(&held, 0, sizeof(held));
    assert(pthread_mutex_init(&held.lock, NULL) == 0);
    assert(pthread_cond_init(&held.changed, NULL) == 0);
    repose_test_server_t watch_server;
    repose_test_server_start(&watch_server, 1, serve_held_watch, &held);
    repose_ipc_test_endpoint_t watch_endpoint = {
        .socket_path = watch_server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &held.peer,
    };
    fixture_configure(&fixture, &watch_endpoint);
    cancel_write_context_t writer = {.drop_first_success = true};
    assert(repose_plugin_test_set_cancel_writer(
               fixture.mechanism, injected_cancel_write, &writer) ==
           errAuthorizationSuccess);
    assert(fixture.interface->MechanismInvoke(fixture.mechanism) ==
           errAuthorizationSuccess);

    plugin_peer_context_t denied_peer = {0};
    repose_test_server_t denied_server;
    repose_test_server_start(&denied_server, 1, serve_denied, &denied_peer);
    repose_ipc_test_endpoint_t denied_endpoint = {
        .socket_path = denied_server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &denied_peer,
    };
    fixture_configure(&fixture, &denied_endpoint);
    async_interface_call_t first;
    async_interface_call_t second;
    async_call_init(&first, fixture.interface, fixture.mechanism);
    async_call_init(&second, fixture.interface, fixture.mechanism);
    pthread_t first_thread;
    pthread_t second_thread;
    assert(pthread_create(&first_thread, NULL, invoke_async, &first) == 0);
    uint64_t cancel_deadline = monotonic_milliseconds() + 500;
    while (atomic_load_explicit(&writer.calls, memory_order_relaxed) < 1) {
        assert(monotonic_milliseconds() < cancel_deadline);
        sleep_milliseconds(1);
    }
    assert(pthread_create(&second_thread, NULL, invoke_async, &second) == 0);
    assert(pthread_join(first_thread, NULL) == 0);
    assert(pthread_join(second_thread, NULL) == 0);
    assert(first.status == errAuthorizationSuccess);
    assert(second.status == errAuthorizationSuccess);
    assert(atomic_load_explicit(&writer.calls, memory_order_relaxed) == 2);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultDeny) == 3);
    assert(repose_fake_engine_interrupt_count(&fixture.engine) == 0);

    held_watch_release(&held);
    repose_test_server_join(&watch_server);
    repose_test_server_join(&denied_server);
    fixture_destroy(&fixture);
    async_call_destroy(&second);
    async_call_destroy(&first);
    assert(pthread_cond_destroy(&held.changed) == 0);
    assert(pthread_mutex_destroy(&held.lock) == 0);
}

typedef struct invoke_admission_gate {
    pthread_mutex_t lock;
    pthread_cond_t changed;
    size_t calls;
    bool first_entered;
    bool release_first;
} invoke_admission_gate_t;

static void block_first_invoke_after_admission(void *opaque) {
    invoke_admission_gate_t *gate = opaque;
    pthread_mutex_lock(&gate->lock);
    gate->calls += 1;
    if (gate->calls == 1) {
        gate->first_entered = true;
        pthread_cond_broadcast(&gate->changed);
        while (!gate->release_first) {
            assert(pthread_cond_wait(&gate->changed, &gate->lock) == 0);
        }
    }
    pthread_mutex_unlock(&gate->lock);
}

static bool wait_for_first_invoke_admission(invoke_admission_gate_t *gate,
                                            uint32_t timeout_ms) {
    struct timespec deadline = realtime_deadline(timeout_ms);
    pthread_mutex_lock(&gate->lock);
    while (!gate->first_entered) {
        int status = pthread_cond_timedwait(&gate->changed, &gate->lock, &deadline);
        if (status == ETIMEDOUT) {
            pthread_mutex_unlock(&gate->lock);
            return false;
        }
        assert(status == 0);
    }
    pthread_mutex_unlock(&gate->lock);
    return true;
}

static void release_first_invoke_admission(invoke_admission_gate_t *gate) {
    pthread_mutex_lock(&gate->lock);
    gate->release_first = true;
    pthread_cond_broadcast(&gate->changed);
    pthread_mutex_unlock(&gate->lock);
}

typedef struct gated_ready_context {
    plugin_peer_context_t peer;
    pthread_mutex_t lock;
    pthread_cond_t changed;
    bool send_ready;
} gated_ready_context_t;

static void serve_watching_then_gated_ready(int socket_fd,
                                            size_t connection_index,
                                            void *opaque) {
    gated_ready_context_t *context = opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(atomic_load_explicit(&context->peer.verified, memory_order_acquire));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
    assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
    pthread_mutex_lock(&context->lock);
    struct timespec deadline = realtime_deadline(2000);
    while (!context->send_ready) {
        int status = pthread_cond_timedwait(&context->changed,
                                            &context->lock,
                                            &deadline);
        if (status == ETIMEDOUT) {
            break;
        }
        assert(status == 0);
    }
    bool send_ready = context->send_ready;
    pthread_mutex_unlock(&context->lock);
    if (send_ready) {
        request.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] =
            REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
        request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] =
            REPOSE_UNLOCK_IPC_STATUS_EVENT;
        assert(repose_test_write_frame(socket_fd,
                                       &request,
                                       REPOSE_UNLOCK_IPC_FRAME_LEN));
    }
}

static void gated_ready_send(gated_ready_context_t *context) {
    pthread_mutex_lock(&context->lock);
    context->send_ready = true;
    pthread_cond_broadcast(&context->changed);
    pthread_mutex_unlock(&context->lock);
}

static void test_older_cancel_does_not_kill_newer_watcher(void) {
    plugin_fixture_t fixture;
    fixture_init(&fixture);
    invoke_admission_gate_t admission;
    memset(&admission, 0, sizeof(admission));
    assert(pthread_mutex_init(&admission.lock, NULL) == 0);
    assert(pthread_cond_init(&admission.changed, NULL) == 0);
    assert(repose_plugin_test_set_after_invoke_admitted_hook(
               fixture.mechanism,
               block_first_invoke_after_admission,
               &admission) == errAuthorizationSuccess);

    char old_directory_template[] = "/tmp/repose-old-invoke.XXXXXX";
    char *old_directory = mkdtemp(old_directory_template);
    assert(old_directory != NULL);
    char old_socket_path[104];
    int old_path_length = snprintf(old_socket_path,
                                   sizeof(old_socket_path),
                                   "%s/consume.sock",
                                   old_directory);
    assert(old_path_length > 0 &&
           (size_t)old_path_length < sizeof(old_socket_path));
    plugin_peer_context_t old_peer = {0};
    repose_ipc_test_endpoint_t old_endpoint = {
        .socket_path = old_socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &old_peer,
    };
    fixture_configure(&fixture, &old_endpoint);
    async_interface_call_t older;
    async_call_init(&older, fixture.interface, fixture.mechanism);
    pthread_t older_thread;
    assert(pthread_create(&older_thread, NULL, invoke_async, &older) == 0);
    assert(wait_for_first_invoke_admission(&admission, 500));

    gated_ready_context_t ready;
    memset(&ready, 0, sizeof(ready));
    assert(pthread_mutex_init(&ready.lock, NULL) == 0);
    assert(pthread_cond_init(&ready.changed, NULL) == 0);
    repose_test_server_t ready_server;
    repose_test_server_start(&ready_server, 1, serve_watching_then_gated_ready, &ready);
    repose_ipc_test_endpoint_t ready_endpoint = {
        .socket_path = ready_server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &ready.peer,
    };
    fixture_configure(&fixture, &ready_endpoint);
    async_interface_call_t newer;
    async_call_init(&newer, fixture.interface, fixture.mechanism);
    pthread_t newer_thread;
    assert(pthread_create(&newer_thread, NULL, invoke_async, &newer) == 0);
    assert(pthread_join(newer_thread, NULL) == 0);
    assert(newer.status == errAuthorizationSuccess);
    assert(repose_plugin_test_wait_for_generation(fixture.mechanism, 3, 500));

    release_first_invoke_admission(&admission);
    assert(async_call_wait_done(&older, 500));
    assert(pthread_join(older_thread, NULL) == 0);
    assert(older.status == errAuthorizationSuccess);
    gated_ready_send(&ready);
    assert(repose_fake_engine_wait_for_interrupts(&fixture.engine, 1, 500));
    assert(!repose_fake_engine_wait_for_interrupts(&fixture.engine, 2, 100));
    assert(repose_fake_engine_interrupt_count(&fixture.engine) == 1);
    assert(repose_fake_engine_result_count(&fixture.engine,
                                           kAuthorizationResultDeny) == 2);

    assert(rmdir(old_directory) == 0);
    repose_test_server_join(&ready_server);
    fixture_destroy(&fixture);
    async_call_destroy(&newer);
    async_call_destroy(&older);
    assert(pthread_cond_destroy(&ready.changed) == 0);
    assert(pthread_mutex_destroy(&ready.lock) == 0);
    assert(pthread_cond_destroy(&admission.changed) == 0);
    assert(pthread_mutex_destroy(&admission.lock) == 0);
}

int main(void) {
    test_create_failure_clears_outputs();
    test_consumed_maps_to_exactly_one_allow();
    test_unconfigured_test_build_denies_without_production_fallback();
    test_invoke_after_deactivate_is_rejected_without_callback();
    test_transport_and_correlation_failures_deny_exactly_once();
    test_ready_buffered_before_deny_interrupts_once_after_deny();
    test_set_result_error_prevents_watcher_spawn();
    test_ready_after_deny_interrupts_once();
    test_request_interrupt_error_is_not_retried();
    test_ready_edge_never_allows_until_second_invoke_consumes();
    test_older_invoke_cannot_revive_watcher_after_newer_invoke();
    test_new_invoke_invalidates_ready_decoded_by_old_watcher();
    test_deactivate_waits_for_synchronous_invoke_callback();
    test_destroy_waits_for_synchronous_invoke_callback();
    test_destroy_waits_for_pending_ready_callback();
    test_concurrent_deactivate_calls_both_wait_for_single_callback();
    test_cancel_retries_eintr_and_wakes_watcher();
    test_cancel_write_failure_uses_shutdown_fallback();
    test_concurrent_invokes_share_single_watcher_join();
    test_older_cancel_does_not_kill_newer_watcher();
    puts("plugin lifecycle tests: ok");
    return 0;
}
