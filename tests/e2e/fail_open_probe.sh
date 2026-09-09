#!/bin/bash
# Regression probe for the fail-open discovered on 2026-09-09.
#
# THE SAFETY REQUIREMENT this encodes:
#   With system.login.screensaver's rule referencing our mechanism, removing
#   the mechanism's bundle must NOT make the screensaver right grantable
#   without a password. If it does, a machine whose plugin went missing
#   (dragged to Trash, half-finished upgrade, OS migration) becomes
#   unlockable by anyone.
#
# WHY THIS PROBE USES THE COMMAND LINE
# ------------------------------------
# The vulnerability shows up identically in two places:
#   - the real lock screen (loginwindow/SecurityAgent), which needs GUI
#     injection and steals focus, and
#   - `security authorize system.login.screensaver`, a plain authorization
#     query that needs no GUI at all.
# Both were confirmed to move together on 2026-09-09: bundle present -> NO
# (-60007), bundle absent -> YES (0), with no credential supplied, for both an
# admin and a root caller. So this probe drives the CLI form. It is a proxy for
# the lock screen, not the lock screen itself; the docs record the GUI-level
# A/B that proves they agree (docs/validation/2026-09-09-e3-fail-open.md).
#
# It runs entirely over ssh against the target and never touches the host's
# focus, so it is safe to run while someone is using the driving Mac and safe
# to put in CI.
#
# EXIT STATUS
#   0  SAFE     : bundle absent still denies (fail-closed). The requirement holds.
#   1  VULN     : bundle absent grants without a password (fail-open). Blocker.
#   2  VOID     : could not establish the conditions (ssh, sudo, or the rule was
#                 not in the expected shape). Neither pass nor fail.
#
# It restores the bundle and permit to their prior state on the way out.

set -uo pipefail

SSH="${REPOSE_SSH:-}"
RIGHT="${REPOSE_RIGHT:-system.login.screensaver}"
BUNDLE="${REPOSE_BUNDLE_PATH:-/Library/Security/SecurityAgentPlugins/ReposeSpike.bundle}"
PERMIT="${REPOSE_PERMIT_PATH:-/tmp/repose-permit}"

[ -n "$SSH" ] || { echo "fail_open_probe: REPOSE_SSH is not set (source tools/vm-spike/vm-env.sh)" >&2; exit 2; }

r() { eval "$SSH \"\$1\""; }

# The right must actually reference our mechanism, or this probe would be
# testing an unrelated rule and its green would mean nothing.
rule_json="$(r "sudo security authorizationdb read ${RIGHT} 2>/dev/null | plutil -extract rule json -o - - 2>/dev/null")"
case "$rule_json" in
  *ai.repose.spike*) : ;;
  *)
    echo "fail_open_probe: ${RIGHT} does not reference ai.repose.spike (rule: ${rule_json:-<unreadable>})." >&2
    echo "  Install the plugin first; there is nothing to probe otherwise." >&2
    exit 2 ;;
esac

# Record prior state so the probe leaves no trace.
bundle_was_present=1; r "test -d '${BUNDLE}'" || bundle_was_present=0
permit_was_present=1; r "test -e '${PERMIT}'" || permit_was_present=0
away="${BUNDLE}.probe-away"

restore() {
  # Put the bundle back if we moved it.
  r "test -d '${away}'" && r "sudo mv '${away}' '${BUNDLE}'"
  # Restore the permit to however we found it.
  if [ "$permit_was_present" = "1" ]; then r "touch '${PERMIT}'"; else r "rm -f '${PERMIT}'"; fi
}
trap restore EXIT

[ "$bundle_was_present" = "1" ] || { echo "fail_open_probe: bundle not installed at ${BUNDLE}; nothing to remove." >&2; exit 2; }

# Query the right with NO credential. A password-less grant is the whole signal:
# if the right is granted here, it was granted without anyone proving anything.
# perl provides the timeout (`timeout(1)` is absent on stock macOS).
query() {
  r "security authorize -d '${RIGHT}' >/dev/null 2>&1; \
     perl -e 'alarm 15; exec q{security}, q{authorize}, q{${RIGHT}}' </dev/null >/dev/null 2>&1; echo \$?"
}

# Permit absent throughout: we are testing the password fallback, not the phone.
r "rm -f '${PERMIT}'"

# Baseline: bundle present must DENY (exit non-zero). If this grants, the rule is
# already broken independently of the fail-open we are probing.
base_rc="$(query)"
if [ "$base_rc" = "0" ]; then
  echo "fail_open_probe: VOID -- with the bundle present and no permit, the right was" >&2
  echo "  granted without a password (exit 0). The rule is not denying as expected;" >&2
  echo "  fix that before this probe can mean anything." >&2
  exit 2
fi

# The experiment: remove the bundle, ask again with no credential.
r "sudo mv '${BUNDLE}' '${away}'"
r "test -d '${BUNDLE}'" && { echo "fail_open_probe: VOID -- could not remove the bundle (sudo?)." >&2; exit 2; }
gone_rc="$(query)"

echo "right            : ${RIGHT}"
echo "bundle present   : authorize exit ${base_rc} (non-zero = denied, good)"
echo "bundle removed   : authorize exit ${gone_rc}"

if [ "$gone_rc" = "0" ]; then
  echo ""
  echo "VULN: with the bundle removed, ${RIGHT} was granted WITHOUT a password."
  echo "  A machine whose plugin goes missing (manual delete, failed upgrade, OS"
  echo "  migration) can be unlocked by anyone. This is fail-open and a release"
  echo "  blocker. See docs/validation/2026-09-09-e3-fail-open.md."
  exit 1
fi

echo ""
echo "SAFE: bundle removed still denies (fail-closed). The safety requirement holds."
exit 0
