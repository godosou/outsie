/*
 * repose-permit-probe -- a throwaway command-line client for smoke-testing the
 * daemon's wire protocol, presence check and single-consume WITHOUT the lock
 * screen. It links the same client code plugin.c will use.
 *
 * Note: if the daemon is running with peer verification ON (the default and
 * only safe mode), this probe will be DENIED -- it is neither authorizationhost
 * nor SecurityAgent, which is exactly the point. To exercise the presence /
 * consume logic, run the daemon with --insecure-skip-peer-verify on a scratch
 * socket, e.g.:
 *
 *   sudo ./build/repose-permitd --insecure-skip-peer-verify \
 *        --socket /tmp/rp.sock --presence /tmp/rp.permit --freshness 15 &
 *   sudo touch /tmp/rp.permit
 *   REPOSE_PERMIT_SOCK_PATH=/tmp/rp.sock ./build/repose-permit-probe  # ALLOW
 *   REPOSE_PERMIT_SOCK_PATH=/tmp/rp.sock ./build/repose-permit-probe  # DENY (replay)
 *   sudo touch /tmp/rp.permit
 *   REPOSE_PERMIT_SOCK_PATH=/tmp/rp.sock ./build/repose-permit-probe  # ALLOW again
 */
#include "repose_permit_client.h"
#include <stdio.h>

int main(void)
{
    int allow = repose_request_permit();
    printf("%s\n", allow ? "ALLOW" : "DENY");
    return allow ? 0 : 1;
}
