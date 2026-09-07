#ifndef REPOSE_AUTHORIZATION_IPC_CLIENT_H
#define REPOSE_AUTHORIZATION_IPC_CLIENT_H

#include "repose_unlock_ipc.h"
#include "deadline.h"

#include <stdbool.h>
#include <stdint.h>

#define REPOSE_UNLOCK_PRODUCTION_SOCKET_PATH \
    "/var/run/ai.repose.unlockd/consume.sock"

typedef struct repose_session_selector {
    uint32_t console_uid;
    uint32_t audit_session_id;
} repose_session_selector_t;

typedef enum repose_ipc_reply_kind {
    REPOSE_IPC_REPLY_CONSUMED = 1,
    REPOSE_IPC_REPLY_WATCHING = 2,
    REPOSE_IPC_REPLY_DENIED = 3,
} repose_ipc_reply_kind_t;

typedef struct repose_ipc_correlation {
    uint8_t nonce[32];
    repose_session_selector_t selector;
    uint64_t lock_epoch;
    uint8_t service_instance[16];
    uint64_t watch_id;
} repose_ipc_correlation_t;

typedef struct repose_ipc_reply {
    repose_ipc_reply_kind_t kind;
    repose_ipc_correlation_t correlation;
} repose_ipc_reply_t;

typedef enum repose_ipc_watch_frame_kind {
    REPOSE_IPC_WATCH_READY = 1,
    REPOSE_IPC_WATCH_KEEPALIVE = 2,
} repose_ipc_watch_frame_kind_t;

typedef struct repose_ipc_initial_result {
    repose_ipc_reply_t reply;
    int watch_socket;
} repose_ipc_initial_result_t;

typedef enum repose_ipc_watch_result {
    REPOSE_IPC_WATCH_RESULT_READY = 1,
    REPOSE_IPC_WATCH_RESULT_CANCELLED = 2,
    REPOSE_IPC_WATCH_RESULT_ENDED = 3,
} repose_ipc_watch_result_t;

bool repose_ipc_encode_request(repose_unlock_ipc_frame_t *out_frame,
                               const uint8_t nonce[32],
                               repose_session_selector_t selector);
bool repose_ipc_decode_reply(const repose_unlock_ipc_frame_t *frame,
                             const uint8_t expected_nonce[32],
                             repose_session_selector_t expected_selector,
                             repose_ipc_reply_t *out_reply);
bool repose_ipc_decode_watch_frame(const repose_unlock_ipc_frame_t *frame,
                                   const repose_ipc_correlation_t *expected,
                                   repose_ipc_watch_frame_kind_t *out_kind);
#if !defined(REPOSE_UNLOCK_TESTING)
bool repose_ipc_exchange(repose_session_selector_t selector,
                         const repose_deadline_t *deadline,
                         repose_ipc_initial_result_t *out_result);
#endif
void repose_ipc_initial_result_close(repose_ipc_initial_result_t *result);
repose_ipc_watch_result_t repose_ipc_watch_wait(
    int watch_socket,
    int cancel_socket,
    const repose_ipc_correlation_t *correlation);

#if defined(REPOSE_UNLOCK_TESTING)
typedef bool (*repose_ipc_test_peer_verifier_fn)(int socket_fd, void *context);

typedef struct repose_ipc_test_endpoint {
    const char *socket_path;
    repose_ipc_test_peer_verifier_fn verify_peer;
    void *verify_peer_context;
    void (*after_request_written)(void *context);
    void *after_request_written_context;
    void (*before_request_shutdown)(void *context);
    void *before_request_shutdown_context;
    void (*after_reply_decoded)(void *context);
    void *after_reply_decoded_context;
    void (*after_watch_ready_decoded)(void *context);
    void *after_watch_ready_decoded_context;
} repose_ipc_test_endpoint_t;

repose_ipc_watch_result_t repose_ipc_test_watch_wait(
    int watch_socket,
    int cancel_socket,
    const repose_ipc_correlation_t *correlation,
    void (*after_ready_decoded)(void *context),
    void *after_ready_decoded_context);

bool repose_ipc_test_exchange(const repose_ipc_test_endpoint_t *endpoint,
                              repose_session_selector_t selector,
                              const repose_deadline_t *deadline,
                              repose_ipc_initial_result_t *out_result);
bool repose_ipc_test_verify_root_peer(int socket_fd, void *unused_context);
#endif

#endif
