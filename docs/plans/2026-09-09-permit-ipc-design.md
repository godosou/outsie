# permit IPC design — closing gap #3 with a root consume-daemon

Date: 2026-09-09
Branch: `feat/phone-unlock-walking-skeleton`
Status: **Stage-2 scaffold — NOT integrated.** The Phone Key app (Stage-1) uses the hardened
FILE permit; this daemon is the gap-#3 (replay) endgame for distribution (gate G3), wired in later.

> **⚠️ CORRECTIONS supersede the body below** (from this doc's own adversarial review; the
> generation forked into two implementations):
> - **Canonical implementation: `native/macos/permit-daemon/`** (`repose-permitd.c` + `repose_permit_wire.h`
>   + `repose_permit_client.c` + `repose_peer_verify.c` + `build.sh`). It builds `-Werror`, has a client
>   and a probe, and is what the test + install-permitd.sh use.
> - The **40-byte** frame (`repose_permit_wire.h`, magic `RPU1`) is canonical. The duplicate 84-byte
>   `permit_protocol.h` / `permit-daemon.c` under `minimal-auth-plugin/` was **orphaned and has been
>   deleted**. Wherever §2 / §6.1 below say "84-byte codex frame verbatim", read "40-byte frame"; the
>   "codex `wire_vectors.rs` oracle still validates" rationale is **void** (this is not the codex frame).
> - Socket path is **`/var/run/repose-permitd/permit.sock`** (the daemon self-creates and chowns its
>   dir). The INTEGRATION doc's `/var/run/repose-spike/ipc` paths are **wrong** — reconcile before use.
> - **Test-seam gap:** `repose-permitd` refuses to start unless `euid==0` and requires a root-owned
>   presence file, so the consume/replay/peer-verify matrix runs **only as root** (VM/real-Mac). The
>   unprivileged client fail-closed tests pass (7/7). Add a `--insecure-test-mode` seam to make the
>   full matrix runnable unprivileged.
> - Verified so far: `build.sh` compiles daemon+probe clean; `tests/permit_daemon_test.sh` 7 pass / 10
>   skip (root-only). Gap-#3 single-consume is asserted only at the root tier — run
>   `sudo native/macos/minimal-auth-plugin/tests/permit_daemon_test.sh` on a disposable Mac to cover it.

Status (original): design (human integrates; this doc produces NO edits to existing files — only patch-specs)

> This is the IPC endgame promised by [2026-09-09-permit-design.md](2026-09-09-permit-design.md).
> The file permit closed gaps #1 (world-writable), #2 (no expiry), #4 (crash ⇒ stale ⇒
> fail-open). The **remaining** gap is **#3: the permit is not bound to a specific unlock
> attempt and can be replayed within the 15s freshness window.** This document replaces the
> plugin's *read* of the permit file with a **request/response to a root daemon over a unix
> socket** that evaluates presence live, answers once per connection, and **consumes** the
> presence assertion atomically so it cannot authorize a second unlock. The bridge and the
> presence file are unchanged; only who reads the file, and how the verdict is bound to one
> attempt, changes.

---

## 0. Terms

- **Mechanism** — the Authorization Plugin code running inside SecurityAgent (uid 92) or
  authorizationhost (uid 0). The `kModePermit` branch of `plugin.c`.
- **Daemon** — the new root LaunchDaemon (`repose-permitd`). Owns the socket, verifies peers,
  reads presence, decides Allow/Deny.
- **Bridge** — `tools/ble-spike/mac/permit-bridge.sh`, unchanged. Re-touches the presence file
  while the phone is near.
- **Presence file** — `/var/run/repose-spike/permit`, root-owned, re-touched every ~5s by the
  bridge. Still the presence signal. The daemon reads it; the mechanism no longer does.

---

## 1. Decision: self-contained C daemon (not a Rust dependency)

**Decision: write `repose-permitd` in C, buildable with `clang` alone. Reuse two C assets from
the codex prior art; do NOT take a Rust/cargo build dependency.**

Reasoning:

- A clean macOS has **no guaranteed Rust toolchain**; the spike's plugin and tooling are C/shell
  and must build with `clang`. A Rust daemon adds cargo + a `build.rs` FFI bridge for a daemon
  that needs ~300 lines of C.
- The single most valuable prior-art asset — `repose-unlock-service/native/peer_identity.c` — is
  **already pure C** (CoreFoundation + Security + libbsm) and does exactly the peer verification
  the constraints demand. Reuse it (two small edits, §3.3).
