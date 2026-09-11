#!/bin/bash
#
# Run the whole presence chain: measure -> authenticate -> decide.
#
#   rssi-scan  |  presence-verify (root)  |  permit-bridge.sh
#   your uid      root                       your uid
#   has the radio has the key                has neither
#
# WHY THIS SCRIPT EXISTS RATHER THAN ONE PIPE
# -------------------------------------------
# The three stages cannot share a privilege level. The scanner must stay
# unprivileged: the Bluetooth TCC grant belongs to the terminal app, and the same
# binary run under root reports `STATE unauthorized` and sees nothing. The
# verifier must be root: the presence key is root-owned 0600, and it has to be,
# because anyone who can read K can mint beacons and unlock this Mac. So the
# obvious `a | b | c` cannot be written -- one shell cannot be two users.
#
# Two plain files carry the stages instead, each tailed by the next.
#
# FIFOs were the first attempt and they are a trap here. Opening one blocks until
# the other end opens, so the three stages have to be started in an order nobody
# can see from the code, and the authorization dialog sits in the middle of that
# order. Worse, `do shell script` reclaims the process group when it returns, so a
# verifier detached with nohup sometimes survived and sometimes vanished. Plain
# files have no ordering to get wrong.
#
# TWO SHAPES, BECAUSE THE TARGET DIFFERS
#
#   remote (the spike VM)  scanner | [root: verify] | bridge
#   local  (the product)   scanner | [root: verify | bridge]
#
# The only reason the bridge is kept unprivileged in the remote shape is that it
# reaches the VM over ssh with the invoking user's keys; run it as root and a
# working permit write becomes a silent authentication failure. Locally there is
# no ssh and the permit is a root-owned file on this machine, so the bridge
# belongs inside the privileged half -- which is also simpler, since it means one
# authorization prompt covers the whole chain.
#
# REPOSE_SSH being set is what selects remote. Nothing guesses.
#
# In the product the verifier becomes a launchd job holding the key and none of
# this is user-visible. This is the same shape, assembled by hand.
#
#   tools/ble-spike/mac/presence-pipeline.sh [seconds]
#
# Presence is written to the target the permit commands name -- by default the
# spike VM over ssh, so source tools/vm-spike/vm-env.sh first.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Two layouts, because this file runs from two places.
#
#   repo    tools/ble-spike/mac/  ->  ../../lib/run-root.sh
#   bundle  Resources/scripts/    ->  ../lib/run-root.sh
#
# The repo path was the only one, so in the packaged app the source failed, the
# shell carried on -- `.` on a missing file is not fatal under `set -u` -- and
# the first call to run_root died with "command not found" a hundred lines later.
# A missing helper should not be discovered by the function it defines going
# absent; it is checked here, once, and said plainly.
RUN_ROOT=""
for candidate in "${HERE}/../../lib/run-root.sh" "${HERE}/../lib/run-root.sh"; do
  [ -f "${candidate}" ] && { RUN_ROOT="${candidate}"; break; }
done
[ -n "${RUN_ROOT}" ] || { echo "pipeline: cannot find run-root.sh next to ${HERE}" >&2; exit 2; }
. "${RUN_ROOT}"

DURATION="${1:-0}"          # 0 = until interrupted
KEY_DIR="${REPOSE_KEY_DIR:-/var/db/repose-unlock}"
KEY_ID="${REPOSE_KEY_ID:-1}"
PERMIT_DIR="${REPOSE_PERMIT_DIR:-/var/run/repose-spike}"
STATUS_FILE_ARG="${REPOSE_STATUS_FILE:-}"
# Remote when a VM connection was handed to us; local otherwise. Explicit, so
# nobody has to infer which half the bridge is running in.
MODE="local"
[ -n "${REPOSE_SSH:-}" ] && MODE="remote"

say() { printf 'pipeline: %s\n' "$*" >&2; }

[ -e "${KEY_DIR}/presence-key.${KEY_ID}" ] \
  || { say "no presence key at ${KEY_DIR}/presence-key.${KEY_ID} -- run provision-dev-key.sh"; exit 2; }

