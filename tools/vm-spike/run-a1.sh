#!/bin/bash
# Run the A1 experiment against the throwaway VM, end to end.
#
#   source tools/vm-spike/vm-env.sh
#   tools/vm-spike/run-a1.sh
#
# A1 asks one question: does macOS load and obey an ad-hoc signed third-party
# Authorization Plugin in the screen unlock path? It is scripted rather than
# typed because the answer may be "no", in which case the same experiment gets
# re-run against a different signing configuration -- and three hand-typed runs
# are three subtly different experiments.
#
# WHAT IT DOES TO THE VM
# ----------------------
# Milestone A installs a mechanism that allows unconditionally. While it is
# installed the VM unlocks with NO password. That is the measurement, and it is
# why this only ever runs against a disposable guest. The script uninstalls
# before it exits, including on failure, unless --keep is passed.
#
# Recovery, always: tart delete repose-spike && tart clone repose-spike-clean repose-spike

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PLUGIN_DIR="${REPO}/native/macos/minimal-auth-plugin"
VM="${REPOSE_VM:-repose-spike}"
VM_USER="${REPOSE_VM_USER:-admin}"
REMOTE="/Users/${VM_USER}/spike"
KEEP=0
[ "${1:-}" = "--keep" ] && KEEP=1

RESULTS="${REPO}/docs/validation/$(date +%Y-%m-%d)-macos-plugin-load.md"

die() { printf '\033[31mABORT\033[0m %s\n' "$*" >&2; exit 1; }
step() { printf '\n\033[1m== %s ==\033[0m\n' "$*"; }
note() { printf '   %s\n' "$*"; }

[ -n "${REPOSE_SSH:-}" ] || die "source tools/vm-spike/vm-env.sh first"

sshv() { eval "${REPOSE_SSH} '$1'"; }

# Refuse to run against anything that is not the disposable guest. This script
# installs a mechanism that removes the password requirement; pointing it at a
# real machine would be a serious mistake and must not be one keystroke away.
guard_target() {
  local ip
  ip="$(tart ip "$VM" 2>/dev/null)" || die "cannot resolve ${VM}"
  case "${REPOSE_SSH}" in
    *"${ip}"*) ;;
    *) die "REPOSE_SSH does not point at ${VM} (${ip}). Refusing: this
      experiment removes the password requirement while it runs." ;;
  esac
  note "target confirmed: ${VM} at ${ip}"
}

# ---------------------------------------------------------------- preflight

step "preflight"
guard_target

sshv 'echo ok' >/dev/null 2>&1 || die "ssh to the guest failed; is Remote Login on?"
note "ssh reachable"

sshv 'sudo -n true' >/dev/null 2>&1 \
  || die "the guest has no passwordless sudo; see FIRST-BOOT.md section 4.5"
note "passwordless sudo available"

lock_delay="$(sshv 'sysadminctl -screenLock status 2>&1' | tail -1)"
case "$lock_delay" in
  *immediate*) note "screen lock is immediate" ;;
  *) die "screen lock is not immediate (${lock_delay}).
      A non-zero delay means locking only dims the screen and the session is
      never actually locked, so nothing this script measures would be real.
      Fix in the guest: sysadminctl -screenLock immediate -password <pw>" ;;
esac

state="$("${REPO}/tests/e2e/lockstate.sh" --raw 2>/dev/null)" \
  || die "the lock-state oracle is unreadable against the guest"
[ "$state" = "false" ] || die "the guest is already locked; unlock it and rerun"
note "oracle readable, guest unlocked"

# ------------------------------------------------------------------ staging

step "staging the plugin into the guest"
[ -d "${PLUGIN_DIR}/build/ReposeSpike.bundle" ] || die "run: make -C ${PLUGIN_DIR}"
# Capture rather than pipe, and show what codesign actually said when this
# fails. "not ad-hoc signed" with no evidence sends you looking at the wrong
# thing; the answer is usually in the output nobody printed.
SIGN_OUT="$(codesign -dvvv "${PLUGIN_DIR}/build/ReposeSpike.bundle" 2>&1)"
if ! grep -q "Signature=adhoc" <<< "$SIGN_OUT"; then
  printf '%s\n' "$SIGN_OUT" | sed 's/^/   /'
  die "the built bundle is not ad-hoc signed; that is the variable under test"
