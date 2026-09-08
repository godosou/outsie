#!/bin/bash
# Tests for authdb-edit.py, the only code in the spike that changes how a Mac
# decides whether to unlock.
#
# The cases that matter are the refusals. A transform that writes a rule with no
# way back in does not fail loudly at write time -- it fails the next time
# somebody locks the screen, which may be hours later on a machine nobody can
# get into. Every "refuses" case below is one of those.
#
# Runs anywhere, touches only a scratch directory, never reads or writes the
# real authorization database.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EDIT="${HERE}/../authdb-edit.py"
WORK="$(mktemp -d -t authdb-edit-test)"
trap 'rm -rf "$WORK"' EXIT

SUB="ai.repose.spike"
PASS=0
FAIL=0

ok() { printf '  ok   %s\n' "$1"; PASS=$((PASS + 1)); }
no() { printf '  FAIL %s -- %s\n' "$1" "${2:-}"; FAIL=$((FAIL + 1)); }

# Write a rule plist from a python literal describing the dict.
fixture() {
  local path="$1" body="$2"
  python3 -c "
import plistlib, sys
plistlib.dump(${body}, open('${path}', 'wb'))
"
}

# Print the rule array as a comma-joined string.
rule_of() {
  python3 -c "
import plistlib
d = plistlib.load(open('$1', 'rb'))
print(','.join(d.get('rule', [])))
"
}

kofn_of() {
  python3 -c "
import plistlib
d = plistlib.load(open('$1', 'rb'))
print(d.get('k-of-n', 'unset'))
"
}

STOCK="{'class':'rule','rule':['use-login-window-ui'],'k-of-n':1,'version':1}"
THIRD_PARTY="{'class':'rule','rule':['com.vendor.thing','use-login-window-ui'],'k-of-n':1}"

echo "authdb-edit tests"

# --- add ------------------------------------------------------------------

f="$WORK/stock.plist"; fixture "$f" "$STOCK"
if "$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1; then
  [ "$(rule_of "$f")" = "${SUB},use-login-window-ui" ] \
    && ok "add prepends the sub-rule" \
    || no "add prepends the sub-rule" "got $(rule_of "$f")"
  [ "$(kofn_of "$f")" = "1" ] && ok "add leaves k-of-n at 1" || no "add leaves k-of-n at 1"
else
  no "add prepends the sub-rule" "command failed"
fi

# Installing twice must not stack duplicates: a rule listing the same mechanism
# twice runs it twice and doubles the unlock latency.
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1
[ "$(rule_of "$f")" = "${SUB},use-login-window-ui" ] \
  && ok "add is idempotent" || no "add is idempotent" "got $(rule_of "$f")"

f="$WORK/third.plist"; fixture "$f" "$THIRD_PARTY"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1
[ "$(rule_of "$f")" = "${SUB},com.vendor.thing,use-login-window-ui" ] \
  && ok "add preserves an unrelated third-party entry" \
  || no "add preserves an unrelated third-party entry" "got $(rule_of "$f")"

# A rule with nothing but our own entry, or with no password path, must not be
# written: the spike would become the only way into the machine.
f="$WORK/nofallback.plist"
fixture "$f" "{'class':'rule','rule':['com.vendor.thing'],'k-of-n':1}"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "add refuses when there is no password fallback" "it wrote anyway" \
  || ok "add refuses when there is no password fallback"

f="$WORK/onlyours.plist"
fixture "$f" "{'class':'rule','rule':['${SUB}'],'k-of-n':1}"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "add refuses when our entry would be the only one" "it wrote anyway" \
  || ok "add refuses when our entry would be the only one"

# k-of-n is never modified. Setting it would have to be undone on uninstall, and
# the surgical removal path has no way to know what it used to be -- so a rule
# that is not already 1 is refused rather than quietly weakened from "all
# sub-rules must pass" to "any one may".
f="$WORK/kofn2.plist"
fixture "$f" "{'class':'rule','rule':['a','use-login-window-ui'],'k-of-n':2}"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "add refuses to weaken a rule whose k-of-n is not 1" "it wrote anyway" \
  || ok "add refuses to weaken a rule whose k-of-n is not 1"
[ "$(kofn_of "$f")" = "2" ] \
  && ok "the refused rule keeps its original k-of-n" \
  || no "the refused rule keeps its original k-of-n" "now $(kofn_of "$f")"

