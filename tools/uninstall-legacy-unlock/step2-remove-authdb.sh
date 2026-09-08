#!/bin/bash
# Step 2 of 2: remove the ai.repose.unlock entries from the authorization
# database and delete the installed ReposeUnlock plugin bundle.
#
# Run step1-remove-daemon.sh first and confirm the machine still locks and
# unlocks normally before running this.
#
# WHAT THIS TOUCHES
# -----------------
#   1. system.login.screensaver : removes ONLY the "ai.repose.unlock" entry
#                                 from the rule array. Everything else is left
#                                 byte-for-byte alone, including
#                                 com.openai.sky.CUAService.AuthorizationPlugin.remote,
#                                 which belongs to a different tool.
#   2. ai.repose.unlock         : the named rule itself is removed.
#   3. /Library/Security/SecurityAgentPlugins/ReposeUnlock.bundle : deleted.
#
# This is surgical removal, not a "restore to stock" -- the stock rule for this
# machine is not known, and guessing at it is how people lock themselves out.
#
# SAFETY
# ------
# The script refuses to write unless the resulting rule array still contains
# `use-login-window-ui`, which is the entry that produces the normal password
# prompt. It backs up the current rule to a location that survives reboot.
#
# Usage:  sudo bash step2-remove-authdb.sh
#         sudo bash step2-remove-authdb.sh --yes

set -uo pipefail

RIGHT="system.login.screensaver"
SUBRULE="ai.repose.unlock"
BUNDLE="/Library/Security/SecurityAgentPlugins/ReposeUnlock.bundle"
BACKUP_DIR="/var/db/repose-unlock-cleanup"
BACKUP="${BACKUP_DIR}/${RIGHT}.before-cleanup.plist"
NEW="/tmp/${RIGHT}.cleaned.plist"

if [ "$(id -u)" -ne 0 ]; then
  echo "This script must run as root: sudo bash $0" >&2
  exit 1
fi

echo "=== current ${RIGHT} ==="
security authorizationdb read "$RIGHT" 2>/dev/null | plutil -extract rule xml1 -o - - \
  || { echo "Could not read ${RIGHT}. Aborting." >&2; exit 1; }
echo

if ! security authorizationdb read "$RIGHT" 2>/dev/null | grep -q "$SUBRULE"; then
  echo "'${SUBRULE}' is not referenced by ${RIGHT}; nothing to unwire."
  echo "Checking for leftovers anyway."
else
  mkdir -p "$BACKUP_DIR"; chmod 700 "$BACKUP_DIR"
  if [ -s "$BACKUP" ]; then
    echo "Keeping existing backup: ${BACKUP}"
  else
    security authorizationdb read "$RIGHT" > "$BACKUP" 2>/dev/null
    chmod 600 "$BACKUP"
    echo "Backed up current rule to: ${BACKUP}"
  fi

  # Build the cleaned rule and assert a password path survives BEFORE writing.
  security authorizationdb read "$RIGHT" > "$NEW" 2>/dev/null
  python3 - "$NEW" "$SUBRULE" <<'PY' || exit 1
import plistlib, sys
path, sub = sys.argv[1], sys.argv[2]
with open(path, 'rb') as fh:
    d = plistlib.load(fh)
before = list(d.get('rule', []))
after = [r for r in before if r != sub]
if not after:
    sys.exit("REFUSING: removing %s would leave an empty rule array" % sub)
if 'use-login-window-ui' not in after:
    sys.exit("REFUSING: result has no 'use-login-window-ui'; no password path would remain")
d['rule'] = after
with open(path, 'wb') as fh:
    plistlib.dump(d, fh)
print("  before: %s" % before)
print("  after : %s" % after)
PY

  echo
  echo "=== will write the rule shown as 'after' above, then ==="
  echo "  security authorizationdb remove ${SUBRULE}"
  echo "  rm -rf ${BUNDLE}"
  echo
  echo "Keep this terminal open until you have confirmed you can still unlock."
  echo "Rollback:  sudo security authorizationdb write ${RIGHT} < ${BACKUP}"
  echo

  if [ "${1:-}" != "--yes" ]; then
    printf 'Proceed? [y/N] '
    read -r reply
    case "$reply" in y | Y) ;; *) echo "Aborted. Nothing was changed."; exit 0 ;; esac
  fi

  echo "-> writing cleaned ${RIGHT}"
  if ! security authorizationdb write "$RIGHT" < "$NEW"; then
    echo "WRITE FAILED. The rule is unchanged. Backup at ${BACKUP}" >&2
    exit 1
  fi

  # Verify the write landed as intended before going any further.
  if security authorizationdb read "$RIGHT" 2>/dev/null | grep -q "$SUBRULE"; then
    echo "VERIFY FAILED: ${SUBRULE} is still referenced. Restoring backup." >&2
    security authorizationdb write "$RIGHT" < "$BACKUP"
    exit 1
  fi
  if ! security authorizationdb read "$RIGHT" 2>/dev/null | grep -q "use-login-window-ui"; then
    echo "VERIFY FAILED: no password path in the result. Restoring backup." >&2
    security authorizationdb write "$RIGHT" < "$BACKUP"
    exit 1
  fi
  echo "   verified: ${SUBRULE} unwired, use-login-window-ui present"
fi

# Only now is the named rule unreferenced and safe to delete.
if security authorizationdb read "$SUBRULE" >/dev/null 2>&1; then
  echo "-> security authorizationdb remove ${SUBRULE}"
  security authorizationdb remove "$SUBRULE" || echo "   (remove reported an error; continuing)"
else
  echo "-> right ${SUBRULE} already absent"
fi

if [ -d "$BUNDLE" ]; then
  echo "-> rm -rf ${BUNDLE}"
  rm -rf "$BUNDLE"
else
  echo "-> bundle already absent"
fi

echo
echo "=== final state ==="
echo "${RIGHT} rule array:"
security authorizationdb read "$RIGHT" 2>/dev/null | plutil -extract rule xml1 -o - -
echo "SecurityAgentPlugins:"
ls -d /Library/Security/SecurityAgentPlugins/*.bundle 2>/dev/null || echo "  (none)"
echo
echo "Now lock the screen and confirm your password still works, with this"
echo "terminal still open. If anything is wrong:"
echo "  sudo security authorizationdb write ${RIGHT} < ${BACKUP}"
