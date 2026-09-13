#!/bin/bash
#
# E12 -- close the E3 fail-open window.
#
# WHY THIS EXISTS
# ---------------
# E3/E8/E11 established that when system.login.screensaver references our
# mechanism and the mechanism's bundle is missing (or unloadable), macOS's
# authorization engine treats the un-instantiable step as "passed" and grants
# the unlock WITHOUT a password. E8/E11 further proved this cannot be fixed by
# any rule shape: passwordless-when-present and fail-closed-on-missing-bundle
# are mutually exclusive at the real lock screen. So the only defence is
# prevention -- never leave the rule referencing a bundle that is not present
# and loadable.
#
# install.sh / uninstall.sh already keep the safe ordering for the scripted
# path. This guards the UNSCRIPTED paths: the user drags the app to the Trash,
# an upgrade dies half-way, an OS migration empties SecurityAgentPlugins. When
# that happens the rule is left dangling and the machine becomes unlockable by
# anyone. This check detects that and repairs the authorization database back to
# password-only, using the same safety-checked authdb-edit the installer uses.
#
# It runs as a LaunchDaemon: at boot (RunAtLoad), whenever the plugins
# directory changes (WatchPaths), and on a slow timer (StartInterval) as a
# backstop. It cannot repair the instant a locked machine's bundle vanishes --
# nothing can, the machine is already locked -- so its job is to keep the window
# between "bundle gone" and "rule repaired" as small as possible.
#
# SAFETY STANCE
# -------------
# It repairs ONLY when it can positively confirm the reference is dangling
# (bundle missing) or the bundle is unloadable (codesign fails). If it cannot
# read the authorization database at all, it does NOTHING -- guessing here could
# tear down a working password path. Removal goes through authdb-edit, which
# refuses to leave a rule with no password fallback.
#
# Every external touch point is overridable by environment variable so the test
# can drive the whole thing in a sandbox with no root and no real authdb.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RIGHT="${REPOSE_HC_RIGHT:-system.login.screensaver}"
SUBRULE="${REPOSE_HC_SUBRULE:-ai.repose.spike}"
PLUGINS_DIR="${REPOSE_HC_PLUGINS_DIR:-/Library/Security/SecurityAgentPlugins}"
EDIT="${REPOSE_HC_EDIT:-${HERE}/authdb-edit}"
# Transit file for the repaired rule lives in a root-owned 0700 directory, never
# /tmp: it is piped straight into `authorizationdb write`, and a predictable
# world-writable path would let a local user swap in a rule of their choosing
# between validation and write. Same reasoning as uninstall.sh.
STATE_DIR="${REPOSE_HC_STATE_DIR:-/var/db/repose-spike}"

# Overridable so the test can supply a fake authdb and codesign. Defaults are
# the real macOS tools. Each is eval'd with the right/subrule/bundle appended.
AUTHDB_READ_CMD="${REPOSE_HC_READ_CMD:-security authorizationdb read}"
AUTHDB_WRITE_CMD="${REPOSE_HC_WRITE_CMD:-security authorizationdb write}"
CODESIGN_VERIFY_CMD="${REPOSE_HC_CODESIGN_CMD:-codesign --verify}"

DRY_RUN="${REPOSE_HC_DRY_RUN:-0}"

log() {
    local msg="$*"
    # os_log survives when the filesystem does not; stderr is for the test and
    # for `launchctl` to capture. Both, on purpose (same rationale as plugin.c).
    command -v logger >/dev/null 2>&1 && logger -t repose-healthcheck -- "$msg"
    printf 'repose-healthcheck: %s\n' "$msg" >&2
}

# Read a right's `rule` array as JSON. Empty on any failure.
#
# `plutil -extract ... -o -` writes its ERROR text to stdout (that dash IS the
# output), so a failed extract does not produce empty output -- it produces an
# error string. Capturing that as if it were data is how "cannot read the rule"
# silently turned into "the rule does not mention us", skipping the safety stop.
# So the exit code decides, never the emptiness of the text.
read_rule_json() {
    local out
    out="$(eval "${AUTHDB_READ_CMD} '${RIGHT}'" 2>/dev/null | plutil -extract rule json -o - - 2>/dev/null)" \
        && printf '%s' "${out}"
}

# The bundle names referenced by the sub-rule's mechanisms, one per line,
# excluding macOS's own mechanism hosts (which are not bundles on disk).
bundle_names() {
    local mechs="$1" m prefix
    printf '%s' "$mechs" | grep -oE '"[^"]+"' | tr -d '"' | while IFS= read -r m; do
        prefix="${m%%:*}"
        case "$prefix" in
            builtin|loginwindow|PKINITMechanism|HomeDirMechanism|MCXMechanism|CryptoTokenKit|PowerPCMechanism|"")
                continue ;;
            *) printf '%s\n' "$prefix" ;;
        esac
    done | sort -u
}

