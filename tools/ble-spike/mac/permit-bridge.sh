#!/bin/bash
#
# B2: turn the BLE RSSI stream into the presence signal the unlock plugin reads.
#
# rssi-scan.swift only measures: it prints one CSV row per discovery
# (unix_ms,rssi,peripheral_id_prefix) and decides nothing. This bridge is the
# decision. It consumes that stream on stdin, applies near/far hysteresis so the
# phone drifting a dBm around a threshold does not flap the lock, and while the
# phone is "present" it REFRESHES the permit every few seconds. When the phone
# leaves -- or the stream goes quiet, or this bridge dies -- the permit is
# cleared / allowed to go stale, and the plugin falls back to the password.
#
# WHY REFRESH, NOT WRITE-ONCE
# ---------------------------
# The hardened permit is root-owned AND time-limited (plugin.c:
# PERMIT_FRESHNESS_S). A single touch would unlock for the freshness window and
# then stop, and a crashed bridge would leave a permit that expires on its own.
# That is the point: presence is a thing you keep asserting, not a latch. This
# bridge re-touches the permit on a timer strictly shorter than the freshness
# window, so "present" means "asserted within the last few seconds", and any
# failure decays to locked.
#
# WHERE THE PERMIT IS WRITTEN
# ---------------------------
# The plugin runs on the target (the VM in the spike, the same Mac in the
# product). The permit must be root-owned there. So the write/clear are commands,
# injected, defaulting to ssh-into-the-VM-as-root. On a product where the bridge
# and the plugin share a machine, point them at a local `sudo touch`/`rm`. The
# commands are also the test seam: the test points them at a log file and feeds
# synthetic RSSI, so all of the hysteresis/staleness logic is checked with no
# radio and no VM.
#
# CALIBRATION IS NOT DONE (that is B3). The thresholds below are conservative
# placeholders tied to the spike's MEDIUM tx power (rssi at ~1 m measured -84..-77
# dBm). Do not ship these numbers; they exist so the mechanism can be exercised.

set -uo pipefail

NEAR_DBM="${REPOSE_NEAR_DBM:--72}"     # >= this: near enough to count as present
FAR_DBM="${REPOSE_FAR_DBM:--85}"       # <= this: far enough to count as gone
STALE_S="${REPOSE_STALE_S:-8}"         # no sample for this long -> treat as gone
REFRESH_S="${REPOSE_REFRESH_S:-5}"     # re-assert the permit this often while present
                                       # (must be < plugin's PERMIT_FRESHNESS_S=15)

# How presence is asserted / withdrawn on the target. Defaults assume the VM
# harness has REPOSE_SSH exported (tools/vm-spike/vm-env.sh).
PERMIT_ON_CMD="${REPOSE_PERMIT_ON_CMD:-${REPOSE_SSH:-} 'sudo mkdir -p /var/run/repose-spike && sudo chmod 755 /var/run/repose-spike && sudo touch /var/run/repose-spike/permit'}"
PERMIT_OFF_CMD="${REPOSE_PERMIT_OFF_CMD:-${REPOSE_SSH:-} 'sudo rm -f /var/run/repose-spike/permit'}"

now_s() { date +%s; }
log()   { printf 'permit-bridge: %s\n' "$*" >&2; }

assert_permit() { eval "${PERMIT_ON_CMD}"  >/dev/null 2>&1 || log "warn: permit-on command failed"; }
clear_permit()  { eval "${PERMIT_OFF_CMD}" >/dev/null 2>&1 || log "warn: permit-off command failed"; }

# A well-formed integer dBm (negative). Anything else (header, blank, garbage)
# is skipped rather than misread as a strong signal.
is_dbm() { case "$1" in ''|*[!0-9-]*) return 1 ;; -[0-9]*|[0-9]*) return 0 ;; *) return 1 ;; esac; }

present=0
last_sample=0
last_refresh=0

log "starting: near>=${NEAR_DBM} far<=${FAR_DBM} stale=${STALE_S}s refresh=${REFRESH_S}s"

# read -t returns >128 on timeout, non-zero on EOF. On EOF we stop; on timeout we
# fall through to the staleness/refresh housekeeping with no new sample.
while :; do
    line=""
    if IFS= read -r -t "${REFRESH_S}" line; then
        read_rc=0
    else
        read_rc=$?
        # EOF (rc 1) with an empty line and no more input: the scanner exited.
        if [ "${read_rc}" -le 1 ] && [ -z "${line}" ]; then
            log "input stream ended; clearing permit and exiting"
            [ "${present}" = 1 ] && clear_permit
            exit 0
        fi
    fi

    now="$(now_s)"

    if [ -n "${line}" ]; then
        # CSV: unix_ms,rssi,id -- take field 2.
        rssi="$(printf '%s' "${line}" | cut -d, -f2 | tr -d '[:space:]')"
        if is_dbm "${rssi}"; then
            last_sample="${now}"
            if [ "${rssi}" -ge "${NEAR_DBM}" ]; then
                if [ "${present}" = 0 ]; then
                    present=1; last_refresh="${now}"
                    log "ENTER (rssi=${rssi}) -> asserting permit"
                    assert_permit
                fi
            elif [ "${rssi}" -le "${FAR_DBM}" ]; then
                if [ "${present}" = 1 ]; then
                    present=0
                    log "LEAVE (rssi=${rssi}) -> clearing permit"
                    clear_permit
                fi
            fi
            # between FAR and NEAR: hold current state (that is the hysteresis).
        fi
    fi

    # Staleness: presence must be continuously re-observed. A quiet stream means
    # the phone is out of range or the scanner stalled -- either way, not present.
    if [ "${present}" = 1 ] && [ "$((now - last_sample))" -ge "${STALE_S}" ]; then
        present=0
        log "STALE (no sample for $((now - last_sample))s) -> clearing permit"
        clear_permit
    fi

    # Keep the permit fresh while present.
    if [ "${present}" = 1 ] && [ "$((now - last_refresh))" -ge "${REFRESH_S}" ]; then
        last_refresh="${now}"
        assert_permit
    fi
done
