#include "ipc_client.h"
#include "test_server.h"

#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

static void sleep_milliseconds(uint32_t milliseconds);
static uint64_t monotonic_milliseconds(void);

static uint32_t read_u32_be(const uint8_t *bytes) {
    return ((uint32_t)bytes[0] << 24) | ((uint32_t)bytes[1] << 16) |
           ((uint32_t)bytes[2] << 8) | (uint32_t)bytes[3];
}

static void write_u32_be(uint8_t *bytes, uint32_t value) {
    bytes[0] = (uint8_t)(value >> 24);
    bytes[1] = (uint8_t)(value >> 16);
    bytes[2] = (uint8_t)(value >> 8);
    bytes[3] = (uint8_t)value;
}

static void write_u64_be(uint8_t *bytes, uint64_t value) {
    for (size_t index = 0; index < 8; index += 1) {
        bytes[index] = (uint8_t)(value >> (56 - (index * 8)));
    }
}

static repose_unlock_ipc_frame_t reply_frame(uint8_t status,
                                              const uint8_t nonce[32],
                                              uint64_t watch_id) {
    repose_unlock_ipc_frame_t frame;
    assert(repose_ipc_encode_request(
        &frame,
        nonce,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77}));
    frame.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = status;
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET], 42);
    memset(&frame.bytes[REPOSE_UNLOCK_IPC_INSTANCE_OFFSET], 0x5a, 16);
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_WATCH_ID_OFFSET], watch_id);
    return frame;
}

static void test_reply_validation(const uint8_t nonce[32]) {
    repose_session_selector_t selector = {.console_uid = 501, .audit_session_id = 77};
    repose_ipc_reply_t reply;

    repose_unlock_ipc_frame_t frame =
        reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    assert(repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    assert(reply.kind == REPOSE_IPC_REPLY_CONSUMED);
    assert(reply.correlation.lock_epoch == 42);
    assert(reply.correlation.watch_id == 0);

    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_WATCHING, nonce, 9);
    assert(repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    assert(reply.kind == REPOSE_IPC_REPLY_WATCHING);
    assert(reply.correlation.watch_id == 9);

    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_DENIED, nonce, 0);
    assert(repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    assert(reply.kind == REPOSE_IPC_REPLY_DENIED);

    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    frame.bytes[REPOSE_UNLOCK_IPC_FLAGS_OFFSET] = 1;
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    frame.bytes[REPOSE_UNLOCK_IPC_VERSION_OFFSET] += 1;
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    write_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_PAYLOAD_LEN_OFFSET], 71);
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    frame.bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET] ^= 1;
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    write_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET], 502);
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    write_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET], 78);
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    frame.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] =
        REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(0xff, nonce, 0);
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET], 0);
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    memset(&frame.bytes[REPOSE_UNLOCK_IPC_INSTANCE_OFFSET], 0, 16);
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 9);
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_WATCHING, nonce, 0);
    assert(!repose_ipc_decode_reply(&frame, nonce, selector, &reply));

    uint8_t zero_nonce[32] = {0};
    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_CONSUMED, nonce, 0);
    memset(&frame.bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET], 0, 32);
    assert(!repose_ipc_decode_reply(&frame, zero_nonce, selector, &reply));
}

