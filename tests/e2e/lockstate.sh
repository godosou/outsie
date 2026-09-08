#!/bin/bash
# Read the macOS console lock state.
#
# This is the oracle the whole phone-unlock acceptance suite is built on: if we
# cannot observe "is the screen locked right now" from a script, we cannot write
# a failing test for "the Mac unlocked itself", and the feature has no TDD loop.
#
# IOConsoleLocked is published by IOKit on the Root node and is readable without
# any extra runtime (no PyObjC, no Swift, no signing). It reflects the screen
# lock, which is the state the system.login.screensaver authorization path
# guards -- the exact path this feature hooks.
#
# Usage:
#   lockstate.sh            # prints "locked" or "unlocked"
#   lockstate.sh --raw      # prints "true" or "false"
#   lockstate.sh --is-locked  # exit 0 if locked, 1 if unlocked (prints nothing)
#
# Exit codes: 0 success (or "locked" for --is-locked), 1 unlocked for
# --is-locked, 2 the state could not be read at all.

set -euo pipefail

# The machine being locked is not always the machine running the test: the
# walking skeleton locks a throwaway VM while the harness drives it from the
# host. REPOSE_LOCKSTATE_CMD substitutes the read, so one oracle serves both.
# ioreg needs no GUI session and no TCC grant, so it works fine over ssh.
LOCKSTATE_CMD="${REPOSE_LOCKSTATE_CMD:-}"

read_raw() {
  local raw
  if [ -n "$LOCKSTATE_CMD" ]; then
    if ! raw="$(bash -c "$LOCKSTATE_CMD" 2>/dev/null)"; then
      echo "lockstate: REPOSE_LOCKSTATE_CMD failed: ${LOCKSTATE_CMD}" >&2
      return 2
    fi
    raw="$(printf '%s' "$raw" | tr -d '[:space:]')"
  elif ! raw="$(ioreg -n Root -d1 -a 2>/dev/null | plutil -extract IOConsoleLocked raw -o - - 2>/dev/null)"; then
    echo "lockstate: could not read IOConsoleLocked from IOKit" >&2
    return 2
  fi
  case "$raw" in
    true | false) printf '%s\n' "$raw" ;;
    *)
      echo "lockstate: unexpected IOConsoleLocked value: ${raw}" >&2
      return 2
      ;;
  esac
}

main() {
  local raw
  raw="$(read_raw)" || exit 2

  case "${1:-}" in
    --raw)
      printf '%s\n' "$raw"
      ;;
    --is-locked)
      [ "$raw" = "true" ]
      ;;
    "")
      if [ "$raw" = "true" ]; then echo "locked"; else echo "unlocked"; fi
      ;;
    *)
      echo "lockstate: unknown option: $1" >&2
      exit 2
      ;;
  esac
}

main "$@"
