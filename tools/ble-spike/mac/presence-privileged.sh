#!/bin/sh
#
# The privileged half of the presence pipeline: verify beacons, decide near/far,
# write the permit. Runs as root.
#
# WHY THIS IS A FILE IN THE BUNDLE AND NOT GENERATED AT RUN TIME
#
# It used to be written into the app's data directory on every run and then
# executed as root. That directory is owned by the user and mode 755, as is the
# `bin/` beside it that the staged copies of presence-verify and permit-bridge.sh
# went into. So anything running as the user could replace either, and get root
# the next time presence monitoring was switched on -- behind an authorization
# dialog the user would read as "Outsie wants to start watching for my phone"
# and approve. That turns the prompt into a laundering step: consent collected
# for one thing, spent on another.
#
# Now: this script ships inside the bundle, its parameters arrive as environment
# variables from the process that raised the authorization, and it refuses to
# execute anything out of the user's home directory. Nothing executable is
# created at run time.
#
# Every REPOSE_* below is required; there are no defaults, because a default
# here would be a path this script invented while holding root.

set -u

fail() { printf 'presence-privileged: %s\n' "$*" >&2; exit 2; }

for v in REPOSE_MODE REPOSE_BIN REPOSE_KEY_DIR REPOSE_RAW REPOSE_VERIFIED \
         REPOSE_RUNFLAG REPOSE_PERMIT_DIR REPOSE_STATUS_FILE REPOSE_LOG_DIR; do
    eval "test -n \"\${$v:-}\"" || fail "$v is not set"
done

# Refuse to run anything out of $HOME, as root.
#
# The check is on the path this script is about to execute, not on who owns it:
# ownership can be right at the moment of the check and wrong a moment later,
# and a home directory is somewhere the user can always write. In the shipped
# app REPOSE_BIN is inside the bundle; in development it is the repository, and
# the run is refused with a message that says how to proceed deliberately.
case "${REPOSE_BIN}" in
    "${HOME:-/nonexistent}"/*)
        if [ "${REPOSE_ALLOW_HOME_BIN:-}" != "1" ]; then
            fail "refusing to run ${REPOSE_BIN}/presence-verify as root: it is inside a
directory the logged-in user can write to, so approving this prompt would
approve whatever happens to be there. Set REPOSE_ALLOW_HOME_BIN=1 for a
development run if that is genuinely what you want."
        fi
        printf 'presence-privileged: WARNING running root binaries from %s\n' \
            "${REPOSE_BIN}" >&2
        ;;
esac

[ -x "${REPOSE_BIN}/presence-verify" ] || fail "no presence-verify in ${REPOSE_BIN}"
[ -r "${REPOSE_BIN}/permit-bridge.sh" ] || fail "no permit-bridge.sh in ${REPOSE_BIN}"

export REPOSE_PERMIT_ON_CMD="mkdir -p ${REPOSE_PERMIT_DIR} && chmod 755 ${REPOSE_PERMIT_DIR} && touch ${REPOSE_PERMIT_DIR}/permit"
export REPOSE_PERMIT_OFF_CMD="rm -f ${REPOSE_PERMIT_DIR}/permit"
export REPOSE_STATUS_FILE="${REPOSE_STATUS_FILE}"

# The stages are DIRECT children of this script, not wrapped in an inner
# `sh -c`. pkill -P reaches children, not grandchildren, so a wrapper meant the
# kill landed on the wrapper alone and left tail, the verifier and the bridge
# running -- after which the only thing that stopped them was the backstop
# tearing the authorization host down, which is the abrupt path that skips the
# bridge's handler and leaves a live permit behind.
if [ "${REPOSE_MODE}" = remote ]; then
    tail -n +1 -f "${REPOSE_RAW}" \
        | "${REPOSE_BIN}/presence-verify" --key-dir "${REPOSE_KEY_DIR}" \
        >> "${REPOSE_VERIFIED}" 2>> "${REPOSE_LOG_DIR}/verify.log" &
else
    # tee, so the verified stream reaches TWO readers.
    #
    # The bridge is inside this root chain and gets it on a pipe. The Mac's own
    # state beacon cannot be: advertising needs the Bluetooth grant, which
    # belongs to the app, and the same binary under root reports STATE
    # unauthorized -- the exact reason rssi-scan is unprivileged. So the second
    # reader is outside, and the handover is this file: created 644 by the
    # unprivileged half, appended to here, tailed by the advertiser.
    #
    # A file rather than a second FIFO on purpose. An earlier version of this
    # pipeline deadlocked on FIFO open ordering, and a file has no ordering.
    tail -n +1 -f "${REPOSE_RAW}" \
        | "${REPOSE_BIN}/presence-verify" --key-dir "${REPOSE_KEY_DIR}" \
            2>> "${REPOSE_LOG_DIR}/verify.log" \
        | tee -a "${REPOSE_VERIFIED}" \
        | bash "${REPOSE_BIN}/permit-bridge.sh" 2>> "${REPOSE_LOG_DIR}/bridge.log" &
fi

# A file, not a pid: a dead-but-unreaped process still answers `kill -0`, and
# the watcher read that as alive. Presence of a file has no such ambiguity.
while [ -e "${REPOSE_RUNFLAG}" ]; do sleep 1; done

# TERM, not KILL: the bridge has a handler that clears the permit and publishes
# that it stopped. Killing it outright would leave the door open for the
# plugin's whole freshness window.
pkill -P $$ 2>/dev/null
sleep 1
