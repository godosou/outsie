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
# Prerequisites: a VM created from Apple's IPSW and set up by hand once --
# see FIRST-BOOT.md. ghcr.io's prebuilt images were abandoned because the pull
# ran at under 1 MB/s from here; Apple's CDN is faster and yields a guest on the
# exact build this host runs, so results transfer without a version caveat.

set -uo pipefail

VM="${REPOSE_VM:-repose-spike}"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PLUGIN_DIR="${REPO}/native/macos/minimal-auth-plugin"
VM_USER="${REPOSE_VM_USER:-admin}"
REMOTE_DIR="/Users/${VM_USER}/spike"

die() { echo "ERROR: $*" >&2; exit 1; }

command -v tart >/dev/null || die "tart is not installed"
tart list 2>/dev/null | grep -q "[[:space:]]${VM}[[:space:]]" \
  || die "VM '${VM}' not found. Create it with:
    tart create --from-ipsw latest ${VM}
  then follow tools/vm-spike/FIRST-BOOT.md for the one-time manual setup."

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
  || die "ssh failed. Check Remote Login is on in the guest and that the
  account matches REPOSE_VM_USER (default 'admin'); see FIRST-BOOT.md."
scp -q -r "${SSH_OPTS[@]}" "$PLUGIN_DIR" "${VM_USER}@${IP}:${REMOTE_DIR}/" || die "scp of plugin failed"
scp -q -r "${SSH_OPTS[@]}" "${REPO}/tests" "${VM_USER}@${IP}:${REMOTE_DIR}/" || die "scp of tests failed"

echo "-> verifying the bundle survived the copy with its signature intact"
ssh "${SSH_OPTS[@]}" "${VM_USER}@${IP}" \
  "codesign -dvvv ${REMOTE_DIR}/minimal-auth-plugin/build/ReposeSpike.bundle 2>&1 | grep -E 'Signature|CDHash'" \
  || die "the copied bundle does not verify inside the VM"

# The contract test binary is copied rather than rebuilt: a vanilla macOS image
# has no developer tools. Running it in the guest before installing anything
# proves the plugin still behaves correctly on that machine, so a silent
# milestone A cannot be blamed on a bundle damaged in transit.
if [ -x "${PLUGIN_DIR}/build/plugin_contract_test" ]; then
  echo "-> running the plugin contract test inside the VM (takes ~10s)"
  if ssh "${SSH_OPTS[@]}" "${VM_USER}@${IP}" \
      "cd ${REMOTE_DIR}/minimal-auth-plugin && ./build/plugin_contract_test \
       build/ReposeSpike.bundle/Contents/MacOS/ReposeSpike" ; then
    echo "   the plugin behaves correctly in the guest"
  else
    die "the contract test fails inside the VM. Fix that before installing:
      a milestone A that logs nothing would be our bug, not macOS's answer."
  fi
else
  echo "-> WARNING: no contract test binary; run 'make test' on the host first"
fi

cat <<EOF

=== staged. Run these INSIDE the VM window (not over ssh) ===

The screensaver needs a real GUI session, so drive milestone A from the VM's
own Terminal:

  The contract test above already passed in this guest, so the plugin's own
  logic is not in question. Anything that goes wrong from here is either the
  install or macOS itself.

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

  5. Whatever happens, roll back to the clean snapshot:
       tart stop ${VM} && tart delete ${VM}
       tart clone repose-spike-clean ${VM}

If step 3 produces no log line, work down the ladder in
docs/product-tech-research/2026-09-08-securityagent-plugin-loading.md rather
than guessing: ad-hoc + SIP on, then library validation off, then SIP off.
Record which rung worked -- that is the real signing requirement.
EOF
