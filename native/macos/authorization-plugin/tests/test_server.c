#include "test_server.h"

#include <assert.h>
#include <errno.h>
#include <signal.h>
#include <poll.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

static void write_u64_be(uint8_t *bytes, uint64_t value) {
    for (size_t index = 0; index < 8; index += 1) {
        bytes[index] = (uint8_t)(value >> (56 - (index * 8)));
    }
}

static void *serve(void *opaque) {
    repose_test_server_t *server = opaque;
    for (size_t index = 0; index < server->connection_count; index += 1) {
        struct pollfd listener_poll = {
            .fd = server->listener,
            .events = POLLIN,
            .revents = 0,
        };
        assert(poll(&listener_poll, 1, 2000) == 1);
        assert((listener_poll.revents & POLLIN) != 0);
        int accepted = accept(server->listener, NULL, NULL);
        assert(accepted >= 0);
        server->handler(accepted, index, server->context);
        close(accepted);
    }
    return NULL;
}

static ssize_t read_bounded(int socket_fd, void *buffer, size_t length) {
    struct pollfd descriptor = {.fd = socket_fd, .events = POLLIN, .revents = 0};
    for (;;) {
        int status = poll(&descriptor, 1, 1000);
        if (status == 0) {
            errno = ETIMEDOUT;
            return -1;
        }
        if (status < 0 && errno == EINTR) {
            continue;
        }
        if (status < 0) {
            return -1;
        }
        return read(socket_fd, buffer, length);
    }
}

void repose_test_server_start(repose_test_server_t *server,
                              size_t connection_count,
                              repose_test_connection_handler_fn handler,
                              void *context) {
    assert(server != NULL);
    assert(connection_count > 0);
    assert(handler != NULL);
    assert(signal(SIGPIPE, SIG_IGN) != SIG_ERR);
    memset(server, 0, sizeof(*server));
    char template_path[] = "/tmp/repose-auth-plugin.XXXXXX";
    char *directory = mkdtemp(template_path);
    assert(directory != NULL);
    assert(strlen(directory) < sizeof(server->directory));
    memcpy(server->directory, directory, strlen(directory) + 1);
    int length = snprintf(server->socket_path,
                          sizeof(server->socket_path),
                          "%s/consume.sock",
                          server->directory);
    assert(length > 0 && (size_t)length < sizeof(server->socket_path));

    server->listener = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(server->listener >= 0);
    struct sockaddr_un address;
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    memcpy(address.sun_path, server->socket_path, strlen(server->socket_path) + 1);
    address.sun_len = SUN_LEN(&address);
    int bind_status = bind(server->listener,
                           (const struct sockaddr *)&address,
                           (socklen_t)SUN_LEN(&address));
    if (bind_status != 0) {
        perror("bind temporary authorization test socket");
        abort();
    }
    assert(listen(server->listener, (int)connection_count) == 0);
    server->connection_count = connection_count;
    server->handler = handler;
    server->context = context;
    assert(pthread_create(&server->thread, NULL, serve, server) == 0);
}

void repose_test_server_join(repose_test_server_t *server) {
    assert(pthread_join(server->thread, NULL) == 0);
    assert(close(server->listener) == 0);
    assert(unlink(server->socket_path) == 0);
    assert(rmdir(server->directory) == 0);
    server->listener = -1;
}

bool repose_test_read_request(int socket_fd,
                              repose_unlock_ipc_frame_t *out_frame,
                              size_t *out_byte_count,
                              bool *out_saw_eof) {
    assert(out_frame != NULL);
    assert(out_byte_count != NULL);
    assert(out_saw_eof != NULL);
    size_t offset = 0;
    while (offset < sizeof(out_frame->bytes)) {
        ssize_t count = read_bounded(socket_fd,
                                     &out_frame->bytes[offset],
                                     sizeof(out_frame->bytes) - offset);
        if (count > 0) {
            offset += (size_t)count;
            continue;
        }
        if (count == 0) {
            *out_byte_count = offset;
            *out_saw_eof = true;
            return offset == sizeof(out_frame->bytes);
        }
        if (errno != EINTR) {
            *out_byte_count = offset;
            *out_saw_eof = false;
            return false;
        }
    }
    uint8_t extra;
    ssize_t count;
    do {
        count = read_bounded(socket_fd, &extra, sizeof(extra));
    } while (count < 0 && errno == EINTR);
    *out_byte_count = offset + (count > 0 ? (size_t)count : 0);
    *out_saw_eof = count == 0;
    return offset == sizeof(out_frame->bytes) && count == 0;
}

void repose_test_make_reply(repose_unlock_ipc_frame_t *frame,
                            uint8_t status,
                            uint64_t lock_epoch,
                            uint8_t instance_byte,
                            uint64_t watch_id) {
    frame->bytes[REPOSE_UNLOCK_IPC_STATUS_OFFSET] = status;
    write_u64_be(&frame->bytes[REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET], lock_epoch);
    memset(&frame->bytes[REPOSE_UNLOCK_IPC_INSTANCE_OFFSET], instance_byte, 16);
    write_u64_be(&frame->bytes[REPOSE_UNLOCK_IPC_WATCH_ID_OFFSET], watch_id);
}

bool repose_test_write_frame(int socket_fd,
                             const repose_unlock_ipc_frame_t *frame,
                             size_t chunk_size) {
    assert(frame != NULL);
    assert(chunk_size > 0);
    size_t offset = 0;
    while (offset < sizeof(frame->bytes)) {
        size_t remaining = sizeof(frame->bytes) - offset;
        size_t amount = remaining < chunk_size ? remaining : chunk_size;
        ssize_t count = write(socket_fd, &frame->bytes[offset], amount);
        if (count > 0) {
            offset += (size_t)count;
            continue;
        }
        if (count < 0 && errno == EINTR) {
            continue;
        }
        return false;
    }
    return true;
}
