#!/bin/bash
#
# Reverse install.sh: restore system.login.screensaver exactly from the backup,
# remove the spike sub-rule, and delete the bundle.
#
set -euo pipefail

BUNDLE_NAME="ReposeSpike"
DEST_BUNDLE="/Library/Security/SecurityAgentPlugins/${BUNDLE_NAME}.bundle"
RIGHT="system.login.screensaver"
SUBRULE="ai.repose.spike"
# Must match install.sh. These drifted apart once already: install.sh moved its
# backup to /var/db so a rollback survives a reboot, this script kept looking in
# /tmp, and the result was an uninstall that silently never restored the rule.
BACKUP_DIR="/var/db/repose-spike"
BACKUP="${BACKUP_DIR}/${RIGHT}.backup.plist"
LEGACY_BACKUP="/tmp/repose-spike-${RIGHT}.backup.plist"
# Fall back to the old location so a machine installed by the earlier script can
# still be restored precisely.
EDIT="$(dirname "$0")/authdb-edit"

# E12 health-check daemon, installed by install.sh.
SUPPORT_DIR="/Library/Application Support/ReposeSpike"
DAEMON_LABEL="ai.repose.spike.healthcheck"
DAEMON_PLIST="/Library/LaunchDaemons/${DAEMON_LABEL}.plist"

# -s not -f throughout. install.sh creates the backup by redirecting the output
# of `security authorizationdb read`, so an interrupt or a failure between the
# redirect and the read leaves a zero-byte file. Treating that as a usable
# backup would feed nothing to `authorizationdb write` and replace a working
# screensaver rule with an empty one.
backup_usable() {
    [[ -s "$1" ]] && "${EDIT}" validate "$1" >/dev/null 2>&1
}

backup_usable "${BACKUP}" || ! backup_usable "${LEGACY_BACKUP}" \
    || BACKUP="${LEGACY_BACKUP}"

[[ $EUID -eq 0 ]] || { echo "Must run as root (sudo $0)." >&2; exit 1; }

cat <<EOF
This will undo the spike install on THIS machine:

  1. Restore '${RIGHT}' from the backup, replacing the whole rule:
       ${BACKUP}
  2. Remove authorization right '${SUBRULE}'.
  3. Remove the bundle:
       ${DEST_BUNDLE}
EOF
if ! backup_usable "${BACKUP}"; then
    echo
    echo "NOTE: no usable backup at ${BACKUP}"
    echo "(missing, empty, or not a restorable rule). Falling back to removing"
    echo "just our own entry, which keeps the existing password path intact."
fi
# --yes exists so the A1 ladder can be re-run identically three times if the
# first signing configuration does not load. A prompt in the middle of a
# scripted experiment is how runs end up subtly different from each other.
if [[ "${ASSUME_YES:-}" != "1" ]]; then
    read -r -p "Proceed? [y/N] " reply
    [[ "${reply}" == "y" || "${reply}" == "Y" ]] || { echo "Aborted."; exit 0; }
fi

# Stop the health-check daemon FIRST. It watches the plugins directory, so if it
# were still loaded when we remove the bundle below it would race us to rewrite
# the rule. Removing it up front makes the uninstall the only writer.
echo "==> Removing health-check daemon ${DAEMON_LABEL}"
launchctl bootout "system/${DAEMON_LABEL}" 2>/dev/null || true
rm -f "${DAEMON_PLIST}"
rm -rf "${SUPPORT_DIR}"

if backup_usable "${BACKUP}"; then
    echo "==> Restoring ${RIGHT} from ${BACKUP}"
    security authorizationdb write "${RIGHT}" < "${BACKUP}"
    # Keep the backup until the restore is confirmed below. Deleting it here
    # would throw away the only way back if the write did not take.
else
    # No backup: surgically drop our entry instead of leaving the rule pointing
    # at a right we are about to delete. A dangling reference is how a machine
    # ends up with a screensaver rule naming a rule that no longer exists.
    echo "==> No backup; removing '${SUBRULE}' from ${RIGHT} surgically"
    # Transit file goes in the 0700 root-owned directory, never /tmp. This file
    # is piped straight into `security authorizationdb write`, so a predictable
    # world-writable path would let a local user swap in a rule of their choosing
    # between the validation and the write.
    mkdir -p "${BACKUP_DIR}"; chmod 700 "${BACKUP_DIR}"
    CLEANED="${BACKUP_DIR}/${RIGHT}.uninstall.plist"
    security authorizationdb read "${RIGHT}" > "${CLEANED}" 2>/dev/null
    "$(dirname "$0")/authdb-edit" remove-subrule "${CLEANED}" "${SUBRULE}" \
        || { echo "Refusing to write; restore from a backup instead." >&2; exit 1; }
    security authorizationdb write "${RIGHT}" < "${CLEANED}"
    rm -f "${CLEANED}"
fi

# Confirm the rule really is clean before touching anything else. Only now is
# the backup expendable.
if security authorizationdb read "${RIGHT}" 2>/dev/null | grep -q "${SUBRULE}"; then
    echo "ERROR: ${RIGHT} still references ${SUBRULE}; leaving the right in place" >&2
    echo "Inspect: security authorizationdb read ${RIGHT}" >&2
    exit 1
fi
rm -f "${BACKUP}"

echo "==> Removing right ${SUBRULE}"
security authorizationdb remove "${SUBRULE}" 2>/dev/null || true

if [[ -d "${DEST_BUNDLE}" ]]; then
    echo "==> Removing ${DEST_BUNDLE}"
    rm -rf "${DEST_BUNDLE}"
fi

# The permit file is an unlock switch. Leaving it behind after the mechanism
# that reads it is gone is harmless today and a trap the next time something
# reads that path. Clear the hardened path and the old /tmp one.
rm -f /var/run/repose-spike/permit /tmp/repose-permit
echo "Diagnostics left at /tmp/repose-plugin.log (evidence; delete when done)."

echo "Done. Current '${RIGHT}':"
security authorizationdb read "${RIGHT}" 2>/dev/null | plutil -extract rule xml1 -o - - || true