for t in rssi-scan presence-verify; do
  src="${HERE}/${t}.swift"
  if [ ! -x "${HERE}/${t}" ] || [ "${src}" -nt "${HERE}/${t}" ]; then
    if [ "$t" = rssi-scan ]; then
      swiftc -O -o "${HERE}/${t}" "${src}" -framework CoreBluetooth || exit 2
    else
      swiftc -O -o "${HERE}/${t}" "${src}" || exit 2
    fi
    say "built ${t}"
  fi
done

WORK="${REPOSE_PIPELINE_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/repose-pipeline.XXXXXX")}"
mkdir -p "${WORK}"

# The privileged half runs from copies here, not from wherever this checkout
# lives.
#
# TCC protects ~/Documents, ~/Desktop and ~/Downloads from processes that have
# not been granted access -- and being root does not exempt you. A root shell
# started from an authorization dialog gets "Operation not permitted" reading a
# script out of ~/Documents, which is a confusing error to receive as root and a
# very confusing one to receive about a file that is plainly there.
#
# Copying is not a workaround for the product; it is what the product already
# does. The shipped app runs these out of its own bundle, which is not in a
# protected folder. This just makes the development path behave the same way.
# Staging happens ONLY when it has to.
#
# The comment above says the shipped app runs these out of its own bundle. It
# did not: this staged unconditionally, so the packaged app was also executing
# root binaries out of a directory the logged-in user can write to. Anyone
# running as the user could replace them and get root the next time presence
# was switched on, behind a dialog the user would approve as "Outsie".
#
# So: run from where they are, unless where they are is somewhere root cannot
# read them.
case "${HERE}" in
  "${HOME}"/*)
    BIN="${WORK}/bin"
    mkdir -p "${BIN}"
    for f in presence-verify permit-bridge.sh; do
      cp "${HERE}/${f}" "${BIN}/${f}" 2>/dev/null || { say "could not stage ${f}"; exit 2; }
    done
    chmod +x "${BIN}"/*
    say "development run: staging root binaries out of ${HERE} (under \$HOME)"
    export REPOSE_ALLOW_HOME_BIN=1
    ;;
  *)
    BIN="${HERE}"
    ;;
esac
# A file, not a pid.
#
# The watcher first polled the scanner's pid with `kill -0`. That reports success
# for a process that has died but not yet been reaped -- and the scanner's parent
# is this script, sitting in its own exit handler, so for those seconds the
# scanner is exactly that: a zombie the watcher reads as alive. The chain then
# only came down when the backstop killed osascript, which is the abrupt path
# that skips the bridge's handler. Presence of a file has no such ambiguity.
# One flag per RUN, not one flag per install.
#
# It used to be the fixed path ${WORK}/running, shared by every pipeline that
# ever ran here. On a restart that is a live hazard: the outgoing pipeline's
# cleanup does `rm -f "${RUNFLAG}"` and then sleeps four seconds, while the
# caller waited two before starting the replacement -- so the corpse deleted
# the newcomer's flag, and the fresh bridge came up, found no flag, and exited
# with "run flag gone" seconds after a successful pairing. Observed 2026-09-12
# after re-pairing: rssi-scan alive, nothing verifying, nothing permitting.
#
# A name the caller chooses per run makes that impossible: an exiting pipeline
# can only ever remove the flag it created.
RUNFLAG="${REPOSE_RUNFLAG:-${WORK}/running}"
RAW="${WORK}/raw.csv"
VERIFIED="${WORK}/verified.csv"
# Created by us, appended to by root. Readable so the bridge (still us) can tail
# what the verifier writes.
: > "${RAW}"; : > "${VERIFIED}"; chmod 644 "${RAW}" "${VERIFIED}"
: > "${RUNFLAG}"; chmod 644 "${RUNFLAG}"

SCAN_PID=""; ROOT_PID=""; TAIL_PID=""; STATE_PID=""
cleanup() {
  # ORDER MATTERS AGAIN, for the same reason as the FIFOs.
  #
  # Kill the scanner and nothing else, first. The privileged half is watching its
  # pid; when it goes, that half signals its own children, and the bridge's
  # handler clears the permit and publishes `stopped` before exiting. No second
  # authorization is needed for any of it.
  #
  # Tearing down osascript first looks equivalent and is not: it takes the whole
  # root chain down with it, so the bridge never runs its handler, and a stopped
  # pipeline leaves behind a live permit plus a status file whose last word is
  # `near`. Both decay safely -- the plugin ages the permit out, the timestamp
  # ages the status out -- but "safe in fifteen seconds" is not the same as
  # "closed now", and the door should not be the thing that waits.
  # Drop the flag first: that is what the privileged half is watching.
  rm -f "${RUNFLAG}" 2>/dev/null
  [ -n "${SCAN_PID}" ] && kill "${SCAN_PID}" 2>/dev/null
  # The watcher polls every second; give it room to notice and unwind.
  sleep 4
  # Backstop only: by now the chain should already be gone.
  for p in "${STATE_PID}" "${TAIL_PID}" "${ROOT_PID}"; do
    [ -n "${p}" ] && kill "${p}" 2>/dev/null
  done
  [ -z "${REPOSE_PIPELINE_DIR:-}" ] && rm -rf "${WORK}"
}
trap cleanup EXIT INT TERM

# 1. Scanner: unprivileged, because the Bluetooth grant belongs to the terminal
#    app and the same binary under root reports STATE unauthorized.
if [ "${DURATION}" -gt 0 ]; then
  "${HERE}/rssi-scan" --duration "${DURATION}" >> "${RAW}" 2> "${WORK}/scan.log" &
else
  "${HERE}/rssi-scan" >> "${RAW}" 2> "${WORK}/scan.log" &
fi
SCAN_PID=$!

# 1b. The Mac's own beacon: unprivileged, for the same reason the scanner is.
#
# It holds no key. presence-verify mints a tag for each state once per window
# and prints it into ${VERIFIED}; this reads them and broadcasts whichever
# matches the current lock state. With no input it stays silent rather than
# advertising a state it cannot authenticate.
#
# Optional: an older bundle without the binary still runs presence, it just
# cannot tell the phone anything back.
if [ -x "${HERE}/state-advertise" ]; then
  tail -n +1 -f "${VERIFIED}" | "${HERE}/state-advertise" \
    2>> "${WORK}/state.log" &
  STATE_PID=$!
else
  STATE_PID=""
fi

# 2. Verifier: root, because the presence key is root-owned 0600 -- anyone who can
#    read K can mint beacons and unlock this Mac. Held in the foreground of its own
#    osascript, which is what keeps it alive.
# The privileged half is written to a launcher rather than inlined, because the
# alternative is three levels of quoting through osascript, and because it has to
# do one more thing than run the verifier:
#
#   IT MUST DIE WITH THE SCANNER.
#
# `tail -f` does not stop when the process writing the file exits, so without
# this the root half outlives every run and stopping the pipeline needs a SECOND
# authorization prompt -- once to start, once to clean up. Watching the scanner's
# pid costs nothing and makes stop free: killing `tail` closes the pipe, the
# verifier sees EOF and exits, the bridge sees EOF, publishes `stopped`, and
# clears the permit on its way out. The whole chain unwinds from one signal we
# are allowed to send.
# The privileged half is a FILE IN THE BUNDLE, not something written here.
#
# It used to be generated into ${WORK} on every run and executed as root, out of
# a directory the user can write to. See presence-privileged.sh for what that
# allowed. Its parameters now travel as environment variables on the command
# line, so nothing executable is created at run time.
PRIV="${HERE}/presence-privileged.sh"
[ -r "${PRIV}" ] || { say "missing presence-privileged.sh next to this script"; exit 2; }

PRIV_ENV="REPOSE_MODE='${MODE}' REPOSE_BIN='${BIN}' REPOSE_KEY_DIR='${KEY_DIR}' \
REPOSE_RAW='${RAW}' REPOSE_VERIFIED='${VERIFIED}' REPOSE_RUNFLAG='${RUNFLAG}' \
REPOSE_PERMIT_DIR='${PERMIT_DIR}' REPOSE_STATUS_FILE='${STATUS_FILE_ARG}' \
REPOSE_LOG_DIR='${WORK}' REPOSE_ALLOW_HOME_BIN='${REPOSE_ALLOW_HOME_BIN:-}' \
REPOSE_NEAR_DBM='${REPOSE_NEAR_DBM:-}' REPOSE_FAR_DBM='${REPOSE_FAR_DBM:-}'"

# Started in the FOREGROUND of a subshell we background ourselves, not detached
# with nohup inside the osascript. `do shell script` reclaims its process group
# when it returns, so anything backgrounded inside it dies the moment the dialog
# is answered -- silently, leaving a pipeline that logs "started" and does
# nothing. Keeping osascript in the foreground is what holds the root process
# alive, and killing that subshell is what takes it down.
say "starting the privileged half as root (one authorization prompt, ${MODE} target)"
# WHO RAISES THE AUTHORIZATION DIALOG
#
# macOS attributes it to the executable that asks. Asking from here means asking
# through /usr/bin/osascript, so the box says "osascript" -- a name nobody
# installed, at the one moment in this product where somebody is typing an
# administrator password. "Do not give your password to software you do not
# recognise" is a good habit and we were training people out of it.
#
# So when the app starts this pipeline it asks in-process instead (NSAppleScript
# from inside Outsie, where the box says Outsie) and sets REPOSE_SKIP_PRIVILEGED.
# It builds the same command from the same paths -- it has all of them -- rather
# than reading one back out of a file this process could write, which would hand
# the app's authorization to whatever the file said.
#
# Run from a terminal, nothing sets that, and this asks the old way: "osascript"
# is then the honest answer to who is asking.
if [ -n "${REPOSE_SKIP_PRIVILEGED:-}" ]; then
  say "the app is raising the authorization itself"
  ROOT_PID=""
else
  run_root "${PRIV_ENV} '${PRIV}'" &
  ROOT_PID=$!
fi
sleep 5

# Did the privileged half actually come up?
#
# It is behind an authorization dialog, and a dialog can be cancelled, dismissed
# by someone who did not expect it, or never noticed at all. When that happens
# the scanner keeps running and keeps writing rows, the process list still shows
# a live pipeline, and nothing verifies anything -- the feature is off, and every
# surface says it is on. That was observed on a real Mac: 77 advertisements
# captured, verified.csv empty, no permit, and a panel reporting a running
# monitor.
#
# The verifier writes its startup line to verify.log the moment it begins, so its
# absence is a reliable "never started".
if [ ! -e "${WORK}/verify.log" ]; then
  say "the privileged half did not start -- the authorization prompt was not completed."
  say "presence monitoring is OFF. Nothing is verifying beacons, and no permit will be written."
  [ -n "${STATUS_FILE_ARG}" ] && printf 'noauth,-,%s\n' "$(date +%s)" > "${STATUS_FILE_ARG}"
  exit 3
fi
# root may have created the status file; the app reads it unprivileged.
# The status file's permissions are handled inside presence-privileged.sh,
# which is already root. Doing it here cost a SECOND authorization dialog --
# titled "osascript", immediately after the one titled Outsie.

# 3. Bridge. Remote: us, so ssh uses our keys. Local: already inside the root
#    half above, so there is nothing left to start here.
say "scanner ${SCAN_PID}, logs in ${WORK}"
if [ "${MODE}" = remote ]; then
  tail -n +1 -f "${VERIFIED}" | bash "${BIN}/permit-bridge.sh"
else
  # The privileged half owns the whole chain; wait on the scanner instead.
  wait "${SCAN_PID}" 2>/dev/null
fi
