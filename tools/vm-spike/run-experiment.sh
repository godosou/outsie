#!/bin/bash
# Stage the Step 2 experiment into the throwaway Tart VM.
#
# The plugin bundle is built on the HOST and copied in as a finished artifact.
# The vanilla macOS image has no developer tools, and copying the exact signed
# bundle we intend to ship keeps the experiment about loading, not compiling.
#
# This script only stages and prints. It never installs anything: the install
# runs inside the VM, where a snapshot rollback is the recovery path.
#
# Prerequisites:
#   tart clone ghcr.io/cirruslabs/macos-sonoma-vanilla:latest repose-spike
#   (cirruslabs images log in as admin / admin)

set -uo pipefail

VM="${REPOSE_VM:-repose-spike}"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PLUGIN_DIR="${REPO}/native/macos/minimal-auth-plugin"
VM_USER="${REPOSE_VM_USER:-admin}"
REMOTE_DIR="/Users/${VM_USER}/spike"

die() { echo "ERROR: $*" >&2; exit 1; }

command -v tart >/dev/null || die "tart is not installed"
tart list 2>/dev/null | grep -q "[[:space:]]${VM}[[:space:]]" \
  || die "VM '${VM}' not found. Run: tart clone ghcr.io/cirruslabs/macos-sonoma-vanilla:latest ${VM}"

# The bundle must exist and must be ad-hoc signed before it is worth copying.
[ -d "${PLUGIN_DIR}/build/ReposeSpike.bundle" ] \
  || die "plugin not built. Run: make -C ${PLUGIN_DIR}"
codesign -dvvv "${PLUGIN_DIR}/build/ReposeSpike.bundle" 2>&1 | grep -q "Signature=adhoc" \
  || die "built bundle is not ad-hoc signed; the experiment would test the wrong thing"

echo "=== VM ==="
if ! tart list | grep "[[:space:]]${VM}[[:space:]]" | grep -qi running; then
  echo "'${VM}' is not running. Start it in another terminal and leave it up:"
  echo "    tart run ${VM}"
  echo
  echo "A GUI window is required: the screensaver cannot engage without one."
  exit 1
fi

echo "-> waiting for the VM to report an IP"
IP=""
for _ in $(seq 1 60); do
  IP="$(tart ip "$VM" 2>/dev/null)" && [ -n "$IP" ] && break
  sleep 2
done
[ -n "$IP" ] || die "VM did not report an IP within 120s"
echo "   ${IP}"

SSH_OPTS=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)

echo "-> copying the experiment to ${VM_USER}@${IP}:${REMOTE_DIR}"
ssh "${SSH_OPTS[@]}" "${VM_USER}@${IP}" "rm -rf ${REMOTE_DIR} && mkdir -p ${REMOTE_DIR}" \
  || die "ssh failed. Password for cirruslabs images is 'admin'."
scp -q -r "${SSH_OPTS[@]}" "$PLUGIN_DIR" "${VM_USER}@${IP}:${REMOTE_DIR}/" || die "scp of plugin failed"
scp -q -r "${SSH_OPTS[@]}" "${REPO}/tests" "${VM_USER}@${IP}:${REMOTE_DIR}/" || die "scp of tests failed"

echo "-> verifying the bundle survived the copy with its signature intact"
ssh "${SSH_OPTS[@]}" "${VM_USER}@${IP}" \
  "codesign -dvvv ${REMOTE_DIR}/minimal-auth-plugin/build/ReposeSpike.bundle 2>&1 | grep -E 'Signature|CDHash'" \
  || die "the copied bundle does not verify inside the VM"

cat <<EOF

=== staged. Run these INSIDE the VM window (not over ssh) ===

The screensaver needs a real GUI session, so drive milestone A from the VM's
own Terminal:

  1. System Settings > Lock Screen > "Require password after screen saver
     begins" -> Immediately.   Without this there is nothing to unlock.

  2. Confirm the oracle works in the VM before trusting any result:
       cd ${REMOTE_DIR}/tests/e2e && ./lockstate.sh

  3. Milestone A -- does macOS load the plugin at all:
       cd ${REMOTE_DIR}/minimal-auth-plugin && sudo ./install.sh log
     Then lock the screen. Afterwards:
       cat /tmp/repose-plugin.log
     A line here means authorizationhost loaded and invoked an ad-hoc signed
     third-party plugin. That is the single most important unknown in this
     project. NOTE: 'log' mode allows unconditionally, so with k-of-n=1 the
     screen will unlock with NO password. That is the point of the test, and
     the reason this only ever runs in a disposable VM.

  4. Milestone B -- can a mechanism actually short-circuit the password:
       sudo ./uninstall.sh && sudo ./install.sh permit
       cd ${REMOTE_DIR}/tests/e2e
       REPOSE_LEAVE_CMD='rm -f /tmp/repose-permit' \\
       REPOSE_RETURN_CMD='touch /tmp/repose-permit' \\
       REPOSE_E2E_ALLOW_LOCK=1 ./unlock_acceptance.sh

  5. Whatever happens, roll back:
       tart stop ${VM} && tart delete ${VM}   (then re-clone)

If step 3 produces no log line, work down the ladder in
docs/product-tech-research/2026-09-08-securityagent-plugin-loading.md rather
than guessing: ad-hoc + SIP on, then library validation off, then SIP off.
Record which rung worked -- that is the real signing requirement.
EOF