static void test_watch_validation(const uint8_t nonce[32]) {
    repose_session_selector_t selector = {.console_uid = 501, .audit_session_id = 77};
    repose_unlock_ipc_frame_t frame =
        reply_frame(REPOSE_UNLOCK_IPC_STATUS_WATCHING, nonce, 9);
    repose_ipc_reply_t reply;
    assert(repose_ipc_decode_reply(&frame, nonce, selector, &reply));

    frame.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] = REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
    frame.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_KEEPALIVE;
    repose_ipc_watch_frame_kind_t kind;
    assert(repose_ipc_decode_watch_frame(&frame, &reply.correlation, &kind));
    assert(kind == REPOSE_IPC_WATCH_KEEPALIVE);
    frame.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
    assert(repose_ipc_decode_watch_frame(&frame, &reply.correlation, &kind));
    assert(kind == REPOSE_IPC_WATCH_READY);

    frame.bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET] ^= 1;
    assert(!repose_ipc_decode_watch_frame(&frame, &reply.correlation, &kind));
    frame.bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET] ^= 1;
    write_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET], 502);
    assert(!repose_ipc_decode_watch_frame(&frame, &reply.correlation, &kind));
    write_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET], 501);
    write_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET], 78);
    assert(!repose_ipc_decode_watch_frame(&frame, &reply.correlation, &kind));
    write_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET], 77);
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET], 43);
    assert(!repose_ipc_decode_watch_frame(&frame, &reply.correlation, &kind));
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET], 42);
    frame.bytes[REPOSE_UNLOCK_IPC_INSTANCE_OFFSET] ^= 1;
    assert(!repose_ipc_decode_watch_frame(&frame, &reply.correlation, &kind));
    frame.bytes[REPOSE_UNLOCK_IPC_INSTANCE_OFFSET] ^= 1;
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_WATCH_ID_OFFSET], 10);
    assert(!repose_ipc_decode_watch_frame(&frame, &reply.correlation, &kind));
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_WATCH_ID_OFFSET], 9);
    frame.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_CONSUMED;
    assert(!repose_ipc_decode_watch_frame(&frame, &reply.correlation, &kind));

    frame = reply_frame(REPOSE_UNLOCK_IPC_STATUS_WATCHING, nonce, 9);
    assert(repose_ipc_decode_reply(&frame, nonce, selector, &reply));
    frame.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] = REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
    frame.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
    repose_ipc_correlation_t invalid = reply.correlation;
    memset(invalid.nonce, 0, sizeof(invalid.nonce));
    memset(&frame.bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET], 0, 32);
    assert(!repose_ipc_decode_watch_frame(&frame, &invalid, &kind));
    invalid = reply.correlation;
    invalid.lock_epoch = 0;
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET], 0);
    memcpy(&frame.bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET], nonce, 32);
    assert(!repose_ipc_decode_watch_frame(&frame, &invalid, &kind));
    invalid = reply.correlation;
    memset(invalid.service_instance, 0, sizeof(invalid.service_instance));
    memset(&frame.bytes[REPOSE_UNLOCK_IPC_INSTANCE_OFFSET], 0, 16);
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET], 42);
    assert(!repose_ipc_decode_watch_frame(&frame, &invalid, &kind));
    invalid = reply.correlation;
    invalid.watch_id = 0;
    write_u64_be(&frame.bytes[REPOSE_UNLOCK_IPC_WATCH_ID_OFFSET], 0);
    memset(&frame.bytes[REPOSE_UNLOCK_IPC_INSTANCE_OFFSET], 0x5a, 16);
    assert(!repose_ipc_decode_watch_frame(&frame, &invalid, &kind));
}

typedef struct consumed_context {
    atomic_bool peer_verified;
    size_t request_bytes;
    bool saw_eof;
} consumed_context_t;

static bool accept_test_peer(int socket_fd, void *opaque) {
    (void)socket_fd;
    consumed_context_t *context = opaque;
    atomic_store(&context->peer_verified, true);
    return true;
}

static void serve_consumed(int socket_fd, size_t connection_index, void *opaque) {
    assert(connection_index == 0);
    consumed_context_t *context = opaque;
    repose_unlock_ipc_frame_t request;
    assert(repose_test_read_request(socket_fd,
                                    &request,
                                    &context->request_bytes,
                                    &context->saw_eof));
    assert(atomic_load(&context->peer_verified));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_CONSUMED, 42, 0x5a, 0);
    assert(repose_test_write_frame(socket_fd, &request, 3));
}

static void test_consumed_transport(void) {
    consumed_context_t context = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_consumed, &context);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context,
    };
    assert(repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    assert(result.reply.kind == REPOSE_IPC_REPLY_CONSUMED);
    assert(result.watch_socket == -1);
    repose_ipc_initial_result_close(&result);
    repose_test_server_join(&server);
    assert(context.request_bytes == REPOSE_UNLOCK_IPC_FRAME_LEN);
    assert(context.saw_eof);
}

static void delay_after_decode(void *opaque) {
    uint32_t *milliseconds = opaque;
    sleep_milliseconds(*milliseconds);
}

typedef struct rejected_peer_context {
    size_t request_bytes;
    bool saw_eof;
} rejected_peer_context_t;

static void observe_rejected_peer(int socket_fd, size_t connection_index, void *opaque) {
    assert(connection_index == 0);
    rejected_peer_context_t *context = opaque;
    repose_unlock_ipc_frame_t request;
    (void)repose_test_read_request(socket_fd,
                                   &request,
                                   &context->request_bytes,
                                   &context->saw_eof);
}

