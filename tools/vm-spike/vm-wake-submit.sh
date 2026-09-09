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
# Requires the host to have granted Accessibility to the terminal running this.
# Without it osascript is refused and the keystrokes go nowhere, which looks
# exactly like a plugin that was never loaded.

set -uo pipefail

APP="${REPOSE_VM_APP:-tart}"

osa() { osascript -e "$1" >/dev/null 2>&1; }

# Focus first. Three separate experiments were invalidated by keystrokes landing
# in whatever else happened to be frontmost, with the guest's HID idle time
# climbing untouched the whole time.
osa "tell application \"System Events\" to tell process \"${APP}\" to set frontmost to true" \
  || { echo "vm-wake-submit: cannot focus ${APP}; is Accessibility granted?" >&2; exit 1; }
sleep 0.6

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