- The wire contract is a **frozen C header**: `repose-unlock-ipc/include/repose_unlock_ipc.h`,
  offsets + magic + `_Static_assert`, with Rust test vectors (`tests/wire_vectors.rs`) that
  double as an oracle. Reuse the header verbatim (§2).
- The Rust crates encode genuinely stronger semantics — `permit_broker.rs` (concurrent
  single-consume), `permit.rs::ConsumedPermit` (exactly-once at the type level) — and are the
  **right choice at productization**, when the daemon graduates from reading the bridge's file to
  holding presence state and issuing per-attempt cryptographic bindings. For the spike they carry
  far more than gap #3 needs (durable replay counters, watch/keepalive, session reducer, service
  ids). **Keep them as the specification for the state transitions, not as a build dependency.**

So: reuse the **C wire header** and the **C peer-verifier**, reimplement single-consume as a
short mutex-guarded critical section in C (§4), and treat the Rust crates as the reference.

---

## 2. Wire protocol — reuse the codex 84-byte fixed frame

**Adopt `repose_unlock_ipc.h` verbatim.** It is ABI-frozen, has an existing test-vector oracle,
and is forward-compatible with the product's WATCH path. The spike uses a **subset** of it:
operation `CONSUME`, statuses `REQUEST` / `CONSUMED` / `DENIED`. No WATCH, no keepalive, no events.

### 2.1 Frame