fi
note "bundle is ad-hoc signed ($(grep '^CDHash=' <<< "$SIGN_OUT"))"

sshv "rm -rf ${REMOTE} && mkdir -p ${REMOTE}" || die "could not prepare ${REMOTE}"
scp -q -r -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR \
  "$PLUGIN_DIR" "${VM_USER}@$(tart ip "$VM"):${REMOTE}/" || die "scp failed"
note "copied"

# If the plugin misbehaves here, a silent milestone A would be our bug rather
# than macOS's answer. Establish that before installing anything.
sshv "cd ${REMOTE}/minimal-auth-plugin && ./build/plugin_contract_test \
  build/ReposeSpike.bundle/Contents/MacOS/ReposeSpike" >/tmp/a1-contract.txt 2>&1 \
  || { sed 's/^/   /' /tmp/a1-contract.txt; die "the contract test fails inside the guest"; }
note "contract test passes in the guest ($(grep -c '^  ok' /tmp/a1-contract.txt) assertions)"

# `sudo env VAR=1 cmd`, not `VAR=1 sudo cmd`. sudo resets the environment by
# default, so the second form sets the variable for sudo itself and the script
# underneath never sees it -- it then waits on a prompt nobody can answer, and
# the run hangs with no output and no error.
cleanup() {
  [ "$KEEP" = "1" ] && { note "--keep: leaving the plugin installed"; return; }
  step "cleaning up"
  sshv "cd ${REMOTE}/minimal-auth-plugin && sudo env ASSUME_YES=1 ./uninstall.sh" \
    >/tmp/a1-uninstall.txt 2>&1 \
    && note "uninstalled" \
    || note "UNINSTALL FAILED -- roll the VM back to repose-spike-clean"
  sshv 'security authorizationdb read system.login.screensaver 2>/dev/null | grep -c ai.repose.spike' \
    | grep -q '^0$' && note "screensaver rule is clean" || note "WARNING: rule still references the spike"
}
trap cleanup EXIT

# -------------------------------------------------------------- milestone A

step "milestone A: does macOS load and call the plugin"
note "installing the 'log' mechanism -- while installed, this guest unlocks with NO password"
sshv "cd ${REMOTE}/minimal-auth-plugin && sudo env ASSUME_YES=1 ./install.sh log" \
  >/tmp/a1-install-log.txt 2>&1 || { sed 's/^/   /' /tmp/a1-install-log.txt; die "install failed"; }
note "installed"

sshv 'rm -f /tmp/repose-plugin.log'
A_START="$(sshv 'date +%s')"

eval "${REPOSE_LOCK_CMD}" >/dev/null 2>&1 || die "lock command failed"
for _ in $(seq 1 40); do
  [ "$("${REPO}/tests/e2e/lockstate.sh" --raw 2>/dev/null)" = "true" ] && break
  sleep 0.5
done
[ "$("${REPO}/tests/e2e/lockstate.sh" --raw 2>/dev/null)" = "true" ] \
  || die "the guest never locked; nothing to measure"
note "guest locked"

eval "${REPOSE_WAKE_CMD}" >/dev/null 2>&1
note "woke the guest -- this is what starts the authorization evaluation"

UNLOCKED=no
for _ in $(seq 1 20); do
  [ "$("${REPO}/tests/e2e/lockstate.sh" --raw 2>/dev/null)" = "false" ] && { UNLOCKED=yes; break; }
  sleep 0.5
done

