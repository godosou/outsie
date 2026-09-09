/*
 * repose_permit_client.h -- the mechanism-side drop-in that replaces plugin.c's
 * wait_for_permit(). It asks repose-permitd over the unix socket instead of
 * reading the permit file directly.
 *
 * Deliberately dependency-light: plain BSD sockets only, NO CoreFoundation and
 * NO Security framework. It is linked into the ReposeSpike plugin bundle, which
 * runs inside SecurityAgent / authorizationhost, and must stay as small and as
 * unsurprising as possible there. All identity proving happens on the DAEMON
 * side (it verifies us); the client's only job is to ask and to fail closed.
 */
#ifndef REPOSE_PERMIT_CLIENT_H
#define REPOSE_PERMIT_CLIENT_H

/*
 * Ask the daemon whether this unlock attempt is permitted.
 *   returns 1  -> Allow  (a fresh, not-yet-spent presence assertion existed)
 *   returns 0  -> Deny   (daemon down, socket missing, timeout, malformed or
 *                         unmatched reply, stale/absent/replayed presence)
 *
 * Never blocks indefinitely: bounded by REPOSE_PERMIT_TIMEOUT_MS (default
 * 1200ms, kept under plugin.c's PERMIT_TIMEOUT_MS). Every error path denies.
 */
int repose_request_permit(void);

#endif /* REPOSE_PERMIT_CLIENT_H */
