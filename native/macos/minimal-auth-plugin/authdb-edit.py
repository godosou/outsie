#!/usr/bin/env python3
"""Edit a macOS authorization rule plist, with the safety checks that matter.

This is the only code in the spike that changes how a Mac decides whether to
unlock. It lived inline in install.sh and uninstall.sh as two separate copies,
which is how the two drifted apart once already. One implementation, one set of
tests, both scripts call it.

Every transform refuses rather than writes when the result would leave the
machine without a way in. A refusal is a non-zero exit and an explanation on
stderr; the caller is expected to abort.

    authdb-edit.py add-subrule    <plist> <subrule>
    authdb-edit.py remove-subrule <plist> <subrule>
    authdb-edit.py validate       <plist>

The plist is edited in place and is expected to be the output of
`security authorizationdb read system.login.screensaver`.
"""

import plistlib
import sys

# Present in the stock rule, and the entry that produces the ordinary password
# prompt. If a transform would remove the last of these, there is no way back in.
PASSWORD_FALLBACKS = (
    "use-login-window-ui",
    "authenticate-session-owner-or-admin",
    "authenticate-session-owner",
    "authenticate-session-user",
)


def die(message):
    sys.stderr.write("authdb-edit: %s\n" % message)
    raise SystemExit(1)


def load(path):
    try:
        with open(path, "rb") as fh:
            data = plistlib.load(fh)
    except Exception as exc:  # noqa: BLE001 - any parse failure is fatal here
        die("cannot read %s: %s" % (path, exc))
    if not isinstance(data, dict):
        die("%s is not a dictionary; refusing to guess at its shape" % path)
    if data.get("class") != "rule":
        die("expected class=rule, found class=%r. This right does not have the "
            "shape this spike knows how to edit; inspect it by hand." % data.get("class"))
    rule = data.get("rule")
    if not isinstance(rule, list):
        die("expected a 'rule' array, found %r" % type(rule).__name__)
    return data, rule


def save(path, data):
    with open(path, "wb") as fh:
        plistlib.dump(data, fh)


def has_fallback(entries, ours):
    """True if some entry other than ours can still authenticate a user."""
    others = [e for e in entries if e != ours]
    if not others:
        return False
    # A known password path is the strong case. Any other third-party entry is
    # accepted too: it was already there before us and removing our own entry
    # must not be blocked just because we do not recognise the neighbours.
    return True


def add_subrule(path, subrule):
    data, rule = load(path)

    # Drop any stale copy first so repeated installs cannot stack duplicates.
    without_ours = [entry for entry in rule if entry != subrule]
    if not without_ours:
        die("adding %s would make it the only entry in the rule. The spike must "
            "never be the sole way to unlock; leave the existing password path "
            "in place." % subrule)
    if not any(entry in PASSWORD_FALLBACKS for entry in without_ours):
        die("the rule has no recognised password fallback (%s). Refusing to add "
            "%s, because a failure of the spike would leave no way in."
            % (", ".join(PASSWORD_FALLBACKS), subrule))

    # k-of-n=1 means any single sub-rule succeeding is enough, which is what
    # lets the spike run first and the password path still work when it denies.
    # Rather than set it and try to restore it on uninstall -- an asymmetry that
    # would leave a machine permanently weakened if the surgical removal path
    # ever ran -- refuse to touch a rule that is not already 1. The stock macOS
    # screensaver rule is 1, so the ordinary case needs no change at all.
    existing = data.get("k-of-n")
    if existing != 1:
        die("this rule has k-of-n=%r, not 1. Setting it to 1 would permanently "
            "weaken the rule from 'all sub-rules must pass' to 'any one may', "
            "and uninstall could not reliably put it back. Inspect it by hand:\n"
            "  security authorizationdb read system.login.screensaver" % existing)

    data["rule"] = [subrule] + without_ours
    save(path, data)
    print("added %s; rule is now %s (k-of-n left at 1)" % (subrule, data["rule"]))


def remove_subrule(path, subrule):
    data, rule = load(path)

    if subrule not in rule:
        print("%s is not present; nothing to remove" % subrule)
        return

    after = [entry for entry in rule if entry != subrule]
    if not after:
        die("removing %s would leave an empty rule array, which would lock the "
            "machine. Restore from the backup instead." % subrule)
    if not has_fallback(rule, subrule):
        die("removing %s would leave no other entry" % subrule)
    if not any(entry in PASSWORD_FALLBACKS for entry in after):
        die("the result would contain no recognised password fallback (%s). "
            "Restore from the backup instead of writing this."
            % ", ".join(PASSWORD_FALLBACKS))

    data["rule"] = after
    save(path, data)
    print("removed %s; rule is now %s" % (subrule, after))


def validate(path):
    """Check a saved rule is safe to write back.

    A backup is only worth restoring if it still describes a usable rule. An
    empty or truncated file is worse than no backup at all: the restore path
    would feed it straight to `security authorizationdb write` and replace a
    working screensaver rule with nothing.
    """
    data, rule = load(path)
    if not rule:
        die("%s has an empty rule array" % path)
    if not any(entry in PASSWORD_FALLBACKS for entry in rule):
        die("%s has no recognised password fallback (%s); writing it back could "
            "leave no way to unlock" % (path, ", ".join(PASSWORD_FALLBACKS)))
    print("%s looks restorable: %s" % (path, rule))


def main(argv):
    if len(argv) == 3 and argv[1] == "validate":
        validate(argv[2])
        return 0
    if len(argv) != 4:
        sys.stderr.write(__doc__)
        return 2
    command, path, subrule = argv[1], argv[2], argv[3]
    if command == "add-subrule":
        add_subrule(path, subrule)
    elif command == "remove-subrule":
        remove_subrule(path, subrule)
    else:
        die("unknown command %r" % command)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
