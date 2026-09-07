#include "ipc_client.h"

#include <Security/SecRandom.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stddef.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

static bool bytes_all_zero(const uint8_t *bytes, size_t length) {
    uint8_t combined = 0;
    for (size_t index = 0; index < length; index += 1) {
        combined |= bytes[index];
    }
    return combined == 0;
}

static void write_u32_be(uint8_t *bytes, uint32_t value) {
    bytes[0] = (uint8_t)(value >> 24);
    bytes[1] = (uint8_t)(value >> 16);
    bytes[2] = (uint8_t)(value >> 8);
    bytes[3] = (uint8_t)value;
}

static uint32_t read_u32_be(const uint8_t *bytes) {
    return ((uint32_t)bytes[0] << 24) | ((uint32_t)bytes[1] << 16) |
           ((uint32_t)bytes[2] << 8) | (uint32_t)bytes[3];
}

static uint64_t read_u64_be(const uint8_t *bytes) {
    uint64_t value = 0;
    for (size_t index = 0; index < 8; index += 1) {
        value = (value << 8) | bytes[index];
    }
    return value;
}

static bool wait_for_socket(int socket_fd,
                            short events,
                            const repose_deadline_t *deadline) {
    struct pollfd descriptor = {.fd = socket_fd, .events = events, .revents = 0};
    for (;;) {
        if (repose_deadline_expired(deadline)) {
            return false;
        }
        int status = poll(&descriptor, 1, repose_deadline_poll_timeout_ms(deadline));
        if (status > 0) {
            return (descriptor.revents & events) != 0;
        }
        if (status == 0) {
            return false;
        }
        if (errno != EINTR) {
            return false;
        }
    }
}

static bool configure_socket(int socket_fd) {
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

static int connect_until(const char *socket_path, const repose_deadline_t *deadline) {
    if (socket_path == NULL || socket_path[0] == '\0' ||
        strlen(socket_path) >= sizeof(((struct sockaddr_un *)0)->sun_path) ||
        repose_deadline_expired(deadline)) {
        return -1;
    }
    int socket_fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (socket_fd < 0 || !configure_socket(socket_fd)) {
        if (socket_fd >= 0) {
            close(socket_fd);
        }
        return -1;
    }
    struct sockaddr_un address;
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    memcpy(address.sun_path, socket_path, strlen(socket_path) + 1);
    address.sun_len = SUN_LEN(&address);
    int status = connect(socket_fd,
                         (const struct sockaddr *)&address,
                         (socklen_t)SUN_LEN(&address));
    if (status != 0 && errno != EINPROGRESS) {
        close(socket_fd);
        return -1;
    }
    if (status != 0) {
        if (!wait_for_socket(socket_fd, POLLOUT, deadline)) {
            close(socket_fd);
            return -1;
        }
        int socket_error = 0;
        socklen_t socket_error_length = (socklen_t)sizeof(socket_error);
        if (getsockopt(socket_fd,
                       SOL_SOCKET,
                       SO_ERROR,
                       &socket_error,
                       &socket_error_length) != 0 ||
            socket_error_length != (socklen_t)sizeof(socket_error) || socket_error != 0) {
            close(socket_fd);
            return -1;
        }
    }
    if (repose_deadline_expired(deadline)) {
        close(socket_fd);
        return -1;
    }
    return socket_fd;
}

static bool write_all_until(int socket_fd,
                            const uint8_t *bytes,
                            size_t length,
                            const repose_deadline_t *deadline) {
    size_t offset = 0;
    while (offset < length) {
        if (repose_deadline_expired(deadline)) {
            return false;
        }
        ssize_t count = write(socket_fd, &bytes[offset], length - offset);
        if (count > 0) {
            offset += (size_t)count;
            continue;
        }
        if (count < 0 && errno == EINTR) {
            continue;
        }
        if (count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK) &&
            wait_for_socket(socket_fd, POLLOUT, deadline)) {
            continue;
        }
        return false;
    }
    return !repose_deadline_expired(deadline);
}

static bool read_exact_until(int socket_fd,
                             uint8_t *bytes,
                             size_t length,
                             const repose_deadline_t *deadline) {
    size_t offset = 0;
    while (offset < length) {
        if (repose_deadline_expired(deadline)) {
            return false;
        }
        ssize_t count = read(socket_fd, &bytes[offset], length - offset);
        if (count > 0) {
            offset += (size_t)count;
            continue;
        }
        if (count < 0 && errno == EINTR) {
            continue;
        }
        if (count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK) &&
            wait_for_socket(socket_fd, POLLIN, deadline)) {
            continue;
        }
        return false;
    }
    return !repose_deadline_expired(deadline);
}

