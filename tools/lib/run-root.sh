# Run one command as root, from anywhere -- sourced, not executed.
#
# `sudo` needs a terminal to ask for a password. Plenty of the places this
# project runs do not have one: an agent shell, a script launched from the GUI,
# a double-clicked tool. They all fail identically, with a message about askpass
# that reads like a bug in the script rather than a missing terminal.
#
# So when no credential is cached, ask the way a Mac app asks: the native
# authorization dialog. That is also the exact prompt the real installer gets
# when the app runs it, so the development path and the product path ask the
# user for the same thing. The password goes to the system; it never passes
# through this file.
#
#   . "$(git rev-parse --show-toplevel)/tools/lib/run-root.sh"
#   run_root "rm -f /var/db/repose-unlock/presence-key.1"
#
# Keep each call to ONE command. Every call may cost a dialog, and a tool that
# asks five times in a row teaches people to click through without reading --
# which is the opposite of what an authorization prompt is for.

run_root() {
  if sudo -n true 2>/dev/null; then
    sudo /bin/sh -c "$1"
  else
    osascript -e "do shell script \"$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')\" \
with administrator privileges" >/dev/null
  fi
}

# True when root can be had without a dialog. Use it to decide whether to prompt
# at a convenient moment rather than in the middle of a timed measurement.
run_root_is_cached() { sudo -n true 2>/dev/null; }