typedef struct shutdown_deadline_context {
    uint32_t delay_ms;
    atomic_uint shutdown_calls;
} shutdown_deadline_context_t;

static void delay_after_request_write(void *opaque) {
    shutdown_deadline_context_t *context = opaque;
    sleep_milliseconds(context->delay_ms);
}

static void record_request_shutdown(void *opaque) {
    shutdown_deadline_context_t *context = opaque;
    atomic_fetch_add_explicit(&context->shutdown_calls, 1, memory_order_relaxed);
}

static void test_expired_request_is_not_half_closed(void) {
    consumed_context_t peer = {0};
    rejected_peer_context_t observation = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, observe_rejected_peer, &observation);
    shutdown_deadline_context_t context = {.delay_ms = 120};
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
        .after_request_written = delay_after_request_write,
        .after_request_written_context = &context,
        .before_request_shutdown = record_request_shutdown,
        .before_request_shutdown_context = &context,
    };
    assert(!repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    assert(atomic_load_explicit(&context.shutdown_calls, memory_order_relaxed) == 0);
    repose_test_server_join(&server);
    assert(observation.request_bytes == REPOSE_UNLOCK_IPC_FRAME_LEN);
}

static void test_decoded_reply_cannot_succeed_after_absolute_deadline(void) {
    consumed_context_t context = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_consumed, &context);
    uint32_t decode_delay = 120;
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context,
        .after_reply_decoded = delay_after_decode,
        .after_reply_decoded_context = &decode_delay,
    };
    assert(!repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    assert(result.watch_socket == -1);
    repose_test_server_join(&server);
}

static void test_non_root_peer_is_rejected_before_request_bytes(void) {
    assert(geteuid() != 0);
    rejected_peer_context_t context = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, observe_rejected_peer, &context);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = repose_ipc_test_verify_root_peer,
        .verify_peer_context = NULL,
    };
    assert(!repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    repose_test_server_join(&server);
    assert(context.request_bytes == 0);
    assert(context.saw_eof);
}

static bool accept_peer_after_deadline(int socket_fd, void *opaque) {
    (void)socket_fd;
    atomic_bool *verified = opaque;
    sleep_milliseconds(120);
    atomic_store_explicit(verified, true, memory_order_release);
    return true;
}

static void test_slow_peer_verification_uses_original_deadline_and_sends_nothing(void) {
    rejected_peer_context_t observation = {0};
    atomic_bool verified = false;
    repose_test_server_t server;
    repose_test_server_start(&server, 1, observe_rejected_peer, &observation);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_peer_after_deadline,
        .verify_peer_context = &verified,
    };
    uint64_t started = monotonic_milliseconds();
    assert(!repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    assert(monotonic_milliseconds() - started >= 100);
    repose_test_server_join(&server);
    assert(atomic_load_explicit(&verified, memory_order_acquire));
    assert(observation.request_bytes == 0);
    assert(observation.saw_eof);
}

typedef struct slow_context {
    uint32_t initial_delay_ms;
    uint32_t per_byte_delay_ms;
} slow_context_t;

static void sleep_milliseconds(uint32_t milliseconds) {
    struct timespec duration = {
        .tv_sec = (time_t)(milliseconds / 1000),
        .tv_nsec = (long)(milliseconds % 1000) * 1000000L,
    };
    while (nanosleep(&duration, &duration) != 0) {
        assert(errno == EINTR);
    }
}

static void serve_slow_reply(int socket_fd, size_t connection_index, void *opaque) {
    assert(connection_index == 0);
    slow_context_t *context = opaque;
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    assert(request_bytes == REPOSE_UNLOCK_IPC_FRAME_LEN && saw_eof);
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_CONSUMED, 42, 0x5a, 0);
    sleep_milliseconds(context->initial_delay_ms);
    for (size_t index = 0; index < REPOSE_UNLOCK_IPC_FRAME_LEN; index += 1) {
        if (write(socket_fd, &request.bytes[index], 1) != 1) {
            return;
        }
        sleep_milliseconds(context->per_byte_delay_ms);
    }
}

static uint64_t monotonic_milliseconds(void) {
    struct timespec now;
    assert(clock_gettime(CLOCK_MONOTONIC, &now) == 0);
    return (uint64_t)now.tv_sec * 1000 + (uint64_t)now.tv_nsec / 1000000;
}

