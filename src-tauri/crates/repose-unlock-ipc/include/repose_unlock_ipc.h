#ifndef REPOSE_UNLOCK_IPC_H
#define REPOSE_UNLOCK_IPC_H

#include <stdint.h>

#define REPOSE_UNLOCK_IPC_FRAME_LEN 84u
#define REPOSE_UNLOCK_IPC_HEADER_LEN 12u
#define REPOSE_UNLOCK_IPC_PAYLOAD_LEN 72u

#define REPOSE_UNLOCK_IPC_MAGIC_OFFSET 0u
#define REPOSE_UNLOCK_IPC_VERSION_OFFSET 4u
#define REPOSE_UNLOCK_IPC_OPERATION_OFFSET 5u
#define REPOSE_UNLOCK_IPC_STATUS_OFFSET 6u
#define REPOSE_UNLOCK_IPC_FLAGS_OFFSET 7u
#define REPOSE_UNLOCK_IPC_PAYLOAD_LEN_OFFSET 8u
#define REPOSE_UNLOCK_IPC_NONCE_OFFSET 12u
#define REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET 44u
#define REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET 48u
#define REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET 52u
#define REPOSE_UNLOCK_IPC_INSTANCE_OFFSET 60u
#define REPOSE_UNLOCK_IPC_WATCH_ID_OFFSET 76u

#define REPOSE_UNLOCK_IPC_MAGIC_0 0x52u
#define REPOSE_UNLOCK_IPC_MAGIC_1 0x50u
#define REPOSE_UNLOCK_IPC_MAGIC_2 0x55u
#define REPOSE_UNLOCK_IPC_MAGIC_3 0x49u
#define REPOSE_UNLOCK_IPC_VERSION 1u
#define REPOSE_UNLOCK_IPC_OP_CONSUME_OR_WATCH 1u
#define REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE 2u
#define REPOSE_UNLOCK_IPC_STATUS_REQUEST 0u
#define REPOSE_UNLOCK_IPC_STATUS_CONSUMED 1u
#define REPOSE_UNLOCK_IPC_STATUS_WATCHING 2u
#define REPOSE_UNLOCK_IPC_STATUS_DENIED 3u
#define REPOSE_UNLOCK_IPC_STATUS_EVENT 4u
#define REPOSE_UNLOCK_IPC_STATUS_KEEPALIVE 5u

/*
 * After writing exactly one request frame, the client MUST call
 * shutdown(socket_fd, SHUT_WR). The read half remains open for a reply/events.
 * The server confirms EOF within the request's absolute deadline before it
 * consults or consumes a permit. After WATCHING, the client MUST validate and
 * ignore fully correlated STATUS_KEEPALIVE frames until STATUS_EVENT arrives.
 * A keepalive is never authorization and never consumes a permit.
 */
#define REPOSE_UNLOCK_IPC_REQUEST_REQUIRES_WRITE_HALF_CLOSE 1u

typedef struct repose_unlock_ipc_frame {
    uint8_t bytes[REPOSE_UNLOCK_IPC_FRAME_LEN];
} repose_unlock_ipc_frame_t;

_Static_assert(sizeof(repose_unlock_ipc_frame_t) == REPOSE_UNLOCK_IPC_FRAME_LEN,
               "Repose unlock IPC frame ABI drift");

#endif
