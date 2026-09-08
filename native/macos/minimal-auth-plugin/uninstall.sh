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
BACKUP="/tmp/repose-spike-${RIGHT}.backup.plist"

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
fi

echo "==> Removing right ${SUBRULE}"
security authorizationdb remove "${SUBRULE}" 2>/dev/null || true

if [[ -d "${DEST_BUNDLE}" ]]; then
    echo "==> Removing ${DEST_BUNDLE}"
    rm -rf "${DEST_BUNDLE}"
fi

echo "Done. Current '${RIGHT}':"
security authorizationdb read "${RIGHT}" 2>/dev/null | plutil -extract rule xml1 -o - - || true
