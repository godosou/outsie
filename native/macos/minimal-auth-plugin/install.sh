#!/bin/bash
#
# Install the Outsie spike Authorization Plugin and wire it into
# system.login.screensaver, mirroring the pattern that is already proven to
# load on macOS (a cdhash-pinned evaluate-mechanisms sub-rule referenced from
# the screensaver rule with k-of-n=1, so the password path still works).
#
# Throwaway experiment. Run only on a disposable, rollback-capable test VM
# with SIP off. See README.md.
#
set -euo pipefail

BUNDLE_NAME="ReposeSpike"
BUILT_BUNDLE="build/${BUNDLE_NAME}.bundle"
DEST_BUNDLE="/Library/Security/SecurityAgentPlugins/${BUNDLE_NAME}.bundle"
RIGHT="system.login.screensaver"
SUBRULE="ai.repose.spike"
# The backup must outlive a reboot: rollback is most often needed *after* the
# machine has been restarted, and /tmp is cleared on boot.
BACKUP_DIR="/var/db/repose-spike"
BACKUP="${BACKUP_DIR}/${RIGHT}.backup.plist"

# Milestone selector: "permit" (default, file-triggered) or "log" (always allow).
MODE="${1:-permit}"
if [[ "${MODE}" != "permit" && "${MODE}" != "log" ]]; then
    echo "usage: sudo $0 [permit|log]" >&2
    exit 2
fi
# The ",privileged" suffix decides which host runs the mechanism: with it, the
# root authorizationhost; without it, SecurityAgent in the user's context. The
# one third-party plugin observed working on a real Mac
# (CodexComputerUseAuthorizationPlugin:allow) does NOT use it, so this is an
# experimental variable rather than a setting -- flip it with PRIVILEGED=0.
PRIVILEGED="${PRIVILEGED:-1}"
if [[ "${PRIVILEGED}" == "1" ]]; then
    MECHANISM="${BUNDLE_NAME}:${MODE},privileged"
else
    MECHANISM="${BUNDLE_NAME}:${MODE}"
fi

cd "$(dirname "$0")"
[[ $EUID -eq 0 ]] || { echo "Must run as root (sudo $0 ${MODE})." >&2; exit 1; }
[[ -d "${BUILT_BUNDLE}" ]] || { echo "Missing ${BUILT_BUNDLE}. Run 'make' first." >&2; exit 1; }

# The screensaver rule must be class=rule with a <rule> array (the macOS 14
# default and the shape this pattern relies on). Refuse to guess otherwise.
CUR_CLASS="$(security authorizationdb read "${RIGHT}" 2>/dev/null \
    | plutil -extract class raw -o - - 2>/dev/null || true)"
if [[ "${CUR_CLASS}" != "rule" ]]; then
    echo "Unexpected '${RIGHT}' class='${CUR_CLASS:-<none>}' (expected 'rule')." >&2
    echo "Inspect it manually before wiring the spike:" >&2
    echo "  security authorizationdb read ${RIGHT}" >&2
    exit 1
fi

# E12 health-check daemon: reverts the screensaver rule to password-only if the
# bundle ever goes missing out-of-band (Trash, failed upgrade, OS migration),
# which is the only defence against the E3 fail-open (no rule shape closes it;
# see docs/validation/2026-09-09-e11-lockscreen-grant-model.md).
SRC_DIR="$(dirname "$0")"
SUPPORT_DIR="/Library/Application Support/ReposeSpike"
DAEMON_LABEL="ai.repose.spike.healthcheck"
DAEMON_PLIST="/Library/LaunchDaemons/${DAEMON_LABEL}.plist"

cat <<EOF
This will make the following changes to THIS machine:

  1. Copy   ${BUILT_BUNDLE}
       ->   ${DEST_BUNDLE}
  2. Back up the current '${RIGHT}' rule to:
       ${BACKUP}
  3. Create authorization right '${SUBRULE}' (evaluate-mechanisms):
       mechanism  ${MECHANISM}
  4. Prepend '${SUBRULE}' to '${RIGHT}'. k-of-n is left exactly as it is; the
     install refuses unless it is already 1, so the spike runs first and the
     normal password path stays as the fallback.
  5. Install the health-check daemon '${DAEMON_LABEL}':
       support  ${SUPPORT_DIR}/
       daemon   ${DAEMON_PLIST}
     It reverts '${RIGHT}' to password-only if the bundle ever disappears while
     the rule still references it (the E3 fail-open).

uninstall.sh restores the backup exactly, removes '${SUBRULE}', deletes the
bundle, and removes the health-check daemon.
EOF
# --yes exists so the A1 ladder can be re-run identically three times if the
# first signing configuration does not load. A prompt in the middle of a
# scripted experiment is how runs end up subtly different from each other.
if [[ "${ASSUME_YES:-}" != "1" ]]; then
    read -r -p "Proceed? [y/N] " reply
    [[ "${reply}" == "y" || "${reply}" == "Y" ]] || { echo "Aborted."; exit 0; }
fi

# Milestone A's entire verdict is "did a line appear in this log". A log left
# over from a previous run would be read as a fresh success, so it is cleared
# here rather than trusted to be empty. Same for a stale permit file, which
# would silently turn the permit mechanism into an unconditional allow.
echo "==> Clearing previous run evidence"
# Clear the permit at the hardened path and the old /tmp path a legacy install
# may have left. The permit's directory (/var/run/repose-spike, root-only) is
# created by whoever writes the permit -- the BLE bridge, or the test harness
# over ssh -- and /var/run is cleared on boot anyway.
rm -f /tmp/repose-plugin.log /var/run/repose-spike/permit /tmp/repose-permit

