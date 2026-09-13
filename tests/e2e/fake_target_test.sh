#!/bin/bash
# Prove the acceptance test can both pass and fail, in the host-drives-target
# mode it will run in against the VM.
#
# A test that only ever passes is worthless, and one that only ever fails is
# indistinguishable from a broken feature. Before trusting unlock_acceptance.sh
# to answer "did the Mac unlock", it has to be shown reporting PASS when the
# target unlocks, FAIL when it never does, and FAIL when it unlocks while the
# phone is supposed to be away.
#
# The target here is a file standing in for the VM's lock state. No screen is
# locked, no VM is needed, and every injection point is exercised exactly as it
# will be with a real one.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ACCEPT="${HERE}/unlock_acceptance.sh"
WORK="$(mktemp -d -t fake-target)"
trap 'rm -rf "$WORK"' EXIT

STATE="${WORK}/locked"
PASS=0
FAIL=0

ok() { printf '  ok   %s\n' "$1"; PASS=$((PASS + 1)); }
no() { printf '  FAIL %s -- %s\n' "$1" "${2:-}"; FAIL=$((FAIL + 1)); }

# The fake target: a file holding "true" or "false".
reset() { echo false > "$STATE"; }

# Shared settings. Short waits keep the suite quick; the logic is unchanged.
run_accept() {
  env \
    REPOSE_TARGET="fake target" \
    REPOSE_E2E_ALLOW_LOCK=1 \
    REPOSE_LOCK_WAIT_S=5 \
    REPOSE_STAY_LOCKED_S=1 \
    REPOSE_UNLOCK_DEADLINE_MS=1500 \
    REPOSE_LOCKSTATE_CMD="cat ${STATE}" \
    REPOSE_LOCK_CMD="echo true > ${STATE}" \
    REPOSE_LEAVE_CMD="$1" \
    REPOSE_RETURN_CMD="$2" \
    "$ACCEPT" 2>&1
}

echo "acceptance test behaviour against a fake target"

# 1. A working plugin: the permit arriving unlocks the target. This is the shape
#    a real PASS will have.
reset
out="$(run_accept "true" "echo false > ${STATE}")"
rc=$?
if [ "$rc" -eq 0 ] && grep -q "^PASS" <<< "$out"; then
  ok "reports PASS when the target unlocks on the phone's return"
else
  no "reports PASS when the target unlocks on the phone's return" "rc=$rc"
  sed 's/^/       /' <<< "$out" | tail -5
fi

# The reported latency must be a real measurement, not a constant.
if grep -qE "end-to-end unlock latency: [0-9]+ ms" <<< "$out"; then
  ok "prints a measured end-to-end latency"
else
  no "prints a measured end-to-end latency" "no latency line"
fi

# 2. A plugin that never unlocks. This is the expected result until one is
#    installed, and it must be clearly distinguishable from a pass.
reset
out="$(run_accept "true" "true")"
rc=$?
if [ "$rc" -ne 0 ] && grep -q "did NOT unlock" <<< "$out"; then
  ok "reports FAIL when the target never unlocks"
else
  no "reports FAIL when the target never unlocks" "rc=$rc"
fi

# 3. A plugin that unlocks regardless of presence. Without this assertion an
#    always-allow mechanism would look like a working feature.
reset
out="$(run_accept "echo false > ${STATE}" "echo false > ${STATE}")"
rc=$?
if [ "$rc" -ne 0 ] && grep -q "after the phone LEFT" <<< "$out"; then
  ok "reports FAIL when the target unlocks while the phone is away"
else
  no "reports FAIL when the target unlocks while the phone is away" "rc=$rc"
fi

# 4. A target that never locks at all -- a wrong lock command, or a guest with
#    no password requirement. Must not be mistaken for either outcome.
reset
out="$(env \
  REPOSE_TARGET="fake target" REPOSE_E2E_ALLOW_LOCK=1 \
  REPOSE_LOCK_WAIT_S=2 REPOSE_UNLOCK_DEADLINE_MS=500 \
  REPOSE_LOCKSTATE_CMD="cat ${STATE}" \
  REPOSE_LOCK_CMD="true" \
  REPOSE_LEAVE_CMD="true" REPOSE_RETURN_CMD="true" \
  "$ACCEPT" 2>&1)"
rc=$?
if [ "$rc" -ne 0 ] && grep -q "did not lock" <<< "$out"; then
  ok "reports FAIL when the target never locks in the first place"
else
  no "reports FAIL when the target never locks in the first place" "rc=$rc"
fi

# 5. An unreachable target must not read as unlocked. Treating a dead ssh
#    connection as "unlocked" would produce a PASS with nothing unlocked.
reset
out="$(env \
  REPOSE_TARGET="fake target" REPOSE_E2E_ALLOW_LOCK=1 \
  REPOSE_LOCKSTATE_CMD="exit 255" \
  REPOSE_LOCK_CMD="true" \
  REPOSE_LEAVE_CMD="true" REPOSE_RETURN_CMD="true" \
  "$ACCEPT" 2>&1)"
rc=$?
if [ "$rc" -ne 0 ]; then
  ok "refuses to run when the target is unreachable"
else
  no "refuses to run when the target is unreachable" "it reported success"
fi

# 6. The oracle dying part way through, which is what a dropped ssh connection
#    looks like. The final step waits for "not locked", so an unreadable poll
#    treated as "not locked" would print PASS with nothing unlocked. This is the
#    single most dangerous failure mode of the whole harness.
reset
COUNTER="${WORK}/calls"
echo 0 > "$COUNTER"
FLAKY="${WORK}/flaky.sh"
cat > "$FLAKY" <<EOF
#!/bin/bash
n=\$(cat "${COUNTER}")
echo \$((n + 1)) > "${COUNTER}"
# Answer honestly for the first few polls so preflight and the lock step pass,
# then start failing the way a severed connection does.
if [ "\$n" -lt 4 ]; then cat "${STATE}"; exit 0; fi
exit 255
EOF
chmod +x "$FLAKY"

out="$(env \
  REPOSE_TARGET="fake target" REPOSE_E2E_ALLOW_LOCK=1 \
  REPOSE_LOCK_WAIT_S=5 REPOSE_STAY_LOCKED_S=1 REPOSE_UNLOCK_DEADLINE_MS=1500 \
  REPOSE_LOCKSTATE_CMD="$FLAKY" \
  REPOSE_LOCK_CMD="echo true > ${STATE}" \
  REPOSE_LEAVE_CMD="true" REPOSE_RETURN_CMD="true" \
  "$ACCEPT" 2>&1)"
rc=$?
if [ "$rc" -ne 0 ] && grep -q "stopped being readable" <<< "$out"; then
  ok "an oracle that dies mid-run voids the run instead of reporting unlocked"
else
  no "an oracle that dies mid-run voids the run instead of reporting unlocked" "rc=$rc"
  sed 's/^/       /' <<< "$out" | tail -4
fi

echo
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