static bool read_terminal_eof_until(int socket_fd,
                                    const repose_deadline_t *deadline) {
    uint8_t extra;
    for (;;) {
        if (repose_deadline_expired(deadline)) {
            return false;
        }
        ssize_t count = read(socket_fd, &extra, sizeof(extra));
        if (count == 0) {
            return !repose_deadline_expired(deadline);
        }
        if (count > 0) {
            return false;
        }
        if (errno == EINTR) {
            continue;
        }
        if (errno != EAGAIN && errno != EWOULDBLOCK) {
            return false;
        }
        struct pollfd descriptor = {
            .fd = socket_fd,
            .events = POLLIN,
            .revents = 0,
        };
        int status;
        do {
            if (repose_deadline_expired(deadline)) {
                return false;
            }
            status = poll(&descriptor, 1, repose_deadline_poll_timeout_ms(deadline));
        } while (status < 0 && errno == EINTR);
        if (status <= 0 ||
            (descriptor.revents & (POLLIN | POLLHUP)) == 0) {
            return false;
        }
    }
}

static bool reply_header_valid(const repose_unlock_ipc_frame_t *frame) {
    uint8_t status = frame->bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET];
    return memcmp(&frame->bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET], "RPUI", 4) == 0 &&
           frame->bytes[REPOSE_UNLOCK_IPC_VERSION_OFFSET] == REPOSE_UNLOCK_IPC_VERSION &&
           frame->bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] ==
               REPOSE_UNLOCK_IPC_OP_CONSUME_OR_WATCH &&
           (status == REPOSE_UNLOCK_IPC_STATUS_CONSUMED ||
            status == REPOSE_UNLOCK_IPC_STATUS_WATCHING ||
            status == REPOSE_UNLOCK_IPC_STATUS_DENIED) &&
           frame->bytes[REPOSE_UNLOCK_IPC_FLAGS_OFFSET] == 0 &&
           read_u32_be(&frame->bytes[REPOSE_UNLOCK_IPC_PAYLOAD_LEN_OFFSET]) ==
               REPOSE_UNLOCK_IPC_PAYLOAD_LEN;
}

static bool watch_header_valid(const repose_unlock_ipc_frame_t *frame) {
    uint8_t status = frame->bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET];
    return memcmp(&frame->bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET], "RPUI", 4) == 0 &&
           frame->bytes[REPOSE_UNLOCK_IPC_VERSION_OFFSET] == REPOSE_UNLOCK_IPC_VERSION &&
           frame->bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] ==
               REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE &&
           (status == REPOSE_UNLOCK_IPC_STATUS_EVENT ||
            status == REPOSE_UNLOCK_IPC_STATUS_KEEPALIVE) &&
           frame->bytes[REPOSE_UNLOCK_IPC_FLAGS_OFFSET] == 0 &&
           read_u32_be(&frame->bytes[REPOSE_UNLOCK_IPC_PAYLOAD_LEN_OFFSET]) ==
               REPOSE_UNLOCK_IPC_PAYLOAD_LEN;
}

bool repose_ipc_encode_request(repose_unlock_ipc_frame_t *out_frame,
                               const uint8_t nonce[32],
                               repose_session_selector_t selector) {
    if (out_frame == NULL || nonce == NULL || bytes_all_zero(nonce, 32) ||
        selector.console_uid == 0 || selector.audit_session_id == 0) {
        return false;
    }
    memset(out_frame->bytes, 0, sizeof(out_frame->bytes));
    out_frame->bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET + 0] = REPOSE_UNLOCK_IPC_MAGIC_0;
    out_frame->bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET + 1] = REPOSE_UNLOCK_IPC_MAGIC_1;
    out_frame->bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET + 2] = REPOSE_UNLOCK_IPC_MAGIC_2;
    out_frame->bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET + 3] = REPOSE_UNLOCK_IPC_MAGIC_3;
    out_frame->bytes[REPOSE_UNLOCK_IPC_VERSION_OFFSET] = REPOSE_UNLOCK_IPC_VERSION;
    out_frame->bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] =
        REPOSE_UNLOCK_IPC_OP_CONSUME_OR_WATCH;
    out_frame->bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] =
        REPOSE_UNLOCK_IPC_STATUS_REQUEST;
    write_u32_be(&out_frame->bytes[REPOSE_UNLOCK_IPC_PAYLOAD_LEN_OFFSET],
                 REPOSE_UNLOCK_IPC_PAYLOAD_LEN);
    memcpy(&out_frame->bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET], nonce, 32);
    write_u32_be(&out_frame->bytes[REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET],
                 selector.console_uid);
    write_u32_be(&out_frame->bytes[REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET],
                 selector.audit_session_id);
    return true;
}