f="$WORK/kofnmissing.plist"
fixture "$f" "{'class':'rule','rule':['use-login-window-ui']}"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "add refuses when k-of-n is absent" "it wrote anyway" \
  || ok "add refuses when k-of-n is absent"

# --- remove ---------------------------------------------------------------

f="$WORK/roundtrip.plist"; fixture "$f" "$STOCK"
before="$(rule_of "$f")"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1
"$EDIT" remove-subrule "$f" "$SUB" >/dev/null 2>&1
[ "$(rule_of "$f")" = "$before" ] \
  && ok "add then remove restores the original rule" \
  || no "add then remove restores the original rule" "got $(rule_of "$f")"

f="$WORK/absent.plist"; fixture "$f" "$STOCK"
"$EDIT" remove-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && ok "remove is a no-op when the sub-rule is absent" \
  || no "remove is a no-op when the sub-rule is absent" "non-zero exit"

f="$WORK/lastentry.plist"
fixture "$f" "{'class':'rule','rule':['${SUB}'],'k-of-n':1}"
"$EDIT" remove-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "remove refuses to empty the rule array" "it wrote anyway" \
  || ok "remove refuses to empty the rule array"

f="$WORK/nopass.plist"
fixture "$f" "{'class':'rule','rule':['${SUB}','com.vendor.thing'],'k-of-n':1}"
"$EDIT" remove-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "remove refuses when the result has no password path" "it wrote anyway" \
  || ok "remove refuses when the result has no password path"

# --- malformed input ------------------------------------------------------

f="$WORK/wrongclass.plist"
fixture "$f" "{'class':'evaluate-mechanisms','mechanisms':['x']}"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "refuses a right that is not class=rule" "it wrote anyway" \
  || ok "refuses a right that is not class=rule"

f="$WORK/notarray.plist"
fixture "$f" "{'class':'rule','rule':'authenticate-session-owner-or-admin'}"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "refuses when 'rule' is a string rather than an array" "it wrote anyway" \
  || ok "refuses when 'rule' is a string rather than an array"

f="$WORK/garbage.plist"; printf 'not a plist at all' > "$f"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "refuses an unparseable file" "it wrote anyway" \
  || ok "refuses an unparseable file"

f="$WORK/empty.plist"; : > "$f"
"$EDIT" remove-subrule "$f" "$SUB" >/dev/null 2>&1 \
  && no "refuses an empty file" "it wrote anyway" \
  || ok "refuses an empty file"

# A refusal must leave the input untouched, not half-written.
f="$WORK/untouched.plist"
fixture "$f" "{'class':'rule','rule':['com.vendor.thing'],'k-of-n':1}"
sum_before="$(shasum "$f" | cut -d' ' -f1)"
"$EDIT" add-subrule "$f" "$SUB" >/dev/null 2>&1
[ "$(shasum "$f" | cut -d' ' -f1)" = "$sum_before" ] \
  && ok "a refusal leaves the file byte-identical" \
  || no "a refusal leaves the file byte-identical" "file was modified"

# --- validate ---------------------------------------------------------------
# uninstall calls this before writing a backup back into the authorization
# database. A zero-byte or truncated backup is worse than none at all.

f="$WORK/good.plist"; fixture "$f" "$STOCK"
"$EDIT" validate "$f" >/dev/null 2>&1 \
  && ok "validate accepts a restorable rule" \
  || no "validate accepts a restorable rule" "rejected a good one"

f="$WORK/zero.plist"; : > "$f"
"$EDIT" validate "$f" >/dev/null 2>&1 \
  && no "validate rejects a zero-byte backup" "it accepted one" \
  || ok "validate rejects a zero-byte backup"

f="$WORK/emptyrule.plist"; fixture "$f" "{'class':'rule','rule':[],'k-of-n':1}"
"$EDIT" validate "$f" >/dev/null 2>&1 \
  && no "validate rejects an empty rule array" "it accepted one" \
  || ok "validate rejects an empty rule array"

f="$WORK/nopathback.plist"
fixture "$f" "{'class':'rule','rule':['com.vendor.thing'],'k-of-n':1}"
"$EDIT" validate "$f" >/dev/null 2>&1 \
  && no "validate rejects a rule with no password path" "it accepted one" \
  || ok "validate rejects a rule with no password path"

f="$WORK/truncated.plist"
head -c 40 "$WORK/good.plist" > "$f" 2>/dev/null
"$EDIT" validate "$f" >/dev/null 2>&1 \
  && no "validate rejects a truncated plist" "it accepted one" \
  || ok "validate rejects a truncated plist"

echo
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
