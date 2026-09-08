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
[[ -f "${BACKUP}" ]] || [[ ! -f "${LEGACY_BACKUP}" ]] || BACKUP="${LEGACY_BACKUP}"

[[ $EUID -eq 0 ]] || { echo "Must run as root (sudo $0)." >&2; exit 1; }

cat <<EOF
This will undo the spike install on THIS machine:

  1. Restore '${RIGHT}' from the backup, replacing the whole rule:
       ${BACKUP}
  2. Remove authorization right '${SUBRULE}'.
  3. Remove the bundle:
       ${DEST_BUNDLE}
EOF
if [[ ! -f "${BACKUP}" ]]; then
    echo
    echo "WARNING: backup ${BACKUP} not found -- cannot precisely restore ${RIGHT}."
    echo "Inspect it manually:  security authorizationdb read ${RIGHT}"
fi
read -r -p "Proceed? [y/N] " reply
[[ "${reply}" == "y" || "${reply}" == "Y" ]] || { echo "Aborted."; exit 0; }

if [[ -f "${BACKUP}" ]]; then
    echo "==> Restoring ${RIGHT} from ${BACKUP}"
    security authorizationdb write "${RIGHT}" < "${BACKUP}"
    rm -f "${BACKUP}"
else
    # No backup: surgically drop our entry instead of leaving the rule pointing
    # at a right we are about to delete. A dangling reference is how a machine
    # ends up with a screensaver rule naming a rule that no longer exists.
    echo "==> No backup; removing '${SUBRULE}' from ${RIGHT} surgically"
    CLEANED="/tmp/${RIGHT}.uninstall.plist"
    security authorizationdb read "${RIGHT}" > "${CLEANED}" 2>/dev/null
    "$(dirname "$0")/authdb-edit.py" remove-subrule "${CLEANED}" "${SUBRULE}" \
        || { echo "Refusing to write; restore from a backup instead." >&2; exit 1; }
    security authorizationdb write "${RIGHT}" < "${CLEANED}"
    rm -f "${CLEANED}"
fi

# Only safe once nothing references it.
if security authorizationdb read "${RIGHT}" 2>/dev/null | grep -q "${SUBRULE}"; then
    echo "ERROR: ${RIGHT} still references ${SUBRULE}; leaving the right in place" >&2
    echo "Inspect: security authorizationdb read ${RIGHT}" >&2
    exit 1
fi
echo "==> Removing right ${SUBRULE}"
security authorizationdb remove "${SUBRULE}" 2>/dev/null || true

if [[ -d "${DEST_BUNDLE}" ]]; then
    echo "==> Removing ${DEST_BUNDLE}"
    rm -rf "${DEST_BUNDLE}"
fi

echo "Done. Current '${RIGHT}':"
security authorizationdb read "${RIGHT}" 2>/dev/null | plutil -extract rule xml1 -o - - || true