bool repose_ipc_decode_reply(const repose_unlock_ipc_frame_t *frame,
                             const uint8_t expected_nonce[32],
                             repose_session_selector_t expected_selector,
                             repose_ipc_reply_t *out_reply) {
    if (frame == NULL || expected_nonce == NULL || out_reply == NULL ||
        bytes_all_zero(expected_nonce, 32) || expected_selector.console_uid == 0 ||
        expected_selector.audit_session_id == 0 ||
        memcmp(&frame->bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET], "RPUI", 4) != 0 ||
        frame->bytes[REPOSE_UNLOCK_IPC_VERSION_OFFSET] != REPOSE_UNLOCK_IPC_VERSION ||
        frame->bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] !=
            REPOSE_UNLOCK_IPC_OP_CONSUME_OR_WATCH ||
        frame->bytes[REPOSE_UNLOCK_IPC_FLAGS_OFFSET] != 0 ||
        read_u32_be(&frame->bytes[REPOSE_UNLOCK_IPC_PAYLOAD_LEN_OFFSET]) !=
            REPOSE_UNLOCK_IPC_PAYLOAD_LEN ||
        memcmp(&frame->bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET], expected_nonce, 32) != 0 ||
        read_u32_be(&frame->bytes[REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET]) !=
            expected_selector.console_uid ||
        read_u32_be(&frame->bytes[REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET]) !=
            expected_selector.audit_session_id) {
        return false;
    }

    repose_ipc_reply_t reply;
    memset(&reply, 0, sizeof(reply));
    memcpy(reply.correlation.nonce, expected_nonce, sizeof(reply.correlation.nonce));
    reply.correlation.selector = expected_selector;
    reply.correlation.lock_epoch =
        read_u64_be(&frame->bytes[REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET]);
    memcpy(reply.correlation.service_instance,
           &frame->bytes[REPOSE_UNLOCK_IPC_INSTANCE_OFFSET],
           sizeof(reply.correlation.service_instance));
    reply.correlation.watch_id =
        read_u64_be(&frame->bytes[REPOSE_UNLOCK_IPC_WATCH_ID_OFFSET]);
    if (reply.correlation.lock_epoch == 0 ||
        bytes_all_zero(reply.correlation.service_instance,
                       sizeof(reply.correlation.service_instance))) {
        return false;
    }

    uint8_t status = frame->bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET];
    if (status == REPOSE_UNLOCK_IPC_STATUS_CONSUMED &&
        reply.correlation.watch_id == 0) {
        reply.kind = REPOSE_IPC_REPLY_CONSUMED;
    } else if (status == REPOSE_UNLOCK_IPC_STATUS_WATCHING &&
               reply.correlation.watch_id != 0) {
        reply.kind = REPOSE_IPC_REPLY_WATCHING;
    } else if (status == REPOSE_UNLOCK_IPC_STATUS_DENIED &&
               reply.correlation.watch_id == 0) {
        reply.kind = REPOSE_IPC_REPLY_DENIED;
    } else {
        return false;
    }
    *out_reply = reply;
    return true;
}

