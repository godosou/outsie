#!/bin/bash
# Wake the guest's lock screen and submit an empty password.
#
# This is the acceptance test's REPOSE_WAKE_CMD. It has to be a GUI action
# rather than an ssh command, because of what A1 measured:
#
#   - The mechanism is not invoked when the machine locks.
#   - It is not invoked when the password field appears either. Waking alone
#     leaves it un-run; fifteen seconds of waiting produced an empty log.
#   - It IS invoked when an unlock is submitted, even with an empty field.
#
# So "the phone returned, now let me in" is expressed as: dismiss the
# screensaver, clear whatever is in the field, press Return. That is also the
# real product gesture -- wake the Mac and press Return, with the phone standing
# in for the password.
#
# WHY IT CHECKS THAT THE INPUT ARRIVED
# ------------------------------------
# `set frontmost to true` can report success and change nothing. It has done so
# repeatedly here: the terminal driving the test stays frontmost, the keystrokes
# land in it instead of the VM, and the guest never sees an unlock attempt. The
# acceptance test then reports "Mac did NOT unlock", which is a statement about
# the feature -- and it is false. The real event was that nobody knocked.
#
# Three consecutive runs were lost to exactly this before the check existed, and
# the give-away was only visible by asking the guest how long since it last saw
# input. So this script now asks, before and after, and fails loudly rather than
# letting a delivery failure be reported as a product failure.

set -uo pipefail

APP="${REPOSE_VM_APP:-tart}"
SSH="${REPOSE_SSH:-}"

osa() { osascript -e "$1" >/dev/null 2>&1; }

# Seconds since the guest last saw any HID input. Empty if unreadable.
guest_idle() {
  [ -n "$SSH" ] || return 0
  eval "${SSH} 'ioreg -c IOHIDSystem | grep -m1 HIDIdleTime | sed \"s/.*= //\" | awk \"{printf \\\"%.0f\\\", \\\$1/1000000000}\"'" 2>/dev/null
}

# The host's own lock state comes first. A locked host has no GUI session to
# inject into, so activation fails, window enumeration returns nothing, and
# clicks take no focus -- all of which look like deep macOS restrictions and are
# not. Several rounds of diagnosis went into that mistake; this check is one
# line and ends it.
host_locked="$(ioreg -n Root -d1 -a 2>/dev/null | plutil -extract IOConsoleLocked raw -o - - 2>/dev/null)"
if [ "$host_locked" = "true" ]; then
  echo "vm-wake-submit: this Mac is locked, so there is no GUI session to type into." >&2
  echo "  Nothing sent from here can reach the VM. Unlock this Mac and rerun." >&2
  exit 1
fi

before="$(guest_idle)"

osa "tell application \"System Events\" to tell process \"${APP}\" to set frontmost to true"
sleep 0.6

front="$(osascript -e 'tell application "System Events" to get name of first process whose frontmost is true' 2>/dev/null)"
if [ -n "$front" ] && [ "$front" != "$APP" ]; then
  echo "vm-wake-submit: could not bring ${APP} to the front (still '${front}')." >&2
  echo "  Keystrokes would land in ${front}, and the acceptance test would read" >&2
  echo "  that as the Mac failing to unlock. Click the VM window once and rerun." >&2
  exit 1
fi

# Dismiss the screensaver so the password field exists.
osa 'tell application "System Events" to key code 49'
sleep 1.5

# Clear the field. A single leftover character silently turns "submit an empty
# password" into "submit a one-character password", which is a different test.
for _ in $(seq 1 24); do
  osa 'tell application "System Events" to key code 51'
done
sleep 0.4

# Return with nothing typed. The mechanism decides; the password path only sees
# this if the mechanism denies.
osa 'tell application "System Events" to key code 36'
sleep 1

# Confirm the guest actually received something. A rising idle time means every
# keystroke above went somewhere else.
after="$(guest_idle)"
if [ -n "$before" ] && [ -n "$after" ]; then
  if [ "$after" -ge "$before" ] 2>/dev/null; then
    echo "vm-wake-submit: the guest saw no input (idle ${before}s -> ${after}s)." >&2
    echo "  The keystrokes did not reach the VM. Whatever the acceptance test" >&2
    echo "  reports after this would be about delivery, not about unlocking." >&2
    exit 1
  fi
fi