static void test_partial_progress_does_not_renew_initial_deadline(void) {
    slow_context_t context = {.initial_delay_ms = 60, .per_byte_delay_ms = 10};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_slow_reply, &context);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &(consumed_context_t){0},
    };
    uint64_t started = monotonic_milliseconds();
    assert(!repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    uint64_t elapsed = monotonic_milliseconds() - started;
    assert(elapsed >= 80 && elapsed <= 160);
    repose_test_server_join(&server);
}

static void serve_bad_reply_status_without_body(int socket_fd,
                                                size_t connection_index,
                                                void *opaque) {
    (void)opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    repose_test_make_reply(&request, 0xff, 42, 0x5a, 0);
    assert(write(socket_fd, request.bytes, REPOSE_UNLOCK_IPC_HEADER_LEN) ==
           REPOSE_UNLOCK_IPC_HEADER_LEN);
    sleep_milliseconds(200);
}

static void test_bad_reply_status_is_rejected_without_waiting_for_body(void) {
    consumed_context_t context = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_bad_reply_status_without_body, NULL);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context,
    };
    uint64_t started = monotonic_milliseconds();
    assert(!repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    assert(monotonic_milliseconds() - started < 50);
    repose_test_server_join(&server);
}

static void test_failure_initializes_result_to_closed(void) {
    char directory_template[] = "/tmp/repose-auth-missing.XXXXXX";
    char *directory = mkdtemp(directory_template);
    assert(directory != NULL);
    char missing_path[104];
    int length = snprintf(missing_path,
                          sizeof(missing_path),
                          "%s/consume.sock",
                          directory);
    assert(length > 0 && (size_t)length < sizeof(missing_path));
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    memset(&result, 0xa5, sizeof(result));
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = missing_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &(consumed_context_t){0},
    };
    assert(!repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    assert(result.watch_socket == -1);
    assert(result.reply.kind == 0);
    assert(rmdir(directory) == 0);
}

static void test_null_endpoint_initializes_result_to_closed(void) {
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    memset(&result, 0xa5, sizeof(result));
    assert(!repose_ipc_test_exchange(
        NULL,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    assert(result.watch_socket == -1);
    assert(result.reply.kind == 0);
}

typedef struct terminal_reply_context {
    bool trailing_byte;
    uint32_t close_delay_ms;
} terminal_reply_context_t;

static void serve_terminal_reply_variant(int socket_fd,
                                         size_t connection_index,
                                         void *opaque) {
    terminal_reply_context_t *context = opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_CONSUMED, 42, 0x5a, 0);
    assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
    if (context->trailing_byte) {
        assert(write(socket_fd, "x", 1) == 1);
    }
    sleep_milliseconds(context->close_delay_ms);
}

static void assert_terminal_variant_is_rejected(terminal_reply_context_t *context) {
    consumed_context_t peer = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_terminal_reply_variant, context);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
    };
    assert(!repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    assert(result.watch_socket == -1);
    repose_test_server_join(&server);
}

static void test_terminal_reply_rejects_trailing_byte_and_late_eof(void) {
    terminal_reply_context_t trailing = {.trailing_byte = true};
    assert_terminal_variant_is_rejected(&trailing);
    terminal_reply_context_t late_eof = {.close_delay_ms = 120};
    assert_terminal_variant_is_rejected(&late_eof);
}

static void serve_keepalive_then_ready(int socket_fd,
                                       size_t connection_index,
                                       void *opaque) {
    (void)opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
    assert(repose_test_write_frame(socket_fd, &request, 84));
    request.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] = REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
    request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_KEEPALIVE;
    assert(repose_test_write_frame(socket_fd, &request, 5));
    request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
    assert(repose_test_write_frame(socket_fd, &request, 7));
}

static void test_watch_ignores_correlated_keepalive_then_reports_ready(void) {
    consumed_context_t context = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_keepalive_then_ready, NULL);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context,
    };
    assert(repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    assert(result.reply.kind == REPOSE_IPC_REPLY_WATCHING);
    assert(result.watch_socket >= 0);
    assert((fcntl(result.watch_socket, F_GETFL) & O_NONBLOCK) != 0);
    assert((fcntl(result.watch_socket, F_GETFD) & FD_CLOEXEC) != 0);
    int no_sigpipe = 0;
    socklen_t option_length = (socklen_t)sizeof(no_sigpipe);
    assert(getsockopt(result.watch_socket,
                      SOL_SOCKET,
                      SO_NOSIGPIPE,
                      &no_sigpipe,
                      &option_length) == 0);
    assert(no_sigpipe == 1);
    int cancel_pipe[2];
    assert(pipe(cancel_pipe) == 0);
    assert(repose_ipc_watch_wait(result.watch_socket,
                                 cancel_pipe[0],
                                 &result.reply.correlation) ==
           REPOSE_IPC_WATCH_RESULT_READY);
    close(cancel_pipe[0]);
    close(cancel_pipe[1]);
    repose_ipc_initial_result_close(&result);
    repose_test_server_join(&server);
}

