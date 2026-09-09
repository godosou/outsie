# Repose minimal Authorization Plugin — a spike

This is a **one-shot experiment**, not a product. Its only job is to answer one
question for Step 2 of `docs/plans/2026-09-08-phone-unlock-tdd-restart.md`:

> Does macOS actually let a third-party plugin take part in screensaver unlock —
> will `authorizationhost` load an ad-hoc-signed plugin and call it, and can that
> plugin allow/deny the unlock without user input?

There is no cryptography, no BLE, no state machine, no IPC, no launchd. There is
also, deliberately, **no** `GateClosed` / `fail-closed` / `Disabled` / "pending
task" abstraction. The whole value of the spike is that it *runs*. If it doesn't
run, that is the finding.

## Run it ONLY on a disposable test VM

- The install rewrites `system.login.screensaver`. Get it wrong and you are
  locked out of that account.
- Use an Apple Silicon macOS VM (Tart / UTM) with **SIP off**, take a snapshot
  first, and roll back if anything goes sideways.
- **Do not run `install.sh` / `uninstall.sh` on a daily-driver Mac.** On a
  SIP-on machine `security authorizationdb write` and writing to
  `/Library/Security/SecurityAgentPlugins/` are the parts that will fight you.

## Two milestones, one binary

Both mechanisms live in `ReposeSpike.bundle`; `install.sh` picks which one runs.

- **Milestone A — "does it even load"** (`install.sh log`): `MechanismInvoke`
  appends a timestamped line to `/tmp/repose-plugin.log` and returns
  `kAuthorizationResultAllow`. If that log line appears after you lock the
  screen, `authorizationhost` loaded and called the plugin. That is the answer
  we came for.
- **Milestone B — file trigger** (`install.sh permit`, the default):
  `MechanismInvoke` polls `/tmp/repose-permit` for briefly (1.5s). Present →
  `Allow` (unlock without the password). Absent briefly (1.5s) → `Deny`, and the
  system falls back to the normal password field. Every decision is logged.

## Build (safe on any Mac)

```sh
make            # compiles the universal .bundle and ad-hoc signs it (codesign -s -)
make verify     # codesign --verify, signature type, arch, exported symbol
```

Build and signing are the only steps done on the dev machine. `make` produces
`build/ReposeSpike.bundle` with an ad-hoc signature (`Signature=adhoc`, no
Developer ID — see the plan on why Developer ID is a false gate).

## Install / test / uninstall (test VM only)

```sh
sudo ./install.sh log        # milestone A: always allow, just prove loading
# ...lock screen, watch /tmp/repose-plugin.log...
sudo ./uninstall.sh

sudo ./install.sh            # milestone B: file-triggered (permit)
touch /tmp/repose-permit     # lock screen -> unlocks within ~1s
rm /tmp/repose-permit        # lock screen -> 1.5s wait -> password field
sudo ./uninstall.sh
```

### How it wires in (and how it backs out)

`install.sh` mirrors the pattern that is already demonstrably accepted by
`authorizationhost`: it creates a `evaluate-mechanisms` right
`ai.repose.spike` holding the single mechanism, then prepends that right to
`system.login.screensaver` and sets `k-of-n=1` so the spike runs first while the
normal authentication path stays as a fallback. The full original
`system.login.screensaver` is saved to
`/var/db/repose-spike/system.login.screensaver.backup.plist` before any change.

`uninstall.sh` restores that backup verbatim, removes the `ai.repose.spike`
right, and deletes the bundle. Both scripts print exactly what they will change
and wait for a `y` confirmation before touching anything.

## What "done" looks like

The plan's acceptance test (`tests/e2e/unlock_acceptance.sh`) goes red→green on
the VM under milestone B: phone leaves (permit removed) → Mac stays locked;
phone returns (permit created) → Mac unlocks within 3s; and an untriggered lock
still takes a password normally. When that happens, the biggest unknown in the
whole phone-unlock effort is dead.