bool repose_ipc_decode_watch_frame(const repose_unlock_ipc_frame_t *frame,
                                   const repose_ipc_correlation_t *expected,
                                   repose_ipc_watch_frame_kind_t *out_kind) {
    if (frame == NULL || expected == NULL || out_kind == NULL ||
        bytes_all_zero(expected->nonce, sizeof(expected->nonce)) ||
        expected->selector.console_uid == 0 || expected->selector.audit_session_id == 0 ||
        expected->lock_epoch == 0 ||
        bytes_all_zero(expected->service_instance, sizeof(expected->service_instance)) ||
        expected->watch_id == 0 ||
        memcmp(&frame->bytes[REPOSE_UNLOCK_IPC_MAGIC_OFFSET], "RPUI", 4) != 0 ||
        frame->bytes[REPOSE_UNLOCK_IPC_VERSION_OFFSET] != REPOSE_UNLOCK_IPC_VERSION ||
        frame->bytes[REPOSE_UNLOCK_IPC_OPERATION_OFFSET] !=
            REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE ||
        frame->bytes[REPOSE_UNLOCK_IPC_FLAGS_OFFSET] != 0 ||
        read_u32_be(&frame->bytes[REPOSE_UNLOCK_IPC_PAYLOAD_LEN_OFFSET]) !=
            REPOSE_UNLOCK_IPC_PAYLOAD_LEN ||
        memcmp(&frame->bytes[REPOSE_UNLOCK_IPC_NONCE_OFFSET], expected->nonce, 32) != 0 ||
        read_u32_be(&frame->bytes[REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET]) !=
            expected->selector.console_uid ||
        read_u32_be(&frame->bytes[REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET]) !=
            expected->selector.audit_session_id ||
        read_u64_be(&frame->bytes[REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET]) !=
            expected->lock_epoch ||
        memcmp(&frame->bytes[REPOSE_UNLOCK_IPC_INSTANCE_OFFSET],
               expected->service_instance,
               sizeof(expected->service_instance)) != 0 ||
        read_u64_be(&frame->bytes[REPOSE_UNLOCK_IPC_WATCH_ID_OFFSET]) !=
            expected->watch_id) {
        return false;
    }
    uint8_t status = frame->bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET];
    if (status == REPOSE_UNLOCK_IPC_STATUS_EVENT) {
        *out_kind = REPOSE_IPC_WATCH_READY;
        return true;
    }
    if (status == REPOSE_UNLOCK_IPC_STATUS_KEEPALIVE) {
        *out_kind = REPOSE_IPC_WATCH_KEEPALIVE;
        return true;
    }
    return false;
}

static bool verify_root_peer(int socket_fd, void *unused_context) {
    (void)unused_context;
    uid_t effective_uid = (uid_t)-1;
    gid_t effective_gid = (gid_t)-1;
    return socket_fd >= 0 && getpeereid(socket_fd, &effective_uid, &effective_gid) == 0 &&
           effective_uid == 0;
}

typedef bool (*peer_verifier_fn)(int socket_fd, void *context);

static bool exchange_with_endpoint(const char *socket_path,
                                   peer_verifier_fn verify_peer,
                                   void *verify_peer_context,
#if defined(REPOSE_UNLOCK_TESTING)
                                   void (*after_request_written)(void *context),
                                   void *after_request_written_context,
                                   void (*before_request_shutdown)(void *context),
                                   void *before_request_shutdown_context,
                                   void (*after_reply_decoded)(void *context),
                                   void *after_reply_decoded_context,
#endif
                                   repose_session_selector_t selector,
                                   const repose_deadline_t *deadline,
                                   repose_ipc_initial_result_t *out_result) {
    if (out_result != NULL) {
        memset(out_result, 0, sizeof(*out_result));
        out_result->watch_socket = -1;
    }
    if (socket_path == NULL || verify_peer == NULL || deadline == NULL ||
        out_result == NULL || selector.console_uid == 0 || selector.audit_session_id == 0 ||
        repose_deadline_expired(deadline)) {
        return false;
    }
    uint8_t nonce[32];
    if (SecRandomCopyBytes(kSecRandomDefault, sizeof(nonce), nonce) != errSecSuccess ||
        bytes_all_zero(nonce, sizeof(nonce))) {
        return false;
    }
    repose_unlock_ipc_frame_t request;
    if (!repose_ipc_encode_request(&request, nonce, selector)) {
        return false;
    }
    int socket_fd = connect_until(socket_path, deadline);
    if (socket_fd < 0) {
        return false;
    }
    if (!verify_peer(socket_fd, verify_peer_context) ||
        repose_deadline_expired(deadline) ||
        !write_all_until(socket_fd, request.bytes, sizeof(request.bytes), deadline)) {
        close(socket_fd);
        return false;
    }
#if defined(REPOSE_UNLOCK_TESTING)
    if (after_request_written != NULL) {
        after_request_written(after_request_written_context);
    }
#endif
    if (repose_deadline_expired(deadline)) {
        close(socket_fd);
        return false;
    }
#if defined(REPOSE_UNLOCK_TESTING)
    if (before_request_shutdown != NULL) {
        before_request_shutdown(before_request_shutdown_context);
    }
#endif
    if (repose_deadline_expired(deadline) || shutdown(socket_fd, SHUT_WR) != 0 ||
        repose_deadline_expired(deadline)) {
        close(socket_fd);
        return false;
    }

    repose_unlock_ipc_frame_t response;
    if (!read_exact_until(socket_fd,
                          response.bytes,
                          REPOSE_UNLOCK_IPC_HEADER_LEN,
                          deadline) ||
        !reply_header_valid(&response) ||
        !read_exact_until(socket_fd,
                          &response.bytes[REPOSE_UNLOCK_IPC_HEADER_LEN],
                          REPOSE_UNLOCK_IPC_FRAME_LEN - REPOSE_UNLOCK_IPC_HEADER_LEN,
                          deadline)) {
        close(socket_fd);
        return false;
    }
    repose_ipc_initial_result_t result;
    memset(&result, 0, sizeof(result));
    result.watch_socket = -1;
    if (!repose_ipc_decode_reply(&response, nonce, selector, &result.reply)) {
        close(socket_fd);
        return false;
    }
#if defined(REPOSE_UNLOCK_TESTING)
    if (after_reply_decoded != NULL) {
        after_reply_decoded(after_reply_decoded_context);
    }
#endif
    if (repose_deadline_expired(deadline)) {
        close(socket_fd);
        return false;
    }
    if (result.reply.kind != REPOSE_IPC_REPLY_WATCHING &&
        !read_terminal_eof_until(socket_fd, deadline)) {
        close(socket_fd);
        return false;
    }
    if (repose_deadline_expired(deadline)) {
        close(socket_fd);
        return false;
    }
    if (result.reply.kind == REPOSE_IPC_REPLY_WATCHING) {
        result.watch_socket = socket_fd;
    } else {
        close(socket_fd);
    }
    *out_result = result;
    return true;
}

