#!/bin/bash
#
# B2: turn the verified BLE stream into the presence signal the unlock plugin reads.
#
# The pipeline is three processes, each of which can only do one thing:
#
#   rssi-scan | sudo presence-verify | permit-bridge.sh
#   measures    authenticates          decides
#
# rssi-scan holds no key and decides nothing. presence-verify holds the key and
# has no radio. This bridge holds neither and makes the near/far call. It applies
# hysteresis so the phone drifting a dBm around a threshold does not flap the
# lock, and while the phone is "present" it REFRESHES the permit every few
# seconds. When the phone leaves -- or the stream goes quiet, or this bridge dies
# -- the permit is cleared / allowed to go stale, and the plugin falls back to
# the password.
#
# BOTH GATES, NOT EITHER
# ----------------------
# A row counts as the phone only when auth=VALID *and* rssi >= NEAR. Before E13
# was fixed there was no auth field at all: presence meant "some Android is
# broadcasting a UUID published in this repository", so anyone could unlock this
# Mac by walking past it with a copy of our app. RSSI was never a second factor
# for that -- proximity is not identity, and an imposter standing next to the Mac
# has excellent RSSI.
#
# There is deliberately no switch to turn the auth gate off. A safeguard with an
# opt-out defaults to the state someone forgot to change, and this project has
# already shipped three documents describing protections the code did not have.
# Rows without a verdict field are treated as unverified and ignored, so pointing
# this bridge at the raw scanner (no verifier in the pipe) yields no permits
# rather than every permit.
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
# WHY 45
# ------
# The two errors this number can make are not symmetric.
#
# Too long: someone who has walked away stays "present" a while longer.
# Too short: someone sitting at their desk, phone in their pocket, gets asked
# for a password for no reason -- repeatedly, in the middle of working.
#
# It started at 8. A capture with the phone motionless 1m away measured the
# interval between sightings as p99 ~7s and max ~9.3s, so 8 meant the bridge
# declared the phone gone roughly every 2.5 minutes while it lay on the desk.
# docs/validation/2026-09-10-scan-cadence.md establishes that this tail belongs
# to macOS's scan cadence for a single advertiser and cannot be reduced from the
# phone: address stability, tx power and in-place payload updates were each
# tried and none of them moved it.
#
# So any packet-recency threshold under about 10s is guaranteed to fire on a
# phone that never went anywhere. 45 leaves roughly 5x margin over a tail that
# was only sampled for a few minutes and will be worse over a workday.
#
# WHAT 45 DOES NOT COST
#
# A verified beacon is already accepted across the current window +/-1, so a
# captured one stays usable for up to 2*WINDOW = 60s no matter what this value
# is. Anything up to 60s therefore grants no exposure the design has not already
# accepted (see the design's replay section). Beyond 60s it would, which is the
# real ceiling -- and permit-bridge-test.sh reads WINDOW out of the verifier and
# asserts it.
#
# It also does not weaken the dead-bridge case. While the bridge believes the
# phone is present it re-touches the permit every REFRESH_S, so the permit stays
# fresh through these gaps. If the bridge dies, nothing refreshes and the permit
# ages out at the plugin's PERMIT_FRESHNESS_S (15s) regardless of STALE_S. An
# earlier version of this comment claimed STALE_S had to stay under that 15s;
# that was wrong, and it had the effect of pinning the default underneath the
# noise floor.
#
# CALIBRATION IS NOT DONE (that is B3). The thresholds below are conservative
# placeholders tied to the spike's MEDIUM tx power (rssi at ~1 m measured -84..-77
# dBm). Do not ship these numbers; they exist so the mechanism can be exercised.

set -uo pipefail

NEAR_DBM="${REPOSE_NEAR_DBM:--72}"     # >= this: near enough to count as present
FAR_DBM="${REPOSE_FAR_DBM:--85}"       # <= this: far enough to count as gone

# Per-phone bands, as `keyId:near:far` separated by commas.
#
# Two phones do not look the same to this Mac even standing in the same place:
# transmit power differs by model, and a phone in a pocket is several dB down
# from one on the desk. One shared pair of thresholds means calibrating for one
# of them and being wrong about the other -- either locking while its owner is
# still sitting there, or staying open after they have gone.
#
# NEAR_DBM/FAR_DBM above remain the fallback for any key with no entry, which is
# every key until someone walks the calibration.
BANDS="${REPOSE_BANDS:-}"