static void serve_bad_watch_header_without_body(int socket_fd,
                                                size_t connection_index,
                                                void *opaque) {
    (void)opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
    assert(repose_test_write_frame(socket_fd, &request, 84));
    request.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] = REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
    request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
    request.bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET] ^= 1;
    assert(write(socket_fd, request.bytes, REPOSE_UNLOCK_IPC_HEADER_LEN) ==
           REPOSE_UNLOCK_IPC_HEADER_LEN);
    sleep_milliseconds(200);
}

static void test_bad_watch_header_is_rejected_without_waiting_for_body(void) {
    consumed_context_t context = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_bad_watch_header_without_body, NULL);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context,
    };
    assert(repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    int cancel_pipe[2];
    assert(pipe(cancel_pipe) == 0);
    uint64_t started = monotonic_milliseconds();
    assert(repose_ipc_watch_wait(result.watch_socket,
                                 cancel_pipe[0],
                                 &result.reply.correlation) ==
           REPOSE_IPC_WATCH_RESULT_ENDED);
    uint64_t elapsed = monotonic_milliseconds() - started;
    assert(elapsed < 50);
    close(cancel_pipe[0]);
    close(cancel_pipe[1]);
    repose_ipc_initial_result_close(&result);
    repose_test_server_join(&server);
}

static void serve_stalled_watch_frame(int socket_fd,
                                      size_t connection_index,
                                      void *opaque) {
    (void)opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
    assert(repose_test_write_frame(socket_fd, &request, 84));
    request.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] = REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
    request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
    assert(write(socket_fd, request.bytes, REPOSE_UNLOCK_IPC_HEADER_LEN + 1) ==
           REPOSE_UNLOCK_IPC_HEADER_LEN + 1);
    sleep_milliseconds(200);
}

static void test_started_watch_frame_must_finish_within_one_hundred_ms(void) {
    consumed_context_t context = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_stalled_watch_frame, NULL);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &context,
    };
    assert(repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    int cancel_pipe[2];
    assert(pipe(cancel_pipe) == 0);
    uint64_t started = monotonic_milliseconds();
    assert(repose_ipc_watch_wait(result.watch_socket,
                                 cancel_pipe[0],
                                 &result.reply.correlation) ==
           REPOSE_IPC_WATCH_RESULT_ENDED);
    uint64_t elapsed = monotonic_milliseconds() - started;
    assert(elapsed >= 80 && elapsed <= 160);
    close(cancel_pipe[0]);
    close(cancel_pipe[1]);
    repose_ipc_initial_result_close(&result);
    repose_test_server_join(&server);
}

typedef struct partial_cancel_context {
    pthread_mutex_t lock;
    pthread_cond_t changed;
    bool partial_sent;
    bool release;
} partial_cancel_context_t;

static void serve_partial_frame_until_cancel(int socket_fd,
                                             size_t connection_index,
                                             void *opaque) {
    partial_cancel_context_t *context = opaque;
    assert(connection_index == 0);
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
    assert(repose_test_write_frame(socket_fd, &request, REPOSE_UNLOCK_IPC_FRAME_LEN));
    request.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] =
        REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE;
    request.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = REPOSE_UNLOCK_IPC_STATUS_EVENT;
    assert(write(socket_fd, request.bytes, REPOSE_UNLOCK_IPC_HEADER_LEN + 1) ==
           REPOSE_UNLOCK_IPC_HEADER_LEN + 1);
    pthread_mutex_lock(&context->lock);
    context->partial_sent = true;
    pthread_cond_broadcast(&context->changed);
    struct timespec deadline;
    assert(clock_gettime(CLOCK_REALTIME, &deadline) == 0);
    deadline.tv_sec += 2;
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

typedef struct watch_wait_call {
    int watch_socket;
    int cancel_socket;
    const repose_ipc_correlation_t *correlation;
    repose_ipc_watch_result_t result;
} watch_wait_call_t;

