#!/bin/bash
#
# Install the Repose spike Authorization Plugin and wire it into
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
MECHANISM="${BUNDLE_NAME}:${MODE},privileged"

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

cat <<EOF
This will make the following changes to THIS machine:

  1. Copy   ${BUILT_BUNDLE}
       ->   ${DEST_BUNDLE}
  2. Back up the current '${RIGHT}' rule to:
       ${BACKUP}
  3. Create authorization right '${SUBRULE}' (evaluate-mechanisms):
       mechanism  ${MECHANISM}
       requirement pinned to the installed bundle's ad-hoc cdhash
  4. Prepend '${SUBRULE}' to '${RIGHT}'. k-of-n is left exactly as it is; the
     install refuses unless it is already 1, so the spike runs first and the
     normal password path stays as the fallback.

uninstall.sh restores the backup exactly, removes '${SUBRULE}', and deletes
the bundle.
EOF
read -r -p "Proceed? [y/N] " reply
[[ "${reply}" == "y" || "${reply}" == "Y" ]] || { echo "Aborted."; exit 0; }

# Milestone A's entire verdict is "did a line appear in this log". A log left
# over from a previous run would be read as a fresh success, so it is cleared
# here rather than trusted to be empty. Same for a stale permit file, which
# would silently turn the permit mechanism into an unconditional allow.
echo "==> Clearing previous run evidence"
rm -f /tmp/repose-plugin.log /tmp/repose-permit

echo "==> Installing bundle"
rm -rf "${DEST_BUNDLE}"
cp -R "${BUILT_BUNDLE}" "${DEST_BUNDLE}"
chown -R root:wheel "${DEST_BUNDLE}"

CDHASH="$(codesign -dvvv "${DEST_BUNDLE}" 2>&1 | sed -n 's/^CDHash=//p')"
[[ -n "${CDHASH}" ]] || { echo "Could not read installed cdhash." >&2; exit 1; }
echo "    cdhash ${CDHASH}"

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
    "$(dirname "$0")/authdb-edit.py" validate "${TMP_BACKUP}" >/dev/null \
        || { echo "Refusing to install: could not capture a restorable backup." >&2
             rm -f "${TMP_BACKUP}"; exit 1; }
    chmod 600 "${TMP_BACKUP}"
    mv "${TMP_BACKUP}" "${BACKUP}"
fi

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
	<key>requirement</key>
	<string>cdhash H"${CDHASH}"</string>
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
"$(dirname "$0")/authdb-edit.py" add-subrule "${NEW_RULE}" "${SUBRULE}"
security authorizationdb write "${RIGHT}" < "${NEW_RULE}"
rm -f "${NEW_RULE}"

echo "Done. New '${RIGHT}':"
security authorizationdb read "${RIGHT}" 2>/dev/null | plutil -extract rule xml1 -o - -
echo
echo "Log:    /tmp/repose-plugin.log"
[[ "${MODE}" == "permit" ]] && \
    echo "Permit: touch /tmp/repose-permit to allow, rm to force password fallback."
echo "Lock the screen to test. If locked out, roll back the VM snapshot."