# Echo `near far` for a key id, falling back to the shared pair.
band_for() {
    _k="$1"
    case ",${BANDS}," in
        *",${_k}:"*)
            _b="${BANDS#*,${_k}:}"      # may still have a leading entry
            case "${BANDS}" in
                "${_k}:"*) _b="${BANDS#${_k}:}" ;;
            esac
            _b="${_b%%,*}"
            printf '%s %s' "${_b%%:*}" "${_b#*:}"
            return
            ;;
    esac
    printf '%s %s' "${NEAR_DBM}" "${FAR_DBM}"
}
STALE_S="${REPOSE_STALE_S:-45}"        # no sample for this long -> treat as gone
                                       # (see "WHY 45" below; being wrong here
                                       # interrupts someone who is working)
REFRESH_S="${REPOSE_REFRESH_S:-5}"     # re-assert the permit this often while present
                                       # (must be < plugin's PERMIT_FRESHNESS_S=15)

# How presence is asserted / withdrawn on the target. Defaults assume the VM
# harness has REPOSE_SSH exported (tools/vm-spike/vm-env.sh).
# How the screen is locked when the phone asks.
#
# NOT ScreenSaverEngine. tools/vm-spike/vm-env.sh recommends
# `open -a ScreenSaverEngine`, and that was verified -- on macOS 14.6.1, in a
# VM. On macOS 26 the command returns success and does not lock, so this bridge
# logged "command lock -> starting the screensaver" three times while the screen
# stayed exactly where it was. A lock command that reports success without
# locking is worse than one that fails.
#
# `pmset displaysleepnow` locks ONLY when the screen-lock delay is immediate --
# which is the same precondition the whole feature rests on, and which
# presence-pipeline.sh checks. With a delay, this blanks the screen and leaves
# the session open, so the guard is not optional.
LOCK_CMD="${REPOSE_LOCK_CMD:-launchctl asuser \$(stat -f %u /dev/console) /usr/bin/pmset displaysleepnow}"
PERMIT_ON_CMD="${REPOSE_PERMIT_ON_CMD:-${REPOSE_SSH:-} 'sudo mkdir -p /var/run/repose-spike && sudo chmod 755 /var/run/repose-spike && sudo touch /var/run/repose-spike/permit'}"
PERMIT_OFF_CMD="${REPOSE_PERMIT_OFF_CMD:-${REPOSE_SSH:-} 'sudo rm -f /var/run/repose-spike/permit'}"

# Where the decision is published for anything that wants to show it -- the Mac
# app's status panel, mainly. Unset means write nothing, so the spike keeps
# working exactly as before.
#
# It carries the RSSI and a timestamp, not just a word, because a reader has to
# be able to tell "away" from "this file is stale because the bridge died". A
# status file with no clock is indistinguishable from a status file nobody is
# updating, and reading the second as the first is how a panel ends up showing a
# confident green dot for a process that exited an hour ago.
STATUS_FILE="${REPOSE_STATUS_FILE:-}"

publish() {
    [ -n "${STATUS_FILE}" ] || return 0
    printf '%s,%s,%s\n' "$1" "${2:--}" "$(date +%s)" > "${STATUS_FILE}.tmp" 2>/dev/null \
        && mv -f "${STATUS_FILE}.tmp" "${STATUS_FILE}" 2>/dev/null
}

now_s() { date +%s; }
log()   { printf 'permit-bridge: %s\n' "$*" >&2; }

assert_permit() { eval "${PERMIT_ON_CMD}"  >/dev/null 2>&1 || log "warn: permit-on command failed"; }