echo "==> Installing bundle"
# Stage beside the destination and swap, rather than deleting what is currently
# in place and hoping the copy succeeds. A failed copy after the delete would
# leave the authorization rule pointing at a bundle that is no longer there.
STAGED="${DEST_BUNDLE}.incoming"
rm -rf "${STAGED}"
cp -R "${BUILT_BUNDLE}" "${STAGED}"
chown -R root:wheel "${STAGED}"
codesign --verify --deep "${STAGED}" 2>/dev/null \
    || { echo "Staged bundle does not verify; leaving the existing one alone." >&2
         rm -rf "${STAGED}"; exit 1; }
rm -rf "${DEST_BUNDLE}"
mv "${STAGED}" "${DEST_BUNDLE}"

# Recorded so the log says which binary is actually in place, and read from the
# installed copy rather than the build directory -- computing it before the copy
# is how an earlier version ended up pinning a hash that matched nothing.
CDHASH="$(codesign -dvvv "${DEST_BUNDLE}" 2>&1 | sed -n 's/^CDHash=//p')"
[[ -n "${CDHASH}" ]] || { echo "Could not read installed cdhash." >&2; exit 1; }
echo "    installed cdhash ${CDHASH}"

echo "==> Backing up ${RIGHT} to ${BACKUP}"
mkdir -p "${BACKUP_DIR}"
chown root:wheel "${BACKUP_DIR}"
chmod 700 "${BACKUP_DIR}"
# Never clobber the pristine pre-spike rule with an already-modified one: if a
# backup exists, a previous install already captured the original.
if [[ -s "${BACKUP}" ]]; then
    echo "    keeping existing backup (captured before the first install)"
else
    # Write to a temporary file and only move it into place once it validates.
    # Redirecting straight into ${BACKUP} creates the file before the read runs,
    # so an interrupt or a failure leaves a zero-byte "backup" behind.
    TMP_BACKUP="${BACKUP}.partial"
    security authorizationdb read "${RIGHT}" > "${TMP_BACKUP}"
    "$(dirname "$0")/authdb-edit" validate "${TMP_BACKUP}" >/dev/null \
        || { echo "Refusing to install: could not capture a restorable backup." >&2
             rm -f "${TMP_BACKUP}"; exit 1; }
    chmod 600 "${TMP_BACKUP}"
    mv "${TMP_BACKUP}" "${BACKUP}"
fi

# No 'requirement' key. It does not constrain the plugin: authd discards the
# value on write and never evaluates it during screensaver authorization, so a
# cdhash written there is decoration that implies a guarantee nobody enforces.
# The plugin's integrity rests on the directory being root-owned and on the
# platform's own loading policy, which is exactly what milestone A measures.
echo "==> Creating right ${SUBRULE}"
security authorizationdb write "${SUBRULE}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>class</key>
	<string>evaluate-mechanisms</string>
	<key>mechanisms</key>
	<array>
		<string>${MECHANISM}</string>
	</array>
	<key>shared</key>
	<true/>
	<key>tries</key>
	<integer>1</integer>
</dict>
</plist>
EOF

echo "==> Wiring ${SUBRULE} into ${RIGHT}"
# Never /tmp. This file is piped straight into `security authorizationdb write`,
# so a predictable path in a world-writable directory would let a local user
# swap in a rule of their choosing between the edit and the write.
NEW_RULE="${BACKUP_DIR}/${RIGHT}.new.plist"
# Build from the rule as it is now, not from the backup. The backup is the
# pristine pre-spike state and is deliberately never overwritten, so using it
# here would silently revert anything another tool added since.
security authorizationdb read "${RIGHT}" > "${NEW_RULE}"
"$(dirname "$0")/authdb-edit" add-subrule "${NEW_RULE}" "${SUBRULE}"
security authorizationdb write "${RIGHT}" < "${NEW_RULE}"
rm -f "${NEW_RULE}"

echo "==> Installing health-check daemon ${DAEMON_LABEL}"
# healthcheck.sh looks for authdb-edit beside itself, so both land in the same
# root-owned directory. The daemon runs as root; these must not be writable by
# anyone else, or the very thing guarding the unlock rule could be swapped out.
install -d -o root -g wheel -m 0755 "${SUPPORT_DIR}"
install -o root -g wheel -m 0755 "${SRC_DIR}/healthcheck.sh" "${SUPPORT_DIR}/healthcheck.sh"
install -o root -g wheel -m 0755 "${SRC_DIR}/authdb-edit"    "${SUPPORT_DIR}/authdb-edit"
install -o root -g wheel -m 0644 "${SRC_DIR}/${DAEMON_LABEL}.plist" "${DAEMON_PLIST}"
# Reload cleanly: bootout an old instance (ignore "not loaded"), then bootstrap.
launchctl bootout "system/${DAEMON_LABEL}" 2>/dev/null || true
launchctl bootstrap system "${DAEMON_PLIST}" \
    || echo "    NOTE: could not bootstrap the daemon; it will start on next boot." >&2
# Run it once now so a fresh install is verified immediately, not on next boot.
launchctl kickstart "system/${DAEMON_LABEL}" 2>/dev/null || true

echo "Done. New '${RIGHT}':"
security authorizationdb read "${RIGHT}" 2>/dev/null | plutil -extract rule xml1 -o - -
echo
echo "Log:    /tmp/repose-plugin.log"
[[ "${MODE}" == "permit" ]] && cat <<'EOF'
Permit: the permit must be ROOT-OWNED and FRESH in a root-only directory now.
        To allow (as root):
          mkdir -p /var/run/repose-spike && touch /var/run/repose-spike/permit
        Re-touch it within 15s to keep it fresh; rm it to force the password path.
        A non-root or stale permit is ignored -- that is the point.
EOF
echo "Lock the screen to test. If locked out, roll back the VM snapshot."