Every frame — request and response — is **exactly 84 bytes**, no length prefix, no variable
fields. Byte order for multi-byte integer fields is **little-endian** (host order on the target;
frozen as LE in the header's vectors). Layout (offsets from `repose_unlock_ipc.h`):

| Offset | Len | Field | Request value | Response value |
|-------:|----:|-------|---------------|----------------|
| 0  | 4  | magic         | `0x52 0x50 0x55 0x49` = `"RPUI"` | same |
| 4  | 1  | version       | `1` | `1` |
| 5  | 1  | operation     | `1` (CONSUME) | `1` (echoed) |
| 6  | 1  | status        | `0` (REQUEST) | `1` CONSUMED **or** `3` DENIED |
| 7  | 1  | flags         | `0` | `0` |
| 8  | 4  | payload_len   | `72` | `72` |
| 12 | 32 | nonce         | client random (see 2.3) | **echoed** unchanged |
| 44 | 4  | console_uid   | client's console uid | echoed |
| 48 | 4  | audit_session | client's audit session id | echoed |
| 52 | 8  | lock_epoch    | client's lock-attempt id | echoed |
| 60 | 16 | instance      | zero (spike) | daemon instance id (or zero) |
| 76 | 8  | watch_id      | zero (unused in spike) | zero |

Constants (from the header): `REPOSE_UNLOCK_IPC_FRAME_LEN 84`, `..._MAGIC_0..3`,
`..._VERSION 1`, `..._OP_CONSUME_OR_WATCH 1`, `..._STATUS_REQUEST 0`, `..._STATUS_CONSUMED 1`,
`..._STATUS_DENIED 3`. Statuses `WATCHING 2`, `EVENT 4`, `KEEPALIVE 5` and op `PERMIT_AVAILABLE 2`
are defined but **unused** by the spike daemon; if a client ever sends them, the daemon replies
DENIED.

### 2.2 Exchange (one connection = one unlock attempt)

```
mechanism                              daemon
   | connect(permit.sock) ------------->|  accept()
   | write(84-byte REQUEST) ----------->|
   | shutdown(fd, SHUT_WR) ------------>|  read to EOF; require exactly 84 bytes + EOF
   |                                    |  verify peer (uid allowlist + SecCodeCheckValidity)
   |                                    |  read presence live; single-consume (mutex)
   |<----------- write(84-byte REPLY) --|  status = CONSUMED | DENIED
   | read(84) then close -------------->|  close()
   Allow iff reply is a well-formed CONSUMED with matching nonce; else Deny
```

The **write-half-close is mandatory** (`REPOSE_UNLOCK_IPC_REQUEST_REQUIRES_WRITE_HALF_CLOSE`).
The daemon confirms EOF-of-request *before* it consumes. This stops (a) a truncated frame from
consuming and (b) a client holding the connection open to consume repeatedly.

### 2.3 What the nonce is and is NOT

The nonce is **frame anti-confusion only** — it lets the client prove the reply it read belongs
to the request it wrote. **It is not a replay defense.** Gap #3 replay is defeated by
single-consume + peer verification (§4), not by the nonce. (A verified, code-signed mechanism does
not replay its own frames to double-unlock; a non-mechanism peer is rejected before consume
regardless of any nonce.) The client MUST still generate it with `arc4random_buf(32)` per attempt
and MUST reject a reply whose nonce differs.

---

## 3. Socket + peer verification

### 3.1 Path, owner, group, mode

- **Presence file (unchanged):** `/var/run/repose-spike/permit` — root-owned, bridge-maintained.
- **Socket dir (new):** `/var/run/repose-spike/ipc/` — **owner `root`, group `_securityagent`
  (0:92), mode `0750`**.
- **Socket (new):** `/var/run/repose-spike/ipc/permit.sock` — **owner `root`, group
  `_securityagent` (0:92), mode `0660`**.

Why a dedicated `ipc/` subdirectory rather than the socket sitting directly in
`/var/run/repose-spike/`: the bridge's default `PERMIT_ON_CMD` runs
`chmod 755 /var/run/repose-spike` every ~5s while the phone is near. That would keep loosening a
`0750` gate on the parent. The bridge **never touches the `ipc/` subdirectory**, so the `0750`
gate there survives. Traversal to reach the socket needs `+x` on `/var/run/repose-spike` (0755 —
everyone) **and** on `ipc/` (0750 — only root and gid 92), so an ordinary uid gets `EACCES` at
path resolution, before `connect()`.

### 3.2 Why these bits let uid 0 AND uid 92 in but exclude ordinary users

Measured on macOS 14.6.1:

- `_securityagent` is **uid 92, primary gid 92**; its full group set is
  `92, 12(everyone), 61(localaccounts), 100(_lpoperator)`.
- An ordinary interactive user (uid 501) is in `20(staff), 12, 61, 100, …`.

Ordinary users are **also** in 12/61/100 — those groups are effectively world and are forbidden by
the permit-design "never world-writable" rule. Of `_securityagent`'s groups, **only gid 92
excludes ordinary users**, and gid 92 has an empty explicit membership (only uid 92 carries it).
So `0660 root:_securityagent` admits exactly `{root (owner), uid 92 (group)}` and no one else —
the tightest ACL that includes the non-privileged mechanism host. For the **privileged variant**
(host = authorizationhost, uid 0), root is the owner and connects regardless; `root:_securityagent
0660` still admits it, so **one socket config serves both install variants** with no
reconfiguration.

**Filesystem perms are defense-in-depth only.** macOS unix-socket `connect()` enforcement on the
socket *node* has varied across releases; the `0750` directory-traversal gate (reliable VFS
behavior) plus the in-daemon peer checks (§3.3) are the real boundaries. The daemon sets the bits
itself after `bind()` (`fchown`/`chown` + `chmod`) and re-asserts them; do not trust the plist
alone (the earlier `0666` control-socket incident is the cautionary tale).

### 3.3 Peer verification (the real gate) — concrete C

The daemon verifies **every** connection before it reads presence or consumes. Reuse
`git show codex/phone-proximity-unlock:src-tauri/crates/repose-unlock-service/native/peer_identity.c`
almost verbatim; it is pure C and links `-framework Security -framework CoreFoundation -lbsm`.

Flow:

1. `getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, &audit_token, &len)` — `SOL_LOCAL`(=0) and
   `LOCAL_PEERTOKEN`(=0x006) from `<sys/un.h>`. This yields the **exact process** (pid +
   pid-generation, immune to pid reuse) and carries the euid + audit session id. Preferred over
   `getpeereid`/`LOCAL_PEERCRED` because the audit token is what `SecCode` consumes.
2. `audit_token_to_euid()`, `audit_token_to_pid()`, `audit_token_to_asid()` — `<bsm/libbsm.h>`.
3. **uid allowlist:** euid must be **0 or 92**; anything else ⇒ reject. Select the requirement by
   uid.
4. `CFDataCreate(token)` → `CFDictionaryCreate({kSecGuestAttributeAudit: data})` (`<Security/SecCode.h>`).
5. `SecCodeCopyGuestWithAttributes(NULL, attrs, kSecCSDefaultFlags, &code)`.
6. `SecRequirementCreateWithString(req, kSecCSDefaultFlags, &requirement)` (`<Security/SecRequirement.h>`).
7. `SecCodeCheckValidity(code, kSecCSDefaultFlags, requirement)` — **this is the identity gate; uid
   is necessary but never sufficient.**

**Designated requirements to pin (verified with `codesign -dr -` on macOS 14.6.1):**

| Host | Install variant | euid | Requirement string |
|------|-----------------|-----:|--------------------|
| authorizationhost | `PRIVILEGED=1` | 0 | `identifier "com.apple.authorizationhost" and anchor apple` |
| SecurityAgent | `PRIVILEGED=0` | 92 | `identifier "com.apple.SecurityAgent" and anchor apple` |

The daemon holds **both** and cross-checks uid↔identity (uid 0 ⇒ authorizationhost requirement;
uid 92 ⇒ SecurityAgent requirement), so it supports both install variants without
reconfiguration.

**Two edits vs. the prior-art C** (which hard-rejected non-root and pinned only
authorizationhost): (a) generalize `euid != 0` reject to the `{0, 92}` two-host allowlist with
per-uid requirements; (b) **drop** the prior art's `audit_session_id <= AU_DEFAUDITSID` hard
reject — the code-signing DR already proves the peer is the exact Apple-signed host, and the asid
value is a field Apple may repopulate differently across releases (false-denial risk). Keep the
`pid > 0` sanity check.

**Bind the claim to the verified session:** cross-check that the request frame's `console_uid` and
`audit_session` (offsets 44/48) match the values derived from the verified peer's audit token. A
mismatch ⇒ DENIED. This makes the frame's session fields trustworthy for the consume key (§4).

Any failure in this section ⇒ the daemon replies DENIED (or closes), and the mechanism falls back
to the password.

**Version-fragility notes:** `LOCAL_PEERTOKEN`/`SOL_LOCAL` via `getsockopt` is undocumented in man
pages but stable since ~10.8. Re-verify the DR strings with `codesign -dr -` on each target OS
major. Use `kSecGuestAttributeAudit` (not the pid-only guest attributes, which are pid-reuse-racy).

---

## 4. Consume / binding semantics — closing gap #3

### 4.1 The threat, precisely

The file permit is a **level-triggered ambient fact, not an event bound to a request**: its
validity is a pure function of `(exists, root-owned, mtime-fresh)`, never of *which* attempt is
asking or *how many times* it has already answered. Reading a file does not remove it, so within
the 15s window it is a **reusable bearer credential**. The core misuse (T2, "presence-then-
departure replay"): the bridge touches at t=0 because the phone is near; the owner leaves at t=3s;
the mtime is still <15s old, so an attacker at the keyboard unlocks at t=4s…t=15s. One walk-past
"charges" the machine for ~15s of unlocks by anyone. (T1 unbounded reuse and T3 cross-session
reuse are the same defect.)

### 4.2 The invariant the daemon must hold

> A given presence assertion authorizes **at most one** Allow, and every Allow reflects presence
> the daemon judged **live at the instant it answered** — not presence observed earlier and read
> off an artifact.

Two independently necessary properties:

1. **Live evaluation (defeats T2):** Allow only if, *at the moment the request is serviced*,
   presence is fresh **now** (§5). No cached "present" flag exists to get stuck.
2. **Atomic single-consume (defeats T1/T3):** when the daemon answers Allow it atomically marks the
   assertion it used as spent; a second request cannot Allow off the *same* assertion. The next
   Allow requires a **new** assertion — presence re-observed (a newer mtime) since the last consume.

### 4.3 Minimal mechanism — connection = attempt

The load-bearing fact: **at the lock screen the mechanism is invoked fresh per unlock attempt, and
each invocation opens a fresh socket connection.** Therefore **the connection itself is the unit of
"this unlock attempt"** — the daemon needs no client-minted token and no issued one-shot token to
bind an attempt. Candidate (C) from the threat analysis — *daemon evaluates presence live per
request and atomically marks it consumed* — is **sufficient and minimal**. Client nonces (A) and
daemon-issued tokens (B) are gold-plating for gap #3 (a token exists to carry authorization
*across* connections; here there is no "later").

### 4.4 The generation + consume key

- **Presence generation** = the presence file's `mtime`. Each bridge re-touch produces a new
  generation. The daemon keeps a **consume watermark** = the mtime of the last generation it spent.
- **Consume rule (Allow):** peer verified ∧ request EOF confirmed ∧ presence fresh now ∧
  `file.mtime > watermark`. On Allow, set `watermark = file.mtime`.
- **Belt-and-suspenders session binding (cheap DiD, not the load-bearing part):** also refuse to
  Allow twice for the same `(audit_session, lock_epoch)` tuple from the request frame (verified in
  §3.3). `audit_session` comes free from the peer's audit token; `lock_epoch` increments per lock.
  This ensures a consume triggered by session A cannot satisfy session B.
- **Startup seed:** watermark = daemon start time (`mach_continuous_time`-anchored), so a crash +
  `KeepAlive` restart cannot replay a pre-restart generation.

### 4.5 Trace

- **1st request, fresh generation:** verified ∧ fresh ∧ `mtime > watermark` ⇒ **CONSUMED (Allow)**;
  watermark ← mtime.
- **2nd request immediately after, no new touch:** `mtime == watermark` ⇒ **DENIED**; the mechanism
  falls back to the password. Correct: one presence assertion bought exactly one unlock.
- **Departure race (T2):** phone gone ⇒ no new touch ⇒ mtime stays stale/unchanged ⇒ DENIED.
- **Legitimate repeat:** two human attempts are seconds apart; the bridge touches every ~5s, so
  attempt 2 Allows off a **newer** generation. Not a regression — each Allow is backed by an
  observation no older than the freshness bound.

### 4.6 Atomicity in C

The take must happen under the same mutex that guards presence read + watermark, before/at the
moment the CONSUMED byte is written, so two concurrent connections cannot both see the generation
unspent. The lock screen is serial, so the daemon is single-threaded with a bounded per-connection
I/O timeout (below); a `pthread_mutex_t` around `{read presence + check mtime>watermark + set
watermark + decide}` is required if concurrency is ever introduced. This mirrors the single
`inner.lock()` in `PermitStore::consume`'s `permit.take()`.

**This is NOT** a per-attempt cryptographic binding to a specific lock event — that is
`repose-unlock-core::ConsumedPermit` + `permit_broker`'s job at productization. It DOES close gap
#3's "replay within the freshness window" for the spike.

---

## 5. Presence feed + the daemon's own fail-closed freshness

**Chosen option: Candidate A — the daemon reads the bridge's presence file per request.**
`permit-bridge.sh` is **unchanged**; the daemon `stat`s `/var/run/repose-spike/permit` fresh on
every request and holds **no cached presence state**.

Rejected: Candidate B (bridge pushes present/absent over a socket). It would need a bash
unix-socket client with reconnect + heartbeat (`nc -U` not guaranteed on a clean Mac), fights the
"clang/shell only" constraint, adds a **second** peer to authorize (the bridge is neither
SecurityAgent nor authorizationhost), and its default "remember last push" behavior is
**fail-open** unless the daemon re-implements the very heartbeat/deadline it gets for free from
`mtime` under A. Gap #3 is closed in the consume path (§4) **regardless** of A or B, so the
presence source is chosen on simplicity and failure direction, and A wins on both.

The daemon derives "present" fresh, per request — `wait_for_permit`'s inner body minus the poll
loop, using the same constants as `plugin.c` so behavior is identical to the validated file check:

```c
#define PRESENCE_PATH        "/var/run/repose-spike/permit"
#define PRESENCE_FRESHNESS_S 15   /* must be > bridge REFRESH_S (5) with margin */
#define PRESENCE_SKEW_S       5

/* Returns 1 only if the bridge-maintained permit is a regular, root-owned, fresh
 * file RIGHT NOW, and reports the generation (mtime) for the consume watermark.
 * Any doubt returns 0 (=> DENIED => password). */
static int presence_is_fresh(time_t *gen_out) {
    int fd = open(PRESENCE_PATH, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) return 0;                     /* absent / symlink / error => absent */
    struct stat st; int fresh = 0;
    if (fstat(fd, &st) == 0 && S_ISREG(st.st_mode) && st.st_uid == 0) {
        double age = difftime(time(NULL), st.st_mtime);
        if (age <= PRESENCE_FRESHNESS_S && age >= -PRESENCE_SKEW_S) {
            fresh = 1;
            if (gen_out) *gen_out = st.st_mtime;   /* generation for §4.4 */
        }
    }
    close(fd);
    return fresh;
}
```

Both directions of a wall-clock jump fail **closed**: too-old ⇒ DENY, future-dated beyond skew ⇒
DENY. At startup (once, not per request) the daemon asserts `/var/run/repose-spike` is root-owned
and not group/other-writable and refuses to serve otherwise. The consume decision is:

```
verify_peer(conn) && await_write_half_close(conn, deadline)
  && presence_is_fresh(&gen) && gen > watermark
  && !already_allowed(audit_session, lock_epoch)   /* §4.4 DiD */
  ? (watermark = gen, record(audit_session,lock_epoch), CONSUMED)
  : DENIED
```

---

## 6. Patch-specs for existing files (human applies; NOT applied here)

### 6.1 `native/macos/minimal-auth-plugin/plugin.c`

Replace the *file read* with a **bounded-timeout socket request/response client** that fails
closed. **Keep the `permit` mode id and the k-of-n=1 rule shape** (mechanism Allow ⇒ unlock).

- **P1** — add near the includes: `#include "repose_permit_client.h"` (new file, §7-adjacent).
- **P2** — in `MechanismInvoke`, `case kModePermit:` replace
  `result = wait_for_permit() ? kAuthorizationResultAllow : kAuthorizationResultDeny;`
  with `result = repose_request_permit() ? kAuthorizationResultAllow : kAuthorizationResultDeny;`
- **P3** (required, not optional) — delete the now-dead `wait_for_permit()` and `permit_path()`
  and the `PERMIT_DIR/PERMIT_PATH/PERMIT_FRESHNESS_S/PERMIT_SKEW_S/PERMIT_TIMEOUT_MS/PERMIT_POLL_MS`
  macros. The Makefile uses `-Werror`, so leaving them triggers `-Wunused-function`. Keep
  `repose_log` and its `fcntl`/`sys/stat` includes — still used.

`repose_request_permit()` (new file `repose_permit_client.c`, plain sockets, no frameworks, so it
builds `-arch arm64 -arch x86_64` on libSystem alone) must:

1. `connect()` to `/var/run/repose-spike/ipc/permit.sock` under a **bounded connect timeout**
   (non-blocking connect + `poll`). Honor a `REPOSE_PERMIT_SOCK_PATH` env override for tests (mirrors
   today's `REPOSE_PERMIT_PATH`).
2. Fill an 84-byte REQUEST: magic/version/op=CONSUME/status=REQUEST/payload_len=72, `arc4random_buf`
   nonce, `console_uid`, `audit_session` (from `audit_token_to_asid` on self), `lock_epoch`.
3. `write` the full frame, `shutdown(fd, SHUT_WR)`.
4. `read` exactly 84 bytes under a **bounded read timeout** (`poll` with an absolute deadline; total
   budget ≤ ~1.5s to match today's `PERMIT_TIMEOUT_MS` so the user never stares at a frozen screen).
5. Return **1 (Allow) iff** the reply is well-formed (magic/version/op match), status == CONSUMED,
   and nonce echoes the request. **Every other outcome — connect fail, timeout, short read, bad
   magic, wrong nonce, DENIED, EOF — returns 0 (Deny).** Never hang; never block unbounded.

### 6.2 `native/macos/minimal-auth-plugin/Makefile`

- **M1** — append the client sources to the bundle dependency line:
  `../permit-daemon/repose_permit_client.c ../permit-daemon/repose_permit_client.h ../permit-daemon/repose_permit_wire.h`
  (or `repose_unlock_ipc.h` if reusing the codex header name).
- **M2** — compile line: `clang $(CFLAGS) $(FRAMEWORKS) -I../permit-daemon -o $(BINARY) plugin.c
  ../permit-daemon/repose_permit_client.c`. The client needs **no** extra frameworks.

### 6.3 `native/macos/minimal-auth-plugin/install.sh`

No change required for correctness — its `rm -f /var/run/repose-spike/permit` and "to allow, touch
the permit" hint stay accurate (the presence file is still the bridge's signal; only the
mechanism's *read* moved behind the daemon). Recommended additions (or, cleaner, keep them in a
**separate `install-permitd.sh`**, §6.6, so `install.sh` is untouched):

- Build + install the daemon **before** wiring the mechanism (if the daemon isn't listening the
  mechanism simply denies ⇒ password, which is safe).
- Create `/var/run/repose-spike/ipc` `0750 root:_securityagent`; install the LaunchDaemon plist;
  `launchctl bootout` any old instance then `bootstrap system`; `kickstart`.
- Run order on the VM: `cd native/macos/permit-daemon && ./build.sh && sudo ./install-permitd.sh`
  then `cd native/macos/minimal-auth-plugin && make && sudo ./install.sh permit`.

### 6.4 `native/macos/minimal-auth-plugin/uninstall.sh`

Recommended additions (or in a separate `uninstall-permitd.sh`): `launchctl bootout
system/ai.repose.spike.permitd`; `rm -f` the daemon plist and binary; `rm -rf
/var/run/repose-spike/ipc`. Order: bootout the daemon **before** removing the mechanism/rule, same
race-avoidance logic the existing script uses for the health-check daemon. The existing
`rm -f /var/run/repose-spike/permit` stays.

### 6.5 `tools/ble-spike/mac/permit-bridge.sh`

**No change** (Candidate A). Preserve the two invariants that already hold: the refresh period
stays strictly below the daemon's freshness window (`REFRESH_S=5` < `PRESENCE_FRESHNESS_S=15`), and
LEAVE/STALE keep clearing the file (`rm -f`). The bridge's `chmod 755 /var/run/repose-spike` does
not affect the `ipc/` subdir (§3.1).

### 6.6 New files (added, not edits)

`native/macos/permit-daemon/`:
`repose_unlock_ipc.h` (reused codex header) or `repose_permit_wire.h`; `repose_peer_verify.{h,c}`
(reused `peer_identity.c` + two edits); `repose-permitd.c` (main: socket setup, peer verify,
presence read, single-consume, fail-closed); `repose_permit_client.{h,c}` (§6.1);
`repose-permit-probe.c` (throwaway CLI client for smoke tests); `build.sh` (clang-only);
`ai.repose.spike.permitd.plist`; `install-permitd.sh` / `uninstall-permitd.sh`; a `.gitignore` for
`build/` mirroring the sibling plugin dir.

---

## 7. LaunchDaemon plist design

**Recommendation: the daemon creates and owns the socket itself** (RunAtLoad + KeepAlive), **not**
launchd socket activation.

Why not `Sockets` socket activation: launchd's `SockPathMode` sets the node mode but does not
reliably set **group ownership** to `_securityagent`, which is exactly the bit we depend on (§3.1).
Owning the socket in-process lets the daemon `bind()`, then `chown(0, 92)` + `chmod(0660)` the
node and `chmod(0750)` the dir, and re-assert on each launch — deterministic and auditable. The
fail-closed client (§6.1) already handles the brief window where the socket is missing (connect
fails ⇒ Deny ⇒ password), so socket activation's "no startup race" advantage buys nothing here.

`/Library/LaunchDaemons/ai.repose.spike.permitd.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>            <string>ai.repose.spike.permitd</string>
  <key>ProgramArguments</key> <array><string>/Library/Application Support/ReposeSpike/repose-permitd</string></array>
  <key>RunAtLoad</key>        <true/>
  <key>KeepAlive</key>        <true/>            <!-- crash => restart; watermark reseeds to start time -->
  <key>ProcessType</key>      <string>Interactive</string>
  <key>ThrottleInterval</key> <integer>1</integer>
  <key>StandardErrorPath</key><string>/var/log/repose-permitd.err</string>
</dict>
</plist>
```

Runs as **root** (LaunchDaemon default) — required to read the root-owned presence file and to
`chown` the socket to gid 92. Plist installed `root:wheel 0644`; the binary in the existing
root-owned `/Library/Application Support/ReposeSpike/` (0755, not writable by anyone but root).
The daemon **refuses to start if not root** (fail-closed startup guard). The E12 health-check can
optionally re-assert the socket dir/mode bits.

---

## 8. Test plan

### 8.1 Sandboxed C unit tests (no root, no lock screen, no VM)

Build the daemon and a mock/probe with `clang` on the dev Mac. Use `REPOSE_PERMIT_SOCK_PATH` to
point client and daemon at a scratch socket in a temp dir, and a
`--insecure-skip-peer-verify` daemon build/flag to exercise the wire + consume path without a real
signed host.

**A. Client fail-closed + bounded-time matrix** (client vs. a mock server on a scratch socket):

| Case | Server behavior | Expect |
|------|-----------------|--------|
| allow | CONSUMED, nonce echoed | Allow, fast |
| deny | DENIED | Deny |
| wrong nonce | CONSUMED, mutated nonce | Deny |
| garbage | random bytes / short frame | Deny |
| no server | socket absent | Deny, bounded (~connect timeout) |
| black hole | accept, never reply | Deny, bounded (~read deadline) |

Assert every non-allow case returns within the total time budget (never hangs).

**B. Consume-once (daemon, mock presence file):** create `PRESENCE_PATH` fresh (root-owned in the
test's fake root, or relax the uid check under the test flag). Request #1 ⇒ CONSUMED; request #2
with **no new touch** ⇒ DENIED; `touch` the file (new mtime) ⇒ next request CONSUMED. Also: two
requests with the same `(audit_session, lock_epoch)` ⇒ second DENIED even if the generation
advanced (§4.4 DiD).

**C. Stale / fail-closed presence:** file mtime older than `PRESENCE_FRESHNESS_S` ⇒ DENIED; file
absent ⇒ DENIED; file future-dated beyond `PRESENCE_SKEW_S` ⇒ DENIED; file not root-owned ⇒
DENIED; symlink/FIFO at the path ⇒ DENIED.

**D. Malformed request:** short frame (<84B) ⇒ DENIED, connection closed, **no consume**; bad
magic/version/op ⇒ DENIED; frame written but **no `SHUT_WR`** within the deadline ⇒ daemon times
out, DENIED, no consume; oversized/garbage ⇒ DENIED.

**E. Peer uid branch (logic only):** unit-test the uid→requirement selection function directly
(uid 0 ⇒ authorizationhost DR; uid 92 ⇒ SecurityAgent DR; any other uid ⇒ reject) with injected
uids. `SecCodeCheckValidity` against the **real** hosts cannot be faked and is deferred to §8.2.

**F. Startup guard:** daemon refuses to start as non-root; refuses to serve if
`/var/run/repose-spike` is group/other-writable.

### 8.2 End-to-end verification on the VM (needs root + real hosts)

1. Build + install: `cd native/macos/permit-daemon && ./build.sh && sudo ./install-permitd.sh`;
   confirm the socket is `srw-rw---- root _securityagent` and the dir is `drwxr-x--- root
   _securityagent`; confirm `repose-permitd` is running (`launchctl print system/...`).
2. **Wire/consume path without the lock screen:** run `repose-permit-probe` against the live socket
   with `--insecure-skip-peer-verify` (or a probe signed to pass) to confirm CONSUMED once /
   DENIED-on-replay against the real presence file re-touched by the bridge.
3. Install the mechanism: `cd native/macos/minimal-auth-plugin && make && sudo ./install.sh permit`.
4. **Peer verification (the part only the VM can prove):** at the real lock screen, phone near ⇒
   the mechanism's connection passes `SecCodeCheckValidity` as SecurityAgent (or authorizationhost
   under `PRIVILEGED=1`) ⇒ CONSUMED ⇒ unlock. Confirm a **non-mechanism** local process (the probe
   without the skip flag) is rejected by peer verification even though its uid may be allowed.
5. **Departure-replay (gap #3 regression):** phone near ⇒ unlock once. Move the phone away; before
   the freshness window expires, attempt to unlock again from the keyboard ⇒ **password required**
   (the generation was consumed and no new touch arrived). This is the exact scenario the file
   permit failed.
6. **Fail-closed:** `launchctl bootout` the daemon ⇒ lock screen requires the password (connect
   fails ⇒ Deny). Restore ⇒ unlock works again. Kill the bridge ⇒ file goes stale ⇒ password.
7. Roll back the VM snapshot when done.

---

## Appendix — Rust prior art mapped to this design (reference, not dependency)

| Rust asset (`codex/phone-proximity-unlock`) | Role here |
|---|---|
| `repose-unlock-ipc` (`repose_unlock_ipc.h` + `wire_vectors.rs`) | **Reused C header** (§2) + test oracle |
| `repose-unlock-service/native/peer_identity.c` | **Reused C** (§3.3) + two edits |
| `repose-unlock-service/src/permit_broker.rs` | Specification for atomic single-consume (§4.6) |
| `repose-unlock-core/src/permit.rs::ConsumedPermit` | Specification for exactly-once; the productization target beyond gap #3 |
| `repose-unlock-service/src/peer_identity.rs` | Reference for the audit-token → SecCode flow (Rust side) |
