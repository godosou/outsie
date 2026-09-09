/*
 * repose_peer_verify.h -- prove that the process on the other end of the unix
 * socket really is one of the two Apple authorization hosts, not just any local
 * process that happens to have the right uid.
 *
 * Two identities are accepted, each pinned to a code-signing designated
 * requirement AND cross-checked against the peer's audit-token euid:
 *
 *   euid 0  (privileged mechanism variant, runs in authorizationhost):
 *       identifier "com.apple.authorizationhost" and anchor apple
 *   euid 92 (non-privileged variant, runs in SecurityAgent as _securityagent):
 *       identifier "com.apple.SecurityAgent" and anchor apple
 *
 * The uid alone is necessary but never sufficient: the SecCodeCheckValidity
 * against the designated requirement is the real identity gate. A local root
 * process is uid 0 but is not com.apple.authorizationhost, so it fails.
 */
#ifndef REPOSE_PEER_VERIFY_H
#define REPOSE_PEER_VERIFY_H

#include <sys/types.h>

typedef struct repose_peer_verifier repose_peer_verifier_t;

/* Compile the two pinned requirements. Returns NULL on failure; a NULL
 * verifier MUST be treated as "deny everything" by the caller (fail closed). */
repose_peer_verifier_t *repose_peer_verifier_create(void);

void repose_peer_verifier_destroy(repose_peer_verifier_t *verifier);

/* Verify the peer connected on `fd`. Returns 0 and fills *out_euid / *out_pid
 * (either may be NULL) when the peer is a permitted, validly-signed host.
 * Returns a negative value on ANY rejection or error -- the caller denies. */
int repose_peer_verifier_check(const repose_peer_verifier_t *verifier, int fd,
                               uid_t *out_euid, pid_t *out_pid);

#endif /* REPOSE_PEER_VERIFY_H */
