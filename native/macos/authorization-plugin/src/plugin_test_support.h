#ifndef REPOSE_AUTHORIZATION_PLUGIN_TEST_SUPPORT_H
#define REPOSE_AUTHORIZATION_PLUGIN_TEST_SUPPORT_H

#if !defined(REPOSE_UNLOCK_TESTING)
#error "plugin test support is unavailable in production builds"
#endif

#include "ipc_client.h"

#include <Security/AuthorizationPlugin.h>
#include <stddef.h>
#include <sys/types.h>

typedef ssize_t (*repose_plugin_test_cancel_write_fn)(int socket_fd,
                                                       const void *bytes,
                                                       size_t length,
                                                       void *context);

OSStatus repose_plugin_test_configure_mechanism(
    AuthorizationMechanismRef mechanism,
    const repose_ipc_test_endpoint_t *endpoint,
    repose_session_selector_t selector);
bool repose_plugin_test_wait_for_generation(AuthorizationMechanismRef mechanism,
                                            uint64_t minimum_generation,
                                            uint32_t timeout_ms);
bool repose_plugin_test_wait_for_deactivated(AuthorizationMechanismRef mechanism,
                                             uint32_t timeout_ms);
bool repose_plugin_test_wait_for_destroying(AuthorizationMechanismRef mechanism,
                                            uint32_t timeout_ms);
bool repose_plugin_test_wait_for_deactivate_waiters(
    AuthorizationMechanismRef mechanism,
    size_t minimum_waiters,
    uint32_t timeout_ms);
OSStatus repose_plugin_test_set_cancel_writer(
    AuthorizationMechanismRef mechanism,
    repose_plugin_test_cancel_write_fn writer,
    void *context);
OSStatus repose_plugin_test_set_after_invoke_admitted_hook(
    AuthorizationMechanismRef mechanism,
    void (*hook)(void *context),
    void *context);

#endif
