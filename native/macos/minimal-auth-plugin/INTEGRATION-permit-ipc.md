# INTEGRATION — permit IPC daemon: apply-ready patch-specs

Date: 2026-09-09
Branch: `feat/phone-unlock-walking-skeleton`
Design: [../../../docs/plans/2026-09-09-permit-ipc-design.md](../../../docs/plans/2026-09-09-permit-ipc-design.md)

These are **apply-ready patch-specs** for `install.sh` and `uninstall.sh`. They are NOT applied
here — the human applies them. Each hunk is anchored on lines that exist verbatim in the current
scripts so it can be dropped in unambiguously.

What these patches add:

1. Build `repose-permitd` (clang only, no Rust) and install it to
   `/Library/Application Support/ReposeSpike/repose-permitd`.
2. Create the socket directory `/var/run/repose-spike/ipc` with the exact
   `root:_securityagent 0750`.
3. Install the LaunchDaemon plist (`ai.repose.spike.permitd.plist`) `root:wheel 0644` and
   `bootstrap` it (with `bootout` of any old instance first).
4. On uninstall: `bootout` the daemon, remove its plist and socket dir — **before** the rule stops
   referencing the mechanism.

The plist itself already exists at
[`ai.repose.spike.permitd.plist`](ai.repose.spike.permitd.plist) (written per design §7). The
daemon sources, `build.sh`, `install-permitd.sh`/`uninstall-permitd.sh` are the separate new files
of design §6.6; if you prefer to keep `install.sh`/`uninstall.sh` untouched, use those standalone
scripts instead of these patches — the command bodies are identical. The patches below are for the
"fold it into the existing installers" path.

---

## Ordering constraints (why the hunks sit where they do)

- **Daemon UP before the rule references the mechanism.** In `install.sh` the mechanism only goes
  live at the lock screen once `system.login.screensaver` references `ai.repose.spike` (the
  `==> Wiring ${SUBRULE} into ${RIGHT}` step). The daemon-install hunk is inserted **before**
  `==> Creating right ${SUBRULE}`, so the socket is listening before any of that. This is a
  cleanliness/observability ordering, not a safety one: if the daemon is down the mechanism just
  **denies ⇒ password** (fail-closed), so an out-of-order or failed bootstrap can never lock anyone
  out.
- **Daemon DOWN before the rule stops referencing the mechanism.** In `uninstall.sh` the
  `bootout` of `permitd` is placed in the same early block that already boots out the health-check
  daemon, i.e. **before** the rule is restored/cleaned. Same race logic the script already documents
  for the health-check daemon: make the uninstall the only writer, leave nothing listening.
- **`/var/run` is cleared on boot.** The socket dir is re-asserted by the daemon on every launch
  (design §7/§3.1); `install.sh` creating it up front only makes the very first run deterministic.

---

## Patch 1 — `native/macos/minimal-auth-plugin/install.sh`

### Hunk 1a — declare permitd variables

Anchor (the health-check daemon vars, currently near line 62-63):

```bash
DAEMON_LABEL="ai.repose.spike.healthcheck"
DAEMON_PLIST="/Library/LaunchDaemons/${DAEMON_LABEL}.plist"
```

Insert immediately AFTER that anchor:

```bash
# permit IPC consume-daemon (docs/plans/2026-09-09-permit-ipc-design.md).
PERMITD_LABEL="ai.repose.spike.permitd"
PERMITD_PLIST="/Library/LaunchDaemons/${PERMITD_LABEL}.plist"
PERMITD_BIN="${SUPPORT_DIR}/repose-permitd"          # installed, root-owned support dir
PERMITD_SRC_BIN="../permit-daemon/build/repose-permitd"  # built by ../permit-daemon/build.sh
# Socket dir gate: root owns it, gid 92 (_securityagent) is the group, 0750 so the
# directory-traversal gate admits ONLY root + uid 92 (the non-privileged mechanism
# host) and excludes ordinary users (design §3.1/§3.2). One config serves both the
# privileged (uid 0) and non-privileged (uid 92) install variants.
PERMITD_IPC_DIR="/var/run/repose-spike/ipc"
PERMITD_SOCK_GROUP="_securityagent"
```

### Hunk 1b — build + install + bootstrap the daemon (before wiring the mechanism)

Anchor (currently near line 152):

```bash
echo "==> Creating right ${SUBRULE}"
```

Insert immediately BEFORE that anchor:

```bash
echo "==> Building and installing permit daemon ${PERMITD_LABEL}"
# clang-only build (a clean Mac has no guaranteed Rust toolchain, design §1).
# Build if the human hasn't already; refuse to continue without a binary.
if [[ ! -x "${PERMITD_SRC_BIN}" ]]; then
    ( cd ../permit-daemon && ./build.sh ) \
        || { echo "permit-daemon build failed (see ../permit-daemon/build.sh)." >&2; exit 1; }
fi
[[ -x "${PERMITD_SRC_BIN}" ]] \
    || { echo "Missing ${PERMITD_SRC_BIN}; build the daemon first." >&2; exit 1; }
# Root-owned support dir (0755, not writable by anyone but root); the binary is the
# thing launchd runs as root, so it must not be swappable by a non-root user.
install -d -o root -g wheel -m 0755 "${SUPPORT_DIR}"
install -o root -g wheel -m 0755 "${PERMITD_SRC_BIN}" "${PERMITD_BIN}"
install -o root -g wheel -m 0644 "${SRC_DIR}/${PERMITD_LABEL}.plist" "${PERMITD_PLIST}"
# Socket dir root:_securityagent 0750 (design §3.1). The daemon re-asserts this on
# each launch; /var/run is cleared on boot, so create it here for the first run.
install -d -o root -g "${PERMITD_SOCK_GROUP}" -m 0750 "${PERMITD_IPC_DIR}"
# Bring the daemon up BEFORE the rule references the mechanism. Reload cleanly:
# bootout an old instance (ignore "not loaded"), then bootstrap, then kickstart so
# the socket exists now rather than on next boot. If bootstrap fails the mechanism
# just denies => password (fail-closed), so this is never a lockout.
launchctl bootout "system/${PERMITD_LABEL}" 2>/dev/null || true
launchctl bootstrap system "${PERMITD_PLIST}" \
    || echo "    NOTE: could not bootstrap ${PERMITD_LABEL}; it will start on next boot." >&2
launchctl kickstart "system/${PERMITD_LABEL}" 2>/dev/null || true

```

Notes:
- `install.sh` already runs `cd "$(dirname "$0")"` (line 41) and sets `SRC_DIR="$(dirname "$0")"`
  (line 60) and `SUPPORT_DIR="/Library/Application Support/ReposeSpike"` (line 61), so the relative
  paths and both vars resolve. `../permit-daemon` is a sibling of `minimal-auth-plugin`.
- The `install -d "${SUPPORT_DIR}"` here is idempotent with the identical call the health-check
  step already makes later (line ~189); either order is fine.
- No change is needed to the existing `rm -f .../permit` line or the "to allow, touch the permit"
  hint — the presence file is still the bridge's signal; only the mechanism's *read* moved behind
  the daemon (design §6.3).

---

## Patch 2 — `native/macos/minimal-auth-plugin/uninstall.sh`

### Hunk 2a — declare permitd variables

Anchor (currently near line 24-25):

```bash
DAEMON_LABEL="ai.repose.spike.healthcheck"
DAEMON_PLIST="/Library/LaunchDaemons/${DAEMON_LABEL}.plist"
```

Insert immediately AFTER that anchor:

```bash
# permit IPC consume-daemon, installed by install.sh.
PERMITD_LABEL="ai.repose.spike.permitd"
PERMITD_PLIST="/Library/LaunchDaemons/${PERMITD_LABEL}.plist"
PERMITD_IPC_DIR="/var/run/repose-spike/ipc"
```

### Hunk 2b — bootout + remove the daemon (before the rule stops referencing the mechanism)

Anchor (the health-check removal block, currently near lines 67-70):

```bash
echo "==> Removing health-check daemon ${DAEMON_LABEL}"
launchctl bootout "system/${DAEMON_LABEL}" 2>/dev/null || true
rm -f "${DAEMON_PLIST}"
rm -rf "${SUPPORT_DIR}"
```

Insert immediately AFTER that anchor block (still before the rule is restored/cleaned):

```bash
# Stop the permit daemon here too — before the rule stops referencing the
# mechanism — for the same reason as the health-check daemon: make the uninstall
# the only writer and leave nothing listening. The daemon binary lives under
# ${SUPPORT_DIR}, already removed just above; its plist and socket dir are separate.
echo "==> Removing permit daemon ${PERMITD_LABEL}"
launchctl bootout "system/${PERMITD_LABEL}" 2>/dev/null || true
rm -f "${PERMITD_PLIST}"
rm -rf "${PERMITD_IPC_DIR}"
```

Note: the existing `rm -f /var/run/repose-spike/permit /tmp/repose-permit` (near line 115) stays as
is; it removes the presence file, and the added `rm -rf "${PERMITD_IPC_DIR}"` removes the socket
directory beside it.

---

## Post-apply verification (on the VM, per design §8.2)

```bash
cd native/macos/permit-daemon && ./build.sh
cd ../minimal-auth-plugin && make && sudo ./install.sh permit
# socket:  srw-rw---- root _securityagent  /var/run/repose-spike/ipc/permit.sock
# dir:     drwxr-x--- root _securityagent  /var/run/repose-spike/ipc
ls -le /var/run/repose-spike/ipc /var/run/repose-spike/ipc/permit.sock
launchctl print system/ai.repose.spike.permitd | sed -n '1,20p'
```

Uninstall leaves nothing behind:

```bash
sudo ./uninstall.sh
launchctl print system/ai.repose.spike.permitd   # => could not find service (expected)
ls /var/run/repose-spike/ipc                       # => No such file or directory (expected)
```
