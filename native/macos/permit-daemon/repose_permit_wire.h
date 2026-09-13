/*
 * repose_permit_wire.h -- the on-the-wire contract shared by the daemon
 * (repose-permitd) and the mechanism-side client (repose_permit_client, linked
 * into the ReposeSpike plugin bundle).
 *
 * Fixed-length frames, identical 40-byte layout in both directions. Nothing is
 * length-prefixed and nothing is variable: the reader always knows exactly how
 * many bytes a valid message is, so a short read is unambiguously a truncated
 * or hostile peer and the only correct response is to fail closed (deny). This
 * mirrors the fixed-length discipline of the prior-art repose-unlock-ipc crate
 * without pulling in its 84-byte protocol; the spike does not carry the phone
 * transcript, so the extra fields would be dead weight here.
 */
#ifndef REPOSE_PERMIT_WIRE_H
#define REPOSE_PERMIT_WIRE_H

#include <stdint.h>

/* Filesystem layout.
 *
 * The SOCKET lives in its own dedicated directory, deliberately NOT the permit
 * directory. permit-bridge.sh's default PERMIT_ON_CMD runs
 *     sudo chmod 755 /var/run/repose-spike
 * every few seconds while the phone is near; if the socket shared that
 * directory, the bridge would keep loosening the daemon's 0750 traversal gate
 * back to 0755. So the socket gets a directory the bridge never touches, held
 * at 0750 root:_securityagent by the daemon -- the primary filesystem gate, so
 * an ordinary uid cannot even traverse to the socket, let alone connect(). */
#define REPOSE_PERMIT_DIR            "/var/run/repose-permitd"
#define REPOSE_PERMIT_SOCK_PATH      "/var/run/repose-permitd/permit.sock"
/* The presence signal. This is the SAME file the existing (unmodified)
 * permit-bridge.sh re-touches while the phone is near -- the daemon reuses it
 * as its "is the phone here right now" input, so the bridge needs no change and
 * the crash => stale => deny property (permit-design gap #4) still holds. The
 * daemon runs as root, so it can traverse /var/run/repose-spike to read this
 * file whatever mode the bridge leaves that directory in. */
#define REPOSE_PERMIT_PRESENCE_PATH  "/var/run/repose-spike/permit"

/* _securityagent's primary gid. The socket and its directory are group-owned by
 * this gid; it is the ONLY group that admits uid 92 without also admitting an
 * ordinary interactive user (see the design notes / group investigation). */
#define REPOSE_PERMIT_SECURITYAGENT_UID 92
#define REPOSE_PERMIT_SECURITYAGENT_GID 92
#define REPOSE_PERMIT_ROOT_UID          0

/* Frame layout (40 bytes, both directions):
 *   [0..4)   magic  = 'R','P','U','1'
 *   [4]      version = 1
 *   [5]      op      = 1 request (client->daemon) / 2 verdict (daemon->client)
 *   [6]      verdict = request: 0 ; verdict: 0 deny / 1 allow
 *   [7]      reserved = 0
 *   [8..40)  nonce[32] -- client-random, echoed verbatim in the verdict so the
 *            client can prove the reply belongs to the request it just sent. */
#define REPOSE_PERMIT_FRAME_LEN   40u
#define REPOSE_PERMIT_NONCE_LEN   32u

#define REPOSE_PERMIT_OFF_MAGIC   0u
#define REPOSE_PERMIT_OFF_VERSION 4u
#define REPOSE_PERMIT_OFF_OP      5u
#define REPOSE_PERMIT_OFF_VERDICT 6u
#define REPOSE_PERMIT_OFF_RSVD    7u
#define REPOSE_PERMIT_OFF_NONCE   8u

#define REPOSE_PERMIT_MAGIC0 'R'
#define REPOSE_PERMIT_MAGIC1 'P'
#define REPOSE_PERMIT_MAGIC2 'U'
#define REPOSE_PERMIT_MAGIC3 '1'
#define REPOSE_PERMIT_VERSION 1u

#define REPOSE_PERMIT_OP_REQUEST 1u
#define REPOSE_PERMIT_OP_VERDICT 2u

#define REPOSE_PERMIT_VERDICT_DENY  0u
#define REPOSE_PERMIT_VERDICT_ALLOW 1u

#endif /* REPOSE_PERMIT_WIRE_H */