FILE_LOG="$(sshv 'cat /tmp/repose-plugin.log 2>/dev/null' || true)"
# grep -c prints 0 and exits 1 when nothing matches, so a bare `|| echo 0`
# appends a second zero and turns the value into two lines, which then fails
# the integer comparison below and is silently treated as "no log".
OS_LOG="$(sshv "log show --start '@${A_START}' --predicate 'subsystem == \"ai.repose.spike\"' --info --style compact 2>/dev/null | grep -c repose || true" | tr -dc '0-9')"
OS_LOG="${OS_LOG:-0}"

note "plugin log lines : $(printf '%s' "$FILE_LOG" | grep -c . || echo 0)"
note "unified log lines: ${OS_LOG}"
note "unlocked without a password: ${UNLOCKED}"

if [ -n "$FILE_LOG" ] || [ "${OS_LOG:-0}" -gt 0 ] 2>/dev/null; then
  if [ "$UNLOCKED" = "yes" ]; then
    VERDICT_A="LOADED AND OBEYED"
  else
    VERDICT_A="LOADED BUT THE ALLOW DID NOT UNLOCK"
  fi
elif [ "$UNLOCKED" = "yes" ]; then
  VERDICT_A="UNLOCKED WITH NO LOG -- suspicious, do not trust without investigation"
else
  VERDICT_A="NOT LOADED (or never invoked)"
fi
printf '\n   \033[1mMilestone A: %s\033[0m\n' "$VERDICT_A"

# -------------------------------------------------------------- milestone B

VERDICT_B="not attempted"
case "$VERDICT_A" in
  "LOADED AND OBEYED")
    step "milestone B: can the mechanism gate on a condition"
    sshv "cd ${REMOTE}/minimal-auth-plugin && sudo env ASSUME_YES=1 ./uninstall.sh" >/dev/null 2>&1
    sshv "cd ${REMOTE}/minimal-auth-plugin && sudo env ASSUME_YES=1 ./install.sh permit" \
      >/tmp/a1-install-permit.txt 2>&1 || die "permit install failed"
    sshv 'rm -f /tmp/repose-permit'
    if REPOSE_E2E_ALLOW_LOCK=1 "${REPO}/tests/e2e/unlock_acceptance.sh" >/tmp/a1-accept.txt 2>&1; then
      VERDICT_B="PASS"
    elif grep -q "did NOT unlock" /tmp/a1-accept.txt; then
      VERDICT_B="FAIL"
    else
      # A harness or wiring problem is not a verdict about the mechanism.
      # Recording it as FAIL would put "cannot gate on a condition" into the
      # results document when the truth is that the test never ran properly.
      VERDICT_B="INCONCLUSIVE (harness, see /tmp/a1-accept.txt)"
    fi
    sed 's/^/   /' /tmp/a1-accept.txt | tail -12
    printf '\n   \033[1mMilestone B: %s\033[0m\n' "$VERDICT_B"
    ;;
  *)
    note "skipping milestone B: it cannot mean anything until A is a yes"
    ;;
esac

# ----------------------------------------------------------------- results

mkdir -p "$(dirname "$RESULTS")"
{
  echo "# A1：macOS 是否加载并听从第三方 ad-hoc 插件"
  echo
  echo "日期：$(date '+%Y-%m-%d %H:%M')"
  echo
  echo "| 项 | 结果 |"
  echo "|---|---|"
  echo "| 里程碑 A | ${VERDICT_A} |"
  echo "| 里程碑 B | ${VERDICT_B} |"
  echo "| 客户机 | $(sshv 'sw_vers -productVersion' 2>/dev/null) $(sshv 'sw_vers -buildVersion' 2>/dev/null) |"
  echo "| SIP | $(sshv 'csrutil status' 2>/dev/null | head -1) |"
  echo "| 签名 | ad-hoc |"
  echo "| 无密码解锁 | ${UNLOCKED} |"
  echo
  echo '## 插件日志'
  echo '```'
  printf '%s\n' "${FILE_LOG:-（空）}"
  echo '```'
} > "$RESULTS"
step "results written"
note "$RESULTS"