static void *wait_for_watch_frame(void *opaque) {
    watch_wait_call_t *call = opaque;
    call->result = repose_ipc_watch_wait(
        call->watch_socket, call->cancel_socket, call->correlation);
    return NULL;
}

static void test_partial_watch_frame_can_be_cancelled_immediately(void) {
    partial_cancel_context_t partial;
    memset(&partial, 0, sizeof(partial));
    assert(pthread_mutex_init(&partial.lock, NULL) == 0);
    assert(pthread_cond_init(&partial.changed, NULL) == 0);
    consumed_context_t peer = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_partial_frame_until_cancel, &partial);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
    };
    assert(repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    pthread_mutex_lock(&partial.lock);
    while (!partial.partial_sent) {
        struct timespec wait_deadline;
        assert(clock_gettime(CLOCK_REALTIME, &wait_deadline) == 0);
        wait_deadline.tv_sec += 2;
        assert(pthread_cond_timedwait(&partial.changed,
                                      &partial.lock,
                                      &wait_deadline) == 0);
    }
    pthread_mutex_unlock(&partial.lock);
    int cancel_pipe[2];
    assert(pipe(cancel_pipe) == 0);
    watch_wait_call_t call = {
        .watch_socket = result.watch_socket,
        .cancel_socket = cancel_pipe[0],
        .correlation = &result.reply.correlation,
    };
    pthread_t watch_thread;
    assert(pthread_create(&watch_thread, NULL, wait_for_watch_frame, &call) == 0);
    uint64_t read_deadline = monotonic_milliseconds() + 100;
    for (;;) {
        int available = -1;
        assert(ioctl(result.watch_socket, FIONREAD, &available) == 0);
        if (available == 0) {
            break;
        }
        assert(monotonic_milliseconds() < read_deadline);
        sleep_milliseconds(1);
    }
    assert(write(cancel_pipe[1], "x", 1) == 1);
    uint64_t started = monotonic_milliseconds();
    assert(pthread_join(watch_thread, NULL) == 0);
    assert(call.result == REPOSE_IPC_WATCH_RESULT_CANCELLED);
    assert(monotonic_milliseconds() - started < 50);
    close(cancel_pipe[0]);
    close(cancel_pipe[1]);
    repose_ipc_initial_result_close(&result);
    pthread_mutex_lock(&partial.lock);
    partial.release = true;
    pthread_cond_broadcast(&partial.changed);
    pthread_mutex_unlock(&partial.lock);
    repose_test_server_join(&server);
    assert(pthread_cond_destroy(&partial.changed) == 0);
    assert(pthread_mutex_destroy(&partial.lock) == 0);
}

typedef struct held_watch_context {
    pthread_mutex_t lock;
    pthread_cond_t changed;
    bool reply_sent;
    bool close_allowed;
} held_watch_context_t;