#if !defined(REPOSE_UNLOCK_TESTING)
bool repose_ipc_exchange(repose_session_selector_t selector,
                         const repose_deadline_t *deadline,
                         repose_ipc_initial_result_t *out_result) {
    return exchange_with_endpoint(REPOSE_UNLOCK_PRODUCTION_SOCKET_PATH,
                                  verify_root_peer,
                                  NULL,
                                  selector,
                                  deadline,
                                  out_result);
}
#endif

#if defined(REPOSE_UNLOCK_TESTING)
bool repose_ipc_test_exchange(const repose_ipc_test_endpoint_t *endpoint,
                              repose_session_selector_t selector,
                              const repose_deadline_t *deadline,
                              repose_ipc_initial_result_t *out_result) {
    if (out_result != NULL) {
        memset(out_result, 0, sizeof(*out_result));
        out_result->watch_socket = -1;
    }
    return endpoint != NULL &&
           exchange_with_endpoint(endpoint->socket_path,
                                  endpoint->verify_peer,
                                  endpoint->verify_peer_context,
                                  endpoint->after_request_written,
                                  endpoint->after_request_written_context,
                                  endpoint->before_request_shutdown,
                                  endpoint->before_request_shutdown_context,
                                  endpoint->after_reply_decoded,
                                  endpoint->after_reply_decoded_context,
                                  selector,
                                  deadline,
                                  out_result);
}

bool repose_ipc_test_verify_root_peer(int socket_fd, void *unused_context) {
    return verify_root_peer(socket_fd, unused_context);
}
#endif

void repose_ipc_initial_result_close(repose_ipc_initial_result_t *result) {
    if (result != NULL && result->watch_socket >= 0) {
        close(result->watch_socket);
        result->watch_socket = -1;
    }
}

typedef enum watch_read_status {
    WATCH_READ_FRAME,
    WATCH_READ_CANCELLED,
    WATCH_READ_ENDED,
} watch_read_status_t;

static int poll_watch_descriptors(int watch_socket,
                                  int cancel_socket,
                                  const repose_deadline_t *deadline,
                                  struct pollfd descriptors[2]) {
    descriptors[0] = (struct pollfd){.fd = cancel_socket, .events = POLLIN, .revents = 0};
    descriptors[1] = (struct pollfd){.fd = watch_socket, .events = POLLIN, .revents = 0};
    for (;;) {
        int timeout = deadline == NULL ? -1 : repose_deadline_poll_timeout_ms(deadline);
        if (deadline != NULL && repose_deadline_expired(deadline)) {
            return 0;
        }
        int status = poll(descriptors, 2, timeout);
        if (status >= 0) {
            return status;
        }
        if (errno != EINTR) {
            return -1;
        }
    }
}