# Act on a verified phone command.
#
# WHY LOCKING NEEDS launchctl asuser
#
# This bridge runs as root, outside any GUI session. A lock request issued from
# here reaches nobody: the screen belongs to the console user's Aqua session,
# and root is not in it. `launchctl asuser <uid>` re-enters that session, which
# is the same trick the pipeline already uses for anything user-facing.
#
# WHY THE UNLOCK COMMAND ONLY WRITES A PERMIT
#
# It cannot do more. `system.login.screensaver` is consulted when a human
# submits at the lock screen; nothing can submit on their behalf, so the most a
# phone can do is authorize the attempt that follows. The button is labelled to
# say exactly that -- calling it 「解锁」 and having the Mac sit there would be
# the same lie this project keeps catching in its own screens.
run_command() {
    case "$1" in
        lock)
            # ScreenSaverEngine, not `pmset displaysleepnow`.
            #
            # displaysleepnow only sleeps the display; whether the session locks
            # depends on the screen-lock delay, so with a non-zero delay the Mac
            # goes dark and stays unlocked. tools/vm-spike/vm-env.sh has the
            # full account -- that mistake once produced a plugin log line
            # proving the mechanism ran next to IOConsoleLocked=false, which is
            # how a confident wrong answer gets made.
            #
            # Overridable so a test can prove the command reaches this branch
            # without actually locking the tester's screen -- the same seam
            # tools/vm-spike/vm-env.sh and tests/e2e/unlock_acceptance.sh use.
            # Refuse rather than blank. See LOCK_CMD.
            if ! sysadminctl -screenLock status 2>&1 | grep -q immediate; then
                log "command lock REFUSED: this Mac's screen-lock delay is not immediate, \
so sleeping the display would leave the session unlocked"
                return
            fi
            log "command lock -> sleeping the display (screen lock is immediate)"
            eval "${LOCK_CMD}" >/dev/null 2>&1 || log "warn: lock command failed"
            ;;
    esac
}
clear_permit()  { eval "${PERMIT_OFF_CMD}" >/dev/null 2>&1 || log "warn: permit-off command failed"; }

# A well-formed integer dBm (negative). Anything else (header, blank, garbage)
# is skipped rather than misread as a strong signal.
is_dbm() { case "$1" in ''|*[!0-9-]*) return 1 ;; -[0-9]*|[0-9]*) return 0 ;; *) return 1 ;; esac; }

present=0
last_sample=0
last_refresh=0
last_reject=""

# Being killed is a normal way for this to end -- the app stops the pipeline, the
# machine sleeps, someone quits. Dying quietly would leave two lies behind: a
# permit that stays valid until the plugin's freshness window expires, and a
# status file whose last word is `near` on a Mac where nothing is watching any
# more. The timestamp means a reader eventually works that out, but eventually is
# not the same as immediately, and the door should not be the thing that waits.
on_signal() {
    log "signalled; clearing the permit and standing down"
    publish stopped
    clear_permit
    exit 0
}
# EXIT as well as the signals: a bridge that ends because its input closed should
# close the door just as firmly as one that was signalled.
#
# WHAT THIS DOES NOT COVER, measured rather than assumed. Tearing the pipeline
# down from outside does not reliably reach this handler -- the privileged half
# runs under an authorization dialog's process group, and several attempts at
# unwinding it in order (watch the scanner's pid, watch a flag file, flatten the
# process tree, signal children before the group) each still left the chain being
# killed from above instead of from within. So after a forced stop the permit can
# survive until the plugin ages it out.
#
# That bound is the design's, not an accident: PERMIT_FRESHNESS_S is 15s and
# permit-bridge-test.sh asserts the refresh interval stays under it, so an
# abandoned permit is dead within fifteen seconds whether anyone tidied up or
# not. Immediate teardown would be nicer. It is not what keeps the door shut.
trap on_signal TERM INT HUP EXIT

# Empty when nobody passed one: a bridge run by hand or by the test harness has
# no run to be part of, and must not exit because a file it was never told
# about does not exist.
RUN_FLAG="${REPOSE_RUNFLAG:-}"