# Rewrite the rule with the sub-rule removed, via the same safety-checked tool
# the installer uses. Returns 0 only if the machine is left with a working
# password path and the reference is actually gone.
repair() {
    mkdir -p "${STATE_DIR}" 2>/dev/null || true
    chmod 700 "${STATE_DIR}" 2>/dev/null || true
    local transit="${STATE_DIR}/${RIGHT}.healthcheck.plist"

    eval "${AUTHDB_READ_CMD} '${RIGHT}'" > "${transit}" 2>/dev/null \
        || { log "repair aborted: could not read ${RIGHT}"; rm -f "${transit}"; return 1; }

    if ! "${EDIT}" remove-subrule "${transit}" "${SUBRULE}" >/dev/null 2>&1; then
        log "repair aborted: authdb-edit refused to remove ${SUBRULE} (would it leave no password path?); NOT writing"
        rm -f "${transit}"; return 1
    fi
    if ! "${EDIT}" validate "${transit}" >/dev/null 2>&1; then
        log "repair aborted: cleaned rule failed validate; NOT writing"
        rm -f "${transit}"; return 1
    fi

    if [ "${DRY_RUN}" = "1" ]; then
        log "DRY RUN: would revert ${RIGHT} to password-only (remove ${SUBRULE})"
        rm -f "${transit}"; return 0
    fi

    # `security authorizationdb write` prints its own "YES (0)" authorization
    # confirmation, which is just noise in the daemon log; the exit code (checked
    # here) is the only part that matters, and a real failure is logged below.
    if ! eval "${AUTHDB_WRITE_CMD} '${RIGHT}'" < "${transit}" >/dev/null 2>&1; then
        log "repair FAILED: could not write ${RIGHT}"; rm -f "${transit}"; return 1
    fi
    rm -f "${transit}"

    if read_rule_json | grep -q "${SUBRULE}"; then
        log "repair FAILED: ${RIGHT} still references ${SUBRULE}"; return 1
    fi
    log "REPAIRED: removed dangling ${SUBRULE} from ${RIGHT}; screensaver is now password-only"
    return 0
}

main() {
    local rule_json mechs name bpath dangling=0

    rule_json="$(read_rule_json)"
    if [ -z "${rule_json}" ]; then
        # Cannot read the rule at all. Do NOT guess -- removing a subrule we
        # cannot see, or writing over an unreadable rule, is how a health check
        # becomes the outage.
        log "cannot read ${RIGHT}; doing nothing"
        exit 0
    fi

    case "${rule_json}" in
        *"${SUBRULE}"*) ;;
        *) log "healthy: ${RIGHT} does not reference ${SUBRULE}"; exit 0 ;;
    esac

    # Same plutil-writes-errors-to-stdout trap as read_rule_json: let the exit
    # code decide, or a failed extract's error text would read as "mechanisms".
    if ! mechs="$(eval "${AUTHDB_READ_CMD} '${SUBRULE}'" 2>/dev/null | plutil -extract mechanisms json -o - - 2>/dev/null)"; then
        mechs=""
    fi
    if [ -z "${mechs}" ]; then
        # The screensaver rule names our sub-rule, but the sub-rule's mechanisms
        # cannot be read -- the sub-rule is gone or broken while still
        # referenced. That is exactly the dangling state.
        log "${SUBRULE} is referenced but its mechanisms are unreadable; treating as dangling"
        repair; exit $?
    fi

    while IFS= read -r name; do
        [ -n "${name}" ] || continue
        bpath="${PLUGINS_DIR}/${name}.bundle"
        if [ ! -d "${bpath}" ]; then
            log "referenced bundle is MISSING: ${bpath}"; dangling=1
        elif ! eval "${CODESIGN_VERIFY_CMD} '${bpath}'" >/dev/null 2>&1; then
            log "referenced bundle FAILS codesign: ${bpath}"; dangling=1
        else
            log "referenced bundle ok: ${bpath}"
        fi
    done < <(bundle_names "${mechs}")

    if [ "${dangling}" = "1" ]; then
        log "dangling/invalid reference detected; repairing to close the fail-open window"
        repair; exit $?
    fi

    log "healthy: ${RIGHT} references ${SUBRULE} and every referenced bundle is present and valid"
    exit 0
}

# Sourcing exposes the helpers (bundle_names, repair) without running, so the
# test can exercise them directly.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    main "$@"
fi