static watch_read_status_t read_one_watch_frame(
    int watch_socket,
    int cancel_socket,
    repose_unlock_ipc_frame_t *out_frame) {
    size_t offset = 0;
    repose_deadline_t frame_deadline;
    bool frame_started = false;
    while (offset < sizeof(out_frame->bytes)) {
        struct pollfd descriptors[2];
        const repose_deadline_t *deadline = frame_started ? &frame_deadline : NULL;
        int status = poll_watch_descriptors(
            watch_socket, cancel_socket, deadline, descriptors);
        if (status <= 0) {
            return WATCH_READ_ENDED;
        }
        if ((descriptors[0].revents & (POLLIN | POLLHUP | POLLERR | POLLNVAL)) != 0) {
            return WATCH_READ_CANCELLED;
        }
        if ((descriptors[1].revents & (POLLIN | POLLHUP | POLLERR | POLLNVAL)) == 0) {
            continue;
        }
        size_t boundary = offset < REPOSE_UNLOCK_IPC_HEADER_LEN
                              ? REPOSE_UNLOCK_IPC_HEADER_LEN
                              : sizeof(out_frame->bytes);
        ssize_t count =
            read(watch_socket, &out_frame->bytes[offset], boundary - offset);
        if (count > 0) {
            if (!frame_started) {
                if (!repose_deadline_start(&frame_deadline, 100)) {
                    return WATCH_READ_ENDED;
                }
                frame_started = true;
            }
            offset += (size_t)count;
            if (offset == REPOSE_UNLOCK_IPC_HEADER_LEN && !watch_header_valid(out_frame)) {
                return WATCH_READ_ENDED;
            }
            continue;
        }
        if (count < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK)) {
            continue;
        }
        return WATCH_READ_ENDED;
    }
    return repose_deadline_expired(&frame_deadline) ? WATCH_READ_ENDED : WATCH_READ_FRAME;
}

static repose_ipc_watch_result_t watch_wait_internal(
    int watch_socket,
    int cancel_socket,
    const repose_ipc_correlation_t *correlation
#if defined(REPOSE_UNLOCK_TESTING)
    ,
    void (*after_ready_decoded)(void *context),
    void *after_ready_decoded_context
#endif
) {
    if (watch_socket < 0 || cancel_socket < 0 || correlation == NULL) {
        return REPOSE_IPC_WATCH_RESULT_ENDED;
    }
    for (;;) {
        repose_unlock_ipc_frame_t frame;
        watch_read_status_t status =
            read_one_watch_frame(watch_socket, cancel_socket, &frame);
        if (status == WATCH_READ_CANCELLED) {
            return REPOSE_IPC_WATCH_RESULT_CANCELLED;
        }
        if (status == WATCH_READ_ENDED) {
            return REPOSE_IPC_WATCH_RESULT_ENDED;
        }
        repose_ipc_watch_frame_kind_t kind;
        if (!repose_ipc_decode_watch_frame(&frame, correlation, &kind)) {
            return REPOSE_IPC_WATCH_RESULT_ENDED;
        }
        if (kind == REPOSE_IPC_WATCH_READY) {
#if defined(REPOSE_UNLOCK_TESTING)
            if (after_ready_decoded != NULL) {
                after_ready_decoded(after_ready_decoded_context);
            }
#endif
            return REPOSE_IPC_WATCH_RESULT_READY;
        }
    }
}

repose_ipc_watch_result_t repose_ipc_watch_wait(
    int watch_socket,
    int cancel_socket,
    const repose_ipc_correlation_t *correlation) {
    return watch_wait_internal(
        watch_socket,
        cancel_socket,
        correlation
#if defined(REPOSE_UNLOCK_TESTING)
        ,
        NULL,
        NULL
#endif
    );
}

#if defined(REPOSE_UNLOCK_TESTING)
repose_ipc_watch_result_t repose_ipc_test_watch_wait(
    int watch_socket,
    int cancel_socket,
    const repose_ipc_correlation_t *correlation,
    void (*after_ready_decoded)(void *context),
    void *after_ready_decoded_context) {
    return watch_wait_internal(watch_socket,
                               cancel_socket,
                               correlation,
                               after_ready_decoded,
                               after_ready_decoded_context);
}
#endif
