#ifndef REPOSE_AUTHORIZATION_TEST_SERVER_H
#define REPOSE_AUTHORIZATION_TEST_SERVER_H

#include "repose_unlock_ipc.h"

#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef void (*repose_test_connection_handler_fn)(int socket_fd,
                                                  size_t connection_index,
                                                  void *context);

typedef struct repose_test_server {
    int listener;
    char directory[1024];
    char socket_path[104];
    pthread_t thread;
    size_t connection_count;
    repose_test_connection_handler_fn handler;
    void *context;
} repose_test_server_t;

void repose_test_server_start(repose_test_server_t *server,
                              size_t connection_count,
                              repose_test_connection_handler_fn handler,
                              void *context);
void repose_test_server_join(repose_test_server_t *server);
bool repose_test_read_request(int socket_fd,
                              repose_unlock_ipc_frame_t *out_frame,
                              size_t *out_byte_count,
                              bool *out_saw_eof);
void repose_test_make_reply(repose_unlock_ipc_frame_t *frame,
                            uint8_t status,
                            uint64_t lock_epoch,
                            uint8_t instance_byte,
                            uint64_t watch_id);
bool repose_test_write_frame(int socket_fd,
                             const repose_unlock_ipc_frame_t *frame,
                             size_t chunk_size);

#endif