static void serve_held_watch(int socket_fd, size_t connection_index, void *opaque) {
    assert(connection_index == 0);
    held_watch_context_t *context = opaque;
    repose_unlock_ipc_frame_t request;
    size_t request_bytes = 0;
    bool saw_eof = false;
    assert(repose_test_read_request(socket_fd, &request, &request_bytes, &saw_eof));
    repose_test_make_reply(&request, REPOSE_UNLOCK_IPC_STATUS_WATCHING, 42, 0x5a, 9);
    assert(repose_test_write_frame(socket_fd, &request, 84));
    pthread_mutex_lock(&context->lock);
    context->reply_sent = true;
    pthread_cond_broadcast(&context->changed);
    struct timespec deadline;
    assert(clock_gettime(CLOCK_REALTIME, &deadline) == 0);
    deadline.tv_sec += 2;
    while (!context->close_allowed) {
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

static void test_idle_watch_is_cancelled_without_waiting_for_network(void) {
    held_watch_context_t held;
    memset(&held, 0, sizeof(held));
    assert(pthread_mutex_init(&held.lock, NULL) == 0);
    assert(pthread_cond_init(&held.changed, NULL) == 0);
    consumed_context_t peer = {0};
    repose_test_server_t server;
    repose_test_server_start(&server, 1, serve_held_watch, &held);
    repose_deadline_t deadline;
    assert(repose_deadline_start(&deadline, 100));
    repose_ipc_initial_result_t result;
    repose_ipc_test_endpoint_t endpoint = {
        .socket_path = server.socket_path,
        .verify_peer = accept_test_peer,
        .verify_peer_context = &peer,
    };
    assert(repose_ipc_test_exchange(
        &endpoint,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77},
        &deadline,
        &result));
    pthread_mutex_lock(&held.lock);
    while (!held.reply_sent) {
        struct timespec deadline;
        assert(clock_gettime(CLOCK_REALTIME, &deadline) == 0);
        deadline.tv_sec += 2;
        assert(pthread_cond_timedwait(&held.changed, &held.lock, &deadline) == 0);
    }
    pthread_mutex_unlock(&held.lock);
    int cancel_pipe[2];
    assert(pipe(cancel_pipe) == 0);
    assert(write(cancel_pipe[1], "x", 1) == 1);
    assert(repose_ipc_watch_wait(result.watch_socket,
                                 cancel_pipe[0],
                                 &result.reply.correlation) ==
           REPOSE_IPC_WATCH_RESULT_CANCELLED);
    close(cancel_pipe[0]);
    close(cancel_pipe[1]);
    repose_ipc_initial_result_close(&result);
    pthread_mutex_lock(&held.lock);
    held.close_allowed = true;
    pthread_cond_broadcast(&held.changed);
    pthread_mutex_unlock(&held.lock);
    repose_test_server_join(&server);
    assert(pthread_cond_destroy(&held.changed) == 0);
    assert(pthread_mutex_destroy(&held.lock) == 0);
}

int main(void) {
    uint8_t nonce[32];
    memset(nonce, 0xa5, sizeof(nonce));
    repose_unlock_ipc_frame_t frame;
    memset(&frame, 0xff, sizeof(frame));

    assert(repose_ipc_encode_request(
        &frame,
        nonce,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77}));
    assert(memcmp(frame.bytes, "RPUI", 4) == 0);
    assert(frame.bytes[REPOSE_UNLOCK_IPC_VERSION_OFFSET] == REPOSE_UNLOCK_IPC_VERSION);
    assert(frame.bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] ==
           REPOSE_UNLOCK_IPC_OP_CONSUME_OR_WATCH);
    assert(frame.bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] ==
           REPOSE_UNLOCK_IPC_STATUS_REQUEST);
    assert(frame.bytes[REPOSE_UNLOCK_IPC_FLAGS_OFFSET] == 0);
    assert(read_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_PAYLOAD_LEN_OFFSET]) ==
           REPOSE_UNLOCK_IPC_PAYLOAD_LEN);
    assert(memcmp(&frame.bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET], nonce, sizeof(nonce)) == 0);
    assert(read_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET]) == 501);
    assert(read_u32_be(&frame.bytes[REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET]) == 77);
    for (size_t index = REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET;
         index < REPOSE_UNLOCK_IPC_FRAME_LEN;
         index += 1) {
        assert(frame.bytes[index] == 0);
    }

    test_reply_validation(nonce);
    test_watch_validation(nonce);
    test_consumed_transport();
    test_expired_request_is_not_half_closed();
    test_decoded_reply_cannot_succeed_after_absolute_deadline();
    test_non_root_peer_is_rejected_before_request_bytes();
    test_slow_peer_verification_uses_original_deadline_and_sends_nothing();
    test_partial_progress_does_not_renew_initial_deadline();
    test_bad_reply_status_is_rejected_without_waiting_for_body();
    test_failure_initializes_result_to_closed();
    test_null_endpoint_initializes_result_to_closed();
    test_terminal_reply_rejects_trailing_byte_and_late_eof();
    test_watch_ignores_correlated_keepalive_then_reports_ready();
    test_bad_watch_header_is_rejected_without_waiting_for_body();
    test_started_watch_frame_must_finish_within_one_hundred_ms();
    test_partial_watch_frame_can_be_cancelled_immediately();
    test_idle_watch_is_cancelled_without_waiting_for_network();

    memset(nonce, 0, sizeof(nonce));
    assert(!repose_ipc_encode_request(
        &frame,
        nonce,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 77}));
    memset(nonce, 0xa5, sizeof(nonce));
    assert(!repose_ipc_encode_request(
        &frame,
        nonce,
        (repose_session_selector_t){.console_uid = 0, .audit_session_id = 77}));
    assert(!repose_ipc_encode_request(
        &frame,
        nonce,
        (repose_session_selector_t){.console_uid = 501, .audit_session_id = 0}));
    puts("ipc request vector: ok");
    return 0;
}
