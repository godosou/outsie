#!/bin/bash
# Static invariants for install.sh and uninstall.sh.
#
# These two scripts cannot be executed under test: they require root and they
# write to /Library/Security. So the properties that have actually bitten this
# project are asserted by reading the scripts instead. That is a weaker check
# than running them, and it is stated as such -- but it is not nothing, because
# every invariant below corresponds to a real failure that already happened.
#
# Runs anywhere, changes nothing.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL="${HERE}/../install.sh"
UNINSTALL="${HERE}/../uninstall.sh"
PASS=0
FAIL=0

ok() { printf '  ok   %s\n' "$1"; PASS=$((PASS + 1)); }
no() { printf '  FAIL %s -- %s\n' "$1" "${2:-}"; FAIL=$((FAIL + 1)); }

line_of() { grep -n -- "$2" "$1" | head -1 | cut -d: -f1; }

echo "install/uninstall invariants"

# 1. The cdhash must be read from the INSTALLED bundle, after the copy.
#    It is only a record of which binary is in place -- authd overwrites any
#    requirement you write with the csreq of whatever process wrote the rule, so
#    a cdhash there pins nothing. Reading it before the copy would still make
#    that record a lie, which is worth preventing: an install log that names a
#    binary other than the installed one is exactly the kind of evidence that
#    sends an investigation down the wrong path for a day.
# The bundle is staged then moved, so "in place" is the mv, not the cp.
place_line="$(line_of "$INSTALL" 'mv "${STAGED}" "${DEST_BUNDLE}"')"
hash_line="$(line_of "$INSTALL" 'CDHASH=')"
if [ -n "$place_line" ] && [ -n "$hash_line" ]; then
  [ "$hash_line" -gt "$place_line" ] \
    && ok "cdhash is read after the bundle is in place" \
    || no "cdhash is read after the bundle is in place" \
          "moved into place at line ${place_line}, cdhash at ${hash_line}"
else
  no "cdhash is read after the bundle is in place" "could not locate both lines"
fi

# Staging must happen before the live bundle is removed, so a failed copy cannot
# leave the authorization rule pointing at nothing.
stage_line="$(line_of "$INSTALL" 'cp -R "${BUILT_BUNDLE}" "${STAGED}"')"
remove_line="$(line_of "$INSTALL" 'rm -rf "${DEST_BUNDLE}"')"
if [ -n "$stage_line" ] && [ -n "$remove_line" ]; then
  [ "$stage_line" -lt "$remove_line" ] \
    && ok "the new bundle is staged before the old one is removed" \
    || no "the new bundle is staged before the old one is removed" \
          "staged at ${stage_line}, removed at ${remove_line}"
else
  no "the new bundle is staged before the old one is removed" "lines not found"
fi

grep -q 'CDHASH=.*codesign -dvvv "${DEST_BUNDLE}"' "$INSTALL" \
  && ok "cdhash is read from DEST_BUNDLE, not the build directory" \
  || no "cdhash is read from DEST_BUNDLE, not the build directory" \
        "$(grep -n 'CDHASH=' "$INSTALL" | head -1)"

grep -q 'BUILT_BUNDLE' <<< "$(grep 'CDHASH=' "$INSTALL")" \
  && no "cdhash never comes from BUILT_BUNDLE" "it does" \
  || ok "cdhash never comes from BUILT_BUNDLE"

# 2. An empty cdhash must abort rather than write a requirement that matches
#    nothing, or worse, matches anything.
grep -q '\[\[ -n "${CDHASH}" \]\]' "$INSTALL" \
  && ok "aborts when the cdhash cannot be read" \
  || no "aborts when the cdhash cannot be read" "no non-empty check found"

# 3. Both scripts must agree on where the backup lives. They did not, once, and
#    the result was an uninstall that silently never restored the rule.
inst_backup="$(grep -o '/var/db/repose-spike' "$INSTALL" | head -1)"
uninst_backup="$(grep -o '/var/db/repose-spike' "$UNINSTALL" | head -1)"
[ -n "$inst_backup" ] && [ "$inst_backup" = "$uninst_backup" ] \
  && ok "install and uninstall agree on the backup directory" \
  || no "install and uninstall agree on the backup directory" \
        "install='${inst_backup}' uninstall='${uninst_backup}'"

# 4. The backup must not live in /tmp, which is cleared on reboot -- and a
#    rollback is most often wanted after a reboot.
grep -q 'BACKUP="/tmp' "$INSTALL" \
  && no "the backup does not live in /tmp" "it does" \
  || ok "the backup does not live in /tmp"

# 5. Both scripts must delegate the rule edit to the tested implementation
#    rather than carrying their own copy.
for f in "$INSTALL" "$UNINSTALL"; do
  name="$(basename "$f")"
  grep -q 'authdb-edit.py' "$f" \
    && ok "${name} delegates the rule edit to authdb-edit.py" \
    || no "${name} delegates the rule edit to authdb-edit.py" "not referenced"
  grep -q 'plistlib' "$f" \
    && no "${name} carries no inline plist editing" "plistlib still present" \
    || ok "${name} carries no inline plist editing"
done

# 6. Both must refuse to run without root, and must confirm before changing
#    anything.
for f in "$INSTALL" "$UNINSTALL"; do
  name="$(basename "$f")"
  grep -qE 'EUID -eq 0|id -u.*-ne 0' "$f" \
    && ok "${name} requires root" || no "${name} requires root" "no check"
  grep -qE 'read -r -p|read -r reply' "$f" \
    && ok "${name} asks before changing anything" \
    || no "${name} asks before changing anything" "no prompt"
  # The prompt may be skipped for scripted runs, but only when something
  # explicitly asks for that. A bypass that defaults to on is not a bypass, it
  # is a missing prompt.
  if grep -q 'ASSUME_YES' "$f"; then
    grep -q 'ASSUME_YES:-}" != "1"' "$f" \
      && ok "${name} only skips the prompt when ASSUME_YES is explicitly 1" \
      || no "${name} only skips the prompt when ASSUME_YES is explicitly 1" \
            "the bypass does not require an explicit opt-in"
  fi
done

# 7. Uninstall must not delete the named right while the screensaver rule still
#    points at it. That ordering is what leaves a dangling reference behind.
ref_check="$(line_of "$UNINSTALL" 'still references')"
remove_line="$(line_of "$UNINSTALL" 'authorizationdb remove')"
if [ -n "$ref_check" ] && [ -n "$remove_line" ]; then
  [ "$ref_check" -lt "$remove_line" ] \
    && ok "uninstall checks for references before removing the right" \
    || no "uninstall checks for references before removing the right" \
          "check at ${ref_check}, remove at ${remove_line}"
else
  no "uninstall checks for references before removing the right" "lines not found"
fi

echo
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