publish starting
log "starting: auth=VALID required, near>=${NEAR_DBM} far<=${FAR_DBM}${BANDS:+ bands=${BANDS}} stale=${STALE_S}s refresh=${REFRESH_S}s"

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
            publish stopped
            [ "${present}" = 1 ] && clear_permit
            exit 0
        fi
    fi

    now="$(now_s)"

    # Stop when the run is over.
    #
    # This compared $PPID and exited when it changed. That is right for a direct
    # child and WRONG here: in `tail | verify | tee | bash permit-bridge.sh &`
    # the shell forks a subshell that starts the members and then goes away, so
    # the parent legitimately changes within milliseconds. The guard fired on
    # every healthy run -- "parent went away", seconds after starting -- taking
    # the permit with it. Presence stopped working entirely, and the phone's
    # lock command had nothing left to reach.
    #
    # The run flag has no such ambiguity: the unprivileged half creates it
    # before starting anything and removes it when the run ends, and it is
    # already what the privileged script watches. One fact, one source.
    #
    # EOF on stdin remains the ordinary exit. This covers what the parent check
    # was aimed at: a supervisor dying without reaping us, leaving a root
    # process the app cannot signal.
    if [ -n "${RUN_FLAG}" ] && [ ! -e "${RUN_FLAG}" ]; then
        log "run flag gone; clearing permit and exiting"
        publish stopped
        [ "${present}" = 1 ] && clear_permit
        exit 0
    fi

    if [ -n "${line}" ]; then
        # CSV: unix_ms,rssi,... then presence-verify's named fields at the end,
        # `auth=<verdict>,cmd=<n>`.
        #
        # Read by NAME, not by column. The verdict used to be field 7; widening
        # the scanner's output by two columns moved it, and every row silently
        # became unverified -- fail-safe, but a whole feature not working and
        # nothing saying so. A row with no auth= field (the raw scanner, no
        # verifier in the pipe) yields an empty verdict, which is refused.
        rssi="$(printf '%s' "${line}" | cut -d, -f2 | tr -d '[:space:]')"
        fields="$(printf '%s' "${line}" | tr ',' '\n')"
        auth="$(printf '%s' "${fields}" | sed -n 's/^auth=//p' | tr -d '[:space:]')"
        vcmd="$(printf '%s' "${fields}" | sed -n 's/^cmd=//p' | tr -d '[:space:]')"
        # Column 5 is the key id, i.e. which phone this row is about. Read
        # positionally like the rssi beside it, and only used to pick a band --
        # a wrong id here costs the wrong thresholds, never a wrong verdict,
        # because auth= is what decides whether the row counts at all.
        kid="$(printf '%s' "${line}" | cut -d, -f5 | tr -d '[:space:]')"
        set -- $(band_for "${kid}")
        near_now="$1"; far_now="$2"

        # Commands arrive already judged.
        #
        # presence-verify emits a non-zero command only when the tag verified
        # AND the sequence was new, so there is nothing to re-check here and --
        # more to the point -- nowhere else that could decide differently. A
        # second opinion about whether a command is genuine is a second place to
        # get it wrong.
        # Only 1. Command 2 (allow-unlock) was built and withdrawn: a near,
        # switched-on phone already makes this bridge assert the permit several
        # times a minute, so the command changed nothing observable. Nothing
        # sends it now, so nothing here answers it -- an unreachable branch that
        # still looks alive is the thing this project keeps deleting.
        case "${vcmd}" in
            1) run_command lock ;;
        esac

        # An unverified row is not a weak signal, it is a device we cannot name.
        # It updates nothing -- not even last_sample -- so a stream of imposter
        # beacons cannot hold a stale permit alive.
        if [ "${auth}" != "VALID" ]; then
            if [ -n "${auth}" ] && [ "${auth}" != "${last_reject}" ]; then
                last_reject="${auth}"
                log "ignoring ${auth} beacons (rssi=${rssi}) -- not the paired device"
            fi
            rssi=""
        fi

        if is_dbm "${rssi}"; then
            last_sample="${now}"
            if [ "${rssi}" -ge "${near_now}" ]; then
                if [ "${present}" = 0 ]; then
                    present=1; last_refresh="${now}"
                    log "ENTER (rssi=${rssi}) -> asserting permit"
                    publish near "${rssi}"
                    assert_permit
                fi
            elif [ "${rssi}" -le "${far_now}" ]; then
                if [ "${present}" = 1 ]; then
                    present=0
                    log "LEAVE (rssi=${rssi}) -> clearing permit"
                    publish away "${rssi}"
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
        publish away
        clear_permit
    fi

    # Keep the permit fresh while present.
    # The status line is a HEARTBEAT, not a change log.
    #
    # This block used to run only while present, so walking away stopped the
    # publishing entirely: the last line aged past the reader's freshness
    # window and the app concluded that presence monitoring had stopped -- on a
    # pipeline that was running perfectly, doing exactly what it should. The
    # panel's switch then showed OFF and could not be turned on, because the
    # start path saw a live pid and did nothing. Walking away from your desk
    # wedged the control.
    #
    # So it beats either way. The permit is still asserted only while present;
    # that part was never in question.
    if [ "$((now - last_refresh))" -ge "${REFRESH_S}" ]; then
        last_refresh="${now}"
        if [ "${present}" = 1 ]; then
            publish near "${rssi:-}"
            assert_permit
        else
            publish away "${rssi:-}"
        fi
    fi
done
