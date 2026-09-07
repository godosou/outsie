# Phone Proximity Unlock Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a fail-closed Android/iOS phone companion and macOS authorization prototype that unlocks only an already logged-in local session after calibrated leave-and-return proximity plus a hardware-backed cryptographic challenge.

**Architecture:** Keep the existing Tauri application unprivileged. Put deterministic security logic in independent Rust crates, use a bounded native IPC protocol between a minimal C Authorization Plugin and a standalone macOS service, and use Flutter only for shared mobile UI while Kotlin/Swift own background BLE and hardware keys. Prove password fallback and unattended authorization re-evaluation before enabling real system installation.

**Tech Stack:** Rust 1.91, Tauri 2, C/Objective-C macOS Authorization Plugin API, Security.framework, AF_UNIX/audit tokens, Flutter 3.38.9/Dart 3.10.8, Kotlin/API 36 Companion Device APIs, Swift/Core Bluetooth/AccessorySetupKit, P-256 ECDSA/ECDH, HKDF-SHA256, AES-GCM.

---

## Execution rules

- Follow `@superpowers:test-driven-development` for every behavior change: red, inspect the failure, minimal green, refactor, commit.
- Use `@superpowers:verification-before-completion` before any completion claim.
- Do not modify timer lifecycle code (`src/hooks/useBreakTimer.ts`, `src/lib/timer.ts`, or its tests).
- Never run an installer, write `authorizationdb`, lock the current session, or load the prototype into `authorizationhost` on this development Mac as part of automated execution.
- Real authorization testing requires the safety checklist in `native/macos/tests/real-machine-checklist.md` and a dedicated recoverable Mac.
- Keep `system.login.console` outside every API surface. The only supported system right is `system.login.screensaver`.
- If password fallback, unattended re-evaluation, or login-keychain behavior fails on any supported macOS version, stop the product path and retain only the documented prototype.

## Known environment gaps

- Android SDK 35 is installed; API 36 platform/build tools are missing.
- Flutter cache metadata reports 3.38.9; FVM availability was not reverified.
- ADB device visibility is not verified because its daemon could not start in the restricted
  inventory environment.
- Command Line Tools are selected as the active developer directory. Full Xcode presence and an
  iPhoneOS SDK were not verified; `xcodebuild` is unavailable with the current selection.
- `security find-identity` reported no valid code-signing identity; notarization credentials were
  not verified.

These gaps do not block pure Rust, C ABI, Dart, and fixture work. They do block API 36 compilation
until the SDK is installed, all iOS builds until a supported Xcode/iPhoneOS SDK is selected, and all
true phone/system-unlock acceptance tests until a device is independently enumerated and authorized.

### Task 1: Create the isolated Rust workspace and calibration model

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/crates/repose-unlock-core/Cargo.toml`
- Create: `src-tauri/crates/repose-unlock-core/src/lib.rs`
- Create: `src-tauri/crates/repose-unlock-core/src/domain.rs`
- Create: `src-tauri/crates/repose-unlock-core/src/calibration.rs`
- Create: `src-tauri/crates/repose-unlock-core/src/proximity.rs`
- Test: `src-tauri/crates/repose-unlock-core/tests/calibration.rs`

**Step 1: Add the workspace shell and failing calibration tests**

Add this workspace section without changing existing Tauri dependencies:

```toml
[workspace]
members = [
  ".",
  "crates/repose-unlock-core",
]
resolver = "3"
```

Start the integration test with explicit policy values:

```rust
use repose_unlock_core::calibration::{calibrate, CalibrationPolicy};

#[test]
fn rejects_overlapping_near_and_far_samples() {
    let policy = CalibrationPolicy::prototype();
    let near = [-61, -60, -62, -61, -60, -62, -61, -60];
    let far = [-62, -61, -63, -62, -61, -63, -62, -61];
    assert!(calibrate(&near, &far, &policy).is_err());
}

#[test]
fn outlier_does_not_expand_the_unlock_boundary() {
    let policy = CalibrationPolicy::prototype();
    let near = [-48, -49, -47, -48, -49, -47, -48, -90];
    let far = [-74, -75, -73, -74, -75, -73, -74, -20];
    let profile = calibrate(&near, &far, &policy).unwrap();
    assert!(profile.near_threshold_dbm > profile.far_threshold_dbm);
    assert!(profile.near_threshold_dbm - profile.far_threshold_dbm >= 8);
}
```

Also test insufficient samples, invalid RSSI, minimum separation, and deterministic output.

**Step 2: Run the focused test and verify red**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --test calibration
```

Expected: compilation fails because the crate/API does not exist.

**Step 3: Implement only the calibration and rolling proximity APIs**

Use newtypes for `MonoMillis`, `LockEpoch`, `AuditSessionId`, and `ConsoleUid`. Make `calibrate` use sorted medians/percentiles and explicit minimum separation. Make `ProximityFilter::push(sample, now)` emit `FarStable` or `NearStable` only after a full sample window and dwell period.

The crate root starts with:

```rust
#![forbid(unsafe_code)]

pub mod calibration;
pub mod domain;
pub mod proximity;
```

No wall-clock calls, Tauri types, BLE APIs, or global constants are allowed in this crate.

**Step 4: Run focused tests, clippy, and formatting**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --test calibration
cargo clippy --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
```

Expected: all pass.

**Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/crates/repose-unlock-core
git commit -m "feat: add proximity calibration core"
```

### Task 2: Implement the leave-and-return state machine

**Files:**

- Create: `src-tauri/crates/repose-unlock-core/src/state_machine.rs`
- Modify: `src-tauri/crates/repose-unlock-core/src/lib.rs`
- Test: `src-tauri/crates/repose-unlock-core/tests/state_machine.rs`
- Test: `src-tauri/crates/repose-unlock-core/tests/security_properties.rs`

**Step 1: Write failing transition tests**

Cover these invariants:

```rust
#[test]
fn phone_near_when_lock_occurs_never_starts_a_challenge() {
    let state = unlocked_fixture();
    let (state, effects) = transition(state, Event::SessionLocked(binding(7)), ms(10)).unwrap();
    let (_, effects) = transition(state, Event::NearStable, ms(20)).unwrap();
    assert!(!effects.contains(&Effect::StartChallenge));
}

#[test]
fn stable_far_then_near_starts_exactly_one_challenge() {
    let state = locked_unarmed_fixture();
    let (state, _) = transition(state, Event::FarStable, ms(20)).unwrap();
    let (_, effects) = transition(state, Event::NearStable, ms(30)).unwrap();
    assert_eq!(effects, vec![Effect::StartChallenge]);
}
```

Add tests for restart-while-locked, UID/audit-session change, stale epoch, cooldown, duplicate near events, unlock, logout, and fast-user-switch reset.

**Step 2: Verify red**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --test state_machine
```

Expected: unresolved `state_machine` symbols.

**Step 3: Implement a pure reducer**

Expose only:

```rust
pub fn transition(
    state: UnlockState,
    event: Event,
    now: MonoMillis,
) -> Result<(UnlockState, Vec<Effect>), TransitionError>;
```

`ChallengeVerified` must have private fields and be constructible only by the protocol verifier. A service restart while locked always yields `LockedUnarmed`; never persist `Armed`, `Challenging`, or `PermitReady`.

**Step 4: Add property tests and verify green**

Generate arbitrary valid event sequences and assert:

- no verified challenge means no `CreatePermit` effect;
- session change makes all prior challenge events inert;
- a lock epoch can issue at most one live challenge at a time.

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --test state_machine
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --test security_properties
```

**Step 5: Commit**

```bash
git add src-tauri/crates/repose-unlock-core
git commit -m "feat: enforce leave and return unlock state"
```

### Task 3: Add bounded wire protocol, cryptography, replay guard, and one-shot permits

**Files:**

- Create: `docs/protocol/repose-unlock-v1.md`
- Create: `protocol/fixtures/v1/crypto-vectors.json`
- Create: `protocol/fixtures/v1/challenge.bin`
- Create: `src-tauri/crates/repose-unlock-core/src/protocol/mod.rs`
- Create: `src-tauri/crates/repose-unlock-core/src/protocol/messages.rs`
- Create: `src-tauri/crates/repose-unlock-core/src/protocol/wire.rs`
- Create: `src-tauri/crates/repose-unlock-core/src/protocol/transcript.rs`
- Create: `src-tauri/crates/repose-unlock-core/src/protocol/crypto.rs`
- Create: `src-tauri/crates/repose-unlock-core/src/replay.rs`
- Create: `src-tauri/crates/repose-unlock-core/src/permit.rs`
- Test: `src-tauri/crates/repose-unlock-core/tests/protocol_vectors.rs`
- Test: `src-tauri/crates/repose-unlock-core/tests/replay.rs`
- Test: `src-tauri/crates/repose-unlock-core/tests/permit.rs`

**Step 1: Specify the v1 byte layout before implementation**

Use fixed-width, big-endian fields with a four-byte magic, one-byte version/kind, explicit bounded payload length, 32-byte nonces, 65-byte uncompressed SEC1 P-256 public keys, and 64-byte raw `r || s` signatures. Define domain-separated transcript labels and directional AEAD key/nonce derivation. Reject unknown versions, kinds, flags, trailing bytes, non-low-S signatures, and lengths over the exact message maximum.

**Step 2: Write failing golden-vector and mutation tests**

Tests load committed fixtures and assert byte-for-byte encoding, ECDH/HKDF vectors, decrypt round trips, and failure after changing any AAD, epoch, counter, tag, nonce, key, or signature byte.

Permit tests include concurrent consumption:

```rust
#[test]
fn concurrent_consumers_get_exactly_one_success() {
    let store = shared_store_with_permit(binding(9), epoch(4), ms(100), ms(3_000));
    let successes = run_100_consumers(store, binding(9), epoch(4), ms(101));
    assert_eq!(successes, 1);
}
```

**Step 3: Verify red**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --test protocol_vectors
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --test replay
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --test permit
```

**Step 4: Add minimal dependencies and implementation**

Use RustCrypto P-256/ECDSA/ECDH, HKDF-SHA256, AES-GCM, and zeroization. Challenge expiry is evaluated only against Mac-local monotonic time. Replay state uses a compare-and-swap intent: a verified response may create a permit only after the new counter is durably committed.

`PermitStore::consume` checks binding and expiry and takes the value under one non-poisoning synchronization primitive. A wrong binding must not destroy a still-valid permit; an expired or session-invalid permit is destroyed.

**Step 5: Verify green and all core invariants**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml -p repose-unlock-core --all-targets -- -D warnings
```

**Step 6: Commit**

```bash
git add docs/protocol protocol src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/crates/repose-unlock-core
git commit -m "feat: add secure unlock protocol core"
```

### Task 4: Make authorization policy changes pure, surgical, and reversible

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/crates/repose-authdb-policy/Cargo.toml`
- Create: `src-tauri/crates/repose-authdb-policy/src/lib.rs`
- Create: `src-tauri/crates/repose-authdb-policy/src/transform.rs`
- Create: `src-tauri/crates/repose-authdb-policy/tests/transform.rs`
- Create: `src-tauri/crates/repose-authdb-policy/tests/fixtures/authorizationdb/*.plist`
- Create: `src-tauri/crates/repose-authdb-policy/tests/fixtures/authorizationdb/README.md`

**Step 1: Write failing fixture tests**

Test stock string/array rules, a `k-of-n = 1` rule with a third-party candidate, already-installed state, missing fallback, duplicate Repose entries, malformed plist, wrong class, and `k-of-n != 1`.

The API must be impossible to point at a different right:

```rust
let installed = ScreenSaverPolicy::parse(input)?.install(&PolicySpec::v1())?;
assert!(installed.has_exactly_one_password_fallback());
assert_eq!(installed.repose_candidate_index() + 1, installed.password_fallback_index());
```

**Step 2: Verify red**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-authdb-policy
```

**Step 3: Implement parse/install/remove/verify**

Preserve unknown plist keys and third-party candidate order. Install Repose immediately before the one `use-login-window-ui` fallback. Repeated install is idempotent. Remove only Repose. Never expose an arbitrary authorization-right string and never replace current policy with a stale backup during normal removal.

**Step 4: Verify green**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-authdb-policy
cargo clippy --manifest-path src-tauri/Cargo.toml -p repose-authdb-policy --all-targets -- -D warnings
```

**Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/crates/repose-authdb-policy
git commit -m "feat: add safe screensaver authorization policy"
```

### Task 5: Build the permit broker and fixed-frame macOS IPC prototype

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Create: `docs/protocol/repose-unlock-ipc-v1.md`
- Create: `src-tauri/crates/repose-unlock-ipc/Cargo.toml`
- Create: `src-tauri/crates/repose-unlock-ipc/src/lib.rs`
- Create: `src-tauri/crates/repose-unlock-ipc/include/repose_unlock_ipc.h`
- Create: `src-tauri/crates/repose-unlock-ipc/tests/wire_vectors.rs`
- Create: `src-tauri/crates/repose-unlock-service/Cargo.toml`
- Create: `src-tauri/crates/repose-unlock-service/build.rs`
- Create: `src-tauri/crates/repose-unlock-service/src/lib.rs`
- Create: `src-tauri/crates/repose-unlock-service/src/main.rs`
- Create: `src-tauri/crates/repose-unlock-service/src/permit_broker.rs`
- Create: `src-tauri/crates/repose-unlock-service/src/ipc_server.rs`
- Create: `src-tauri/crates/repose-unlock-service/src/launchd.rs`
- Create: `src-tauri/crates/repose-unlock-service/src/peer_identity.rs`
- Create: `src-tauri/crates/repose-unlock-service/native/listener_identity.c`
- Create: `src-tauri/crates/repose-unlock-service/native/peer_identity.c`
- Test: `src-tauri/crates/repose-unlock-service/tests/consume_ipc.rs`
- Test: `src-tauri/crates/repose-unlock-service/tests/watch_interrupt.rs`
- Test: `src-tauri/crates/repose-unlock-service/tests/peer_rejection.rs`

**Step 1: Write failing wire and broker tests**

Cover partial frames, oversized lengths, unknown operations, wrong nonce/session, service restart, watcher/consume races, and 100 concurrent consumers. Use barriers or explicit test hooks—not scheduler sleeps—to force both publish/consume and session-or-restart/consume linearization orders. Also force inverse thread arrival to prove monotonic time is sampled only after entering the broker lock. Test `consume_or_watch` as one atomic broker operation so an event cannot occur between lookup and subscription.

**Step 2: Verify red**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-ipc
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-service
```

**Step 3: Implement the test transport and production peer verifier**

Use a temporary AF_UNIX socket in tests. Production never binds or unlinks `/var/run/ai.repose.unlockd/consume.sock`; launchd owns that node, and the service only validates the inherited listener against the fixed packaged path and ownership/mode contract. All connect/read/write operations use nonblocking I/O and one absolute 100 ms deadline. Production macOS peer verification obtains `LOCAL_PEERTOKEN`, checks euid/audit session, and validates this designated requirement with Security.framework:

```text
identifier "com.apple.authorizationhost" and anchor apple
```

The peer is Apple `authorizationhost`, not the Repose plugin bundle. Test builds inject a `PeerVerifier`; production builds cannot select `AllowAll`.

Task 5's `src/main.rs` production entry is intentionally an authenticated deny-only/offline gate: it activates and validates the launchd listener, constructs the concrete production peer verifier, parses only strictly bounded requests, and closes without returning `CONSUMED` or `WATCHING`. It does **not** connect the prototype `PermitBroker` to a production durable/session runtime and must not be described as enabling phone unlock. A dedicated-Mac Task 8 validation build may wire that private runtime boundary for the feasibility tests; the real broker path must remain disabled for ordinary installation until Task 8 records a passing authorization, fallback, and keychain gate.

**Step 4: Verify green and sanitizable boundaries**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-ipc
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlock-service
cargo clippy --manifest-path src-tauri/Cargo.toml -p repose-unlock-service --all-targets -- -D warnings
```

**Step 5: Commit**

```bash
git add docs/plans/2026-09-07-phone-proximity-unlock.md docs/protocol/repose-unlock-ipc-v1.md src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/crates/repose-unlock-ipc src-tauri/crates/repose-unlock-service
git commit -m "feat: add one-shot unlock permit broker"
```

### Task 6: Build and ABI-test the minimal Authorization Plugin

**Files:**

- Create: `native/macos/authorization-plugin/Makefile`
- Create: `native/macos/authorization-plugin/Info.plist`
- Create: `native/macos/authorization-plugin/src/plugin.c`
- Create: `native/macos/authorization-plugin/src/ipc_client.c`
- Create: `native/macos/authorization-plugin/src/ipc_client.h`
- Create: `native/macos/authorization-plugin/src/deadline.c`
- Create: `native/macos/authorization-plugin/src/deadline.h`
- Create: `native/macos/authorization-plugin/src/plugin_test_support.h`
- Create: `native/macos/authorization-plugin/tests/fake_authorization_engine.c`
- Create: `native/macos/authorization-plugin/tests/fake_authorization_engine.h`
- Create: `native/macos/authorization-plugin/tests/ipc_client_test.c`
- Create: `native/macos/authorization-plugin/tests/plugin_lifecycle_test.c`
- Create: `native/macos/authorization-plugin/tests/release_api_compile.c`
- Create: `native/macos/authorization-plugin/tests/test_server.c`
- Create: `native/macos/authorization-plugin/tests/test_server.h`
- Create: `native/macos/authorization-plugin/tests/bundle_smoke.c`
- Create: `native/macos/authorization-plugin/.gitignore`
- Modify: `.gitignore`
- Create: `scripts/build-macos-auth-prototype.sh`
- Create: `scripts/verify-macos-auth-artifacts.sh`

**Step 1: Write the failing callback harness**

The fake `AuthorizationCallbacks` records `SetResult`, `RequestInterrupt`, and `DidDeactivate`. Test:

- a ready permit maps to exactly one `Allow`;
- no service, timeout, malformed response, stale session, and no permit map to `Deny` within 100 ms;
- no ordinary failure maps to `Undefined` or `UserCanceled`;
- an event arriving before/after initial `Deny` requests at most one interrupt;
- deactivation closes the watcher and calls `DidDeactivate`;
- destroy during a pending event has no use-after-free.

The exactly-once `SetResult` contract applies to valid invocations admitted
before deactivation starts. An invalid host call that races after deactivation
or destruction has begun returns `errAuthorizationDenied` without accessing the
engine callbacks; this preserves the stronger rule that no callback occurs
after `DidDeactivate` or `MechanismDestroy` returns.

**Step 2: Verify red**

```bash
make -C native/macos/authorization-plugin test
```

Expected: missing sources/targets.

**Step 3: Implement the plugin lifecycle and bundle**

Export only `AuthorizationPluginCreate`. `MechanismInvoke` calls `CONSUME_OR_WATCH`; it never waits for the three-second permit window. The one-shot watcher may call `RequestInterrupt` once, but a second invocation still has to atomically consume a permit before returning `Allow`.

Use `SO_NOSIGPIPE`, monotonic deadlines, bounded frames, and explicit ownership for watcher state. Plugin code does not contain BLE, JSON, file writes, UI, or network access.

The bundle mechanism identifier is `unlock`. Task 7 must install it through a
named authorization rule using `ReposeUnlock:unlock,privileged` (rule name
`ai.repose.unlock`); Task 6 does not read or modify the authorization database.

**Step 4: Verify harness, sanitizers, and bundle ABI**

```bash
make -C native/macos/authorization-plugin test
make -C native/macos/authorization-plugin test SANITIZE=address,undefined
make -C native/macos/authorization-plugin test SANITIZE=thread
./scripts/build-macos-auth-prototype.sh --configuration debug --arch arm64 --sign ad-hoc
./scripts/verify-macos-auth-artifacts.sh target/macos-auth/debug
```

Expected: lifecycle tests pass; bundle has one arm64 executable, valid Info.plist, ad-hoc signature, minimum macOS 14, and the one required exported symbol.

**Step 5: Commit**

```bash
git add .gitignore docs/plans/2026-09-07-phone-proximity-unlock-design.md docs/plans/2026-09-07-phone-proximity-unlock.md docs/protocol/repose-unlock-ipc-v1.md native/macos/authorization-plugin scripts/build-macos-auth-prototype.sh scripts/verify-macos-auth-artifacts.sh
git commit -m "feat: add macOS authorization plugin prototype"
```

### Task 7: Add safe install, status, repair, and uninstall tooling without applying it

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `src-tauri/crates/repose-authdb-policy/src/transform.rs`
- Modify: `src-tauri/crates/repose-unlock-service/src/main.rs`
- Test: `src-tauri/crates/repose-unlock-service/tests/deny_only_health.rs`
- Create: `src-tauri/crates/repose-unlockctl/Cargo.toml`
- Create: `src-tauri/crates/repose-unlockctl/build.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/lib.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/main.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/cli.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/authdb.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/install_transaction.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/install_transaction/install.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/install_transaction/policy.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/install_transaction/uninstall.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/install_transaction/repair.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/install_transaction/recovery.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/artifact_verify.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/artifact_verify/package_fs.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/production.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/production/attestation.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/production/components.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/production/fs_state.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/production/journal.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/production/plugin_fs.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/production/process.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/production/receipt.rs`
- Create: `src-tauri/crates/repose-unlockctl/src/production/tests.rs`
- Create: `src-tauri/crates/repose-unlockctl/native/authorization_db.m`
- Create: `src-tauri/crates/repose-unlockctl/native/authorization_db_fake_test.m`
- Test: `src-tauri/crates/repose-unlockctl/tests/artifact_failures.rs`
- Test: `src-tauri/crates/repose-unlockctl/tests/command_safety.rs`
- Test: `src-tauri/crates/repose-unlockctl/tests/native_adapter_contract.rs`
- Test: `src-tauri/crates/repose-unlockctl/tests/native_adapter_runtime.rs`
- Test: `src-tauri/crates/repose-unlockctl/tests/package_script_contract.rs`
- Test: `src-tauri/crates/repose-unlockctl/tests/transaction_failures.rs`
- Create: `native/macos/launchd/ai.repose.unlockd.plist`
- Create: `native/macos/tests/real-machine-checklist.md`
- Create: `scripts/package-macos-auth-components.sh`
- Modify: `scripts/verify-macos-auth-artifacts.sh`

**Step 1: Write failing transaction tests**

Use a fake filesystem, fake Authorization Services adapter, and injected failpoints after every step. Assert rollback preserves third-party rules, a failed policy-removal step keeps the plugin/service installed, and `system.login.console` is never requested.

**Step 2: Verify red**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p repose-unlockctl
```

**Step 3: Implement commands and two-phase ordering**

Commands:

```text
repose-unlockctl status
repose-unlockctl plan-install --artifacts <dir>
repose-unlockctl install --artifacts <dir> --apply
repose-unlockctl plan-uninstall
repose-unlockctl uninstall --apply
repose-unlockctl repair --apply --backup <explicit-path>
```

Default behavior is read-only planning. `--apply` requires root and calls only the fixed `system.login.screensaver` and `ai.repose.unlock` operations through the Objective-C `AuthorizationRightGet/Set/Remove` adapter; it never edits database files directly and never names `system.login.console`. Install policy last; uninstall policy first. A root-owned, `O_NOFOLLOW` single-writer lock and fsync'd phase journal make interruption recovery idempotent. Authorization updates transform the latest parseable live value and use structural readback to detect observable drift. Authorization Services has no compare-and-swap, however, so this is not an atomic guarantee against another privileged writer between the last read and write. Opening the Task 8 gate therefore also requires a dedicated maintenance window and exclusive operator serialization; all production mutations remain closed in Task 7.

Task 7 packages are ad-hoc signed and **plan-only**. Their local `SHA256SUMS` detects damage but is not publisher authenticity, and the package verifier never executes a caller-supplied helper. Every production mutation (`install`, `uninstall`, and `repair`) is deliberately hard-gated before backend construction, lock/journal, Authorization Services, launchd, or target-file access until Task 8 pins a signed manifest, Developer ID/designated requirements, exact deny-only service measurement, package/protocol generation, OS/architecture bounds, downgrade rules, and an fd-stable sealed staging implementation that validates ACLs, xattrs/resource forks, and file flags. The helper health command is only bounded liveness sanity after a trusted installed measurement; it is never deny-only attestation.

The recoverable ordering is: durable backup/journal; prove password-only policy; atomically commit and verify one component generation; persist its install receipt and durably record the returned target fingerprint before any policy reference; then set/read back the exact named rule, set/read back the screensaver candidate last, and recheck the receipt-bound loaded target as the final dependency read. Recovery never treats an arbitrary `Trusted` receipt as the requested target: a receipt effect without its target journal marker, or a different fingerprint, is disabled and rolled back. Upgrade first disables the candidate and journals the exact prior receipt. Uninstall precisely removes/read-backs the candidate and named rule before stopping or removing components; the expected prior fingerprint is revalidated inside each stop/quarantine operation so a stale outer read cannot authorize a new generation. It then uses same-parent quarantine rename, post-rename verification, unlink, and parent-directory fsync. The receipt is removed last. Repair recovery completes an already-active exact closure, but any ambiguous pre-terminal or failed-abort marker converges to password-only while retaining dependencies for an explicit retry. When rollback cannot prove the candidate inactive or detects drift, it retains dependencies and requires explicit repair rather than creating a dangling authorization reference.

Task 7 tests the backend-independent transaction state machine at before/after-effect cutpoints plus temp-root `O_NOFOLLOW` lock, checksum-bound backup, receipt, atomic-writer, exact-tree, and quarantine behavior. The production syscall layer does not yet expose deterministic injection for every short write, rename, file/directory fsync, `EXDEV`, launchctl timeout/permission/not-found classification, or crash between those syscalls. Those operation-by-syscall tests, an authoritative loaded-job service adapter, and receipt creation from a trusted sealed generation are explicit Task 8 gate-opening blockers; the closed gate makes those incomplete production paths unreachable in Task 7.

**Step 4: Verify all nonprivileged tests and package structure**

```bash
cargo test --offline --manifest-path src-tauri/Cargo.toml -p repose-unlockctl
./scripts/package-macos-auth-components.sh --unsigned --output target/macos-auth/package
./scripts/verify-macos-auth-artifacts.sh target/macos-auth/package
```

Do not run `install --apply` on this host.

**Step 5: Commit**

```bash
git add docs/plans/2026-09-07-phone-proximity-unlock-design.md docs/plans/2026-09-07-phone-proximity-unlock.md
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/crates/repose-authdb-policy/src/transform.rs
git add src-tauri/crates/repose-unlock-service src-tauri/crates/repose-unlockctl
git add native/macos/launchd native/macos/tests scripts/package-macos-auth-components.sh scripts/verify-macos-auth-artifacts.sh
git commit -m "feat: add recoverable unlock component installer"
```

### Task 8: Run the dedicated-Mac authorization feasibility gate — BLOCKED

> Status on 2026-09-08: **NOT RUN / GATE CLOSED**. A read-only inventory of the current
> development Mac is recorded in `docs/validation/macos-authorization-results.md`. This is not a
> dedicated/restoreable test Mac, and the second-admin, remote-recovery, FileVault-recovery, tested
> restore, signing, and production-measurement prerequisites are unresolved. No Authorization
> Services, install, launchd, lock-screen, latency, password, keychain, repair, or uninstall test was
> run. Steps 2–5 remain future work on a qualified test Mac.

**Files:**

- Modify: `docs/plans/2026-09-07-phone-proximity-unlock.md`
- Modify: `native/macos/tests/real-machine-checklist.md`
- Create: `docs/validation/macos-authorization-results.md`

**Step 1: Prepare, but do not automate, the external safety prerequisites**

Complete every item in `native/macos/tests/real-machine-checklist.md` under **Hard stop before any
installation** before opening any gate: dedicated hardware identity and exact OS/build; rehearsed
erase/restore; second-admin local login; tested remote recovery; independently verified FileVault
recovery; tested backup restore; a recovery channel that survives normal power and network removal;
the checksum-bound policy backup; and a signed offline repair tool and procedure. No subset of the
Hard stop section is sufficient.

**Step 2: Install a permanently deny-only build on the test Mac**

Verify at least 20 password unlocks. No-permit added latency must have p95 below 250 ms and no sample above 500 ms. Kill/hang/remove each service component and repeat fallback.

**Step 3: Test the two unattended paths**

- Permit ready before initial `MechanismInvoke`.
- Password UI already visible, then permit becomes ready and the watcher calls `RequestInterrupt` once.
- Repeat with no keyboard/mouse input and after sleep wake.

**Step 4: Test keychain and scope boundaries**

After an automatic unlock, verify login keychain, saved passwords, SSH keys, and browser credentials. Verify reboot, logout, FileVault, Guest, fast-user-switching, and remote login never take the Repose path.

**Step 5: Record the gate**

Write exact OS/build, hashes, timings, failures, and logs to `docs/validation/macos-authorization-results.md`. If unattended re-evaluation, fallback, or login keychain fails, mark the product gate failed and do not enable installation from Repose.

**Step 6: Commit the closed-gate record**

```bash
git add docs/plans/2026-09-07-phone-proximity-unlock.md
git add native/macos/tests/real-machine-checklist.md docs/validation/macos-authorization-results.md
git commit -m "docs: record closed macOS authorization gate"
```

When the prerequisites are later satisfied, record real-machine evidence in a separate commit.
Do not replace `NOT RUN / GATE CLOSED` with a passing result until every claimed experiment has
actually run and its logs, hashes, and timings are attached.

### Task 9: Scaffold the Flutter companion and test the shared UI state

**Files:**

- Create: `mobile/.fvmrc`
- Create: `mobile/pubspec.yaml` and generated Flutter platform shell
- Create: `mobile/lib/main.dart`
- Create: `mobile/lib/app/repose_unlock_app.dart`
- Create: `mobile/lib/native/native_gateway.dart`
- Create: `mobile/lib/native/native_models.dart`
- Create: `mobile/lib/features/pairing/pairing_controller.dart`
- Create: `mobile/lib/features/calibration/calibration_controller.dart`
- Create: `mobile/lib/features/devices/device_controller.dart`
- Test: `mobile/test/pairing_controller_test.dart`
- Test: `mobile/test/calibration_controller_test.dart`
- Test: `mobile/test/capability_gate_test.dart`

**Step 1: Generate only the Flutter shell**

```bash
flutter create --org ai.repose --project-name repose_unlock --platforms android,ios mobile
```

Pin Flutter `3.38.9` in `.fvmrc`. Do not put keys, BLE, or a raw `sign(bytes)` API in Dart.

**Step 2: Write failing controller tests**

Test QR expiry, explicit first-pair confirmation, calibration step order, overlapping-sample rejection display, capability-unavailable states, revocation, and native snapshot hydration after UI restart.

The gateway surface is restricted to domain commands:

```dart
abstract interface class NativeGateway {
  Future<UnlockSnapshot> getSnapshot();
  Future<PairingSession> beginPairing(String qrPayload);
  Future<void> confirmPairing(String sessionId);
  Future<void> startCalibration();
  Future<void> submitCalibrationStep(CalibrationStep step);
  Future<void> revokeDevice(String deviceId);
  Future<Diagnostics> getDiagnostics();
}
```

**Step 3: Verify red, implement minimal reducers/UI, verify green**

```bash
cd mobile
flutter test test/pairing_controller_test.dart
flutter test test/calibration_controller_test.dart
flutter test test/capability_gate_test.dart
flutter analyze
```

Expected after implementation: all pass with no analyzer warnings.

**Step 4: Commit**

```bash
git add mobile
git commit -m "feat: add Flutter unlock companion shell"
```

### Task 10: Implement the Android 16 native runtime test-first

**Files:**

- Create: `mobile/packages/repose_unlock_native/pubspec.yaml`
- Create: `mobile/packages/repose_unlock_native/pigeons/repose_unlock_api.dart`
- Create: generated Pigeon Dart/Kotlin bindings
- Create: `mobile/packages/repose_unlock_native/android/src/main/AndroidManifest.xml`
- Create: `mobile/packages/repose_unlock_native/android/src/main/kotlin/ai/repose/mobile/unlock/ReposeUnlockRuntime.kt`
- Create: `mobile/packages/repose_unlock_native/android/src/main/kotlin/ai/repose/mobile/unlock/companion/ReposeCompanionDeviceService.kt`
- Create: `mobile/packages/repose_unlock_native/android/src/main/kotlin/ai/repose/mobile/unlock/companion/CompanionAssociationManager.kt`
- Create: `mobile/packages/repose_unlock_native/android/src/main/kotlin/ai/repose/mobile/unlock/crypto/AndroidKeyStoreSigner.kt`
- Create: `mobile/packages/repose_unlock_native/android/src/main/kotlin/ai/repose/mobile/unlock/protocol/*.kt`
- Test: `mobile/packages/repose_unlock_native/android/src/test/kotlin/.../PresenceEventRouterTest.kt`
- Test: `mobile/packages/repose_unlock_native/android/src/test/kotlin/.../AssociationReconcilerTest.kt`
- Test: `mobile/packages/repose_unlock_native/android/src/test/kotlin/.../ContractVectorTest.kt`
- Test: `mobile/packages/repose_unlock_native/android/src/androidTest/kotlin/.../AndroidKeyStoreSignerTest.kt`

**Step 1: Install the API 36 SDK prerequisite**

Install Android command-line tools, then:

```bash
sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0"
```

Set `compileSdk = 36`, `targetSdk = 36`, and `minSdk = 31`. Devices below API 36 report unsupported for the automatic presence feature; do not silently select polling.

**Step 2: Write failing JVM tests and manifest assertions**

Test only an association-ID request is built, unknown/mismatched/duplicate presence events are ignored, startup reconciles `getMyAssociations()`, and callback routing never starts Flutter. Validate there is exactly one primary `CompanionDeviceService` protected by `BIND_COMPANION_DEVICE_SERVICE`.

**Step 3: Verify red**

```bash
cd mobile/android
./gradlew testDebugUnitTest --tests '*PresenceEventRouterTest' --tests '*AssociationReconcilerTest'
```

**Step 4: Implement minimal API 36 native behavior**

Use `ObservingDevicePresenceRequest.Builder().setAssociationId(id)`. `onDevicePresenceEvent()` only validates and enqueues to the native runtime. GATT, persistence, replay, and crypto run off the main thread and without a Flutter engine.

Generate a P-256 signing key with SHA-256 and `setUserAuthenticationRequired(false)`. Require TEE or StrongBox when the device reports it; never export a private key and never enable `setUnlockedDeviceRequired(true)`.

**Step 5: Verify JVM, lint, and instrumentation tests**

```bash
cd mobile/android
./gradlew testDebugUnitTest lintDebug assembleDebug
./gradlew connectedDebugAndroidTest
```

Instrumentation requires an API 36 emulator or attached GT5 Pro. Assert `privateKey.encoded == null` and record actual `KeyInfo.securityLevel`.

**Step 6: Commit**

```bash
git add mobile
git commit -m "feat: add Android companion presence runtime"
```

### Task 11: Prototype both BLE roles on the GT5 Pro

**Files:**

- Create: `mobile/packages/repose_unlock_native/android/src/main/kotlin/ai/repose/mobile/unlock/bluetooth/BleRoleStrategy.kt`
- Create: `mobile/packages/repose_unlock_native/android/src/main/kotlin/ai/repose/mobile/unlock/bluetooth/MacCentralPhonePeripheralStrategy.kt`
- Create: `mobile/packages/repose_unlock_native/android/src/main/kotlin/ai/repose/mobile/unlock/bluetooth/MacPeripheralPhoneCentralStrategy.kt`
- Test: corresponding Kotlin unit tests
- Create: `docs/validation/android-gt5-pro-results.md`

**Step 1: Write failing transport-state tests**

For both strategies test partial GATT frames, reconnect, Bluetooth-off, stale association, process recreation, duplicate callback, and bounded retry/cooldown. Dart must not know which role is active.

**Step 2: Implement the minimal strategies and contract fixtures**

Use the v1 protocol fixtures from `protocol/fixtures/v1`. No raw signature or arbitrary GATT write operation is exposed to Dart.

**Step 3: Verify automatic tests**

```bash
cd mobile/android
./gradlew testDebugUnitTest lintDebug assembleDebug
```

**Step 4: Run the GT5 Pro matrix**

Connect the phone and verify manufacturer/model/API/features, then test screen-off, Flutter UI closed, `am kill`, force-stop, Doze, Bluetooth toggle, reboot-before-first-unlock, reboot-after-first-unlock, calibration in a pocket/bag, and 30 leave/return cycles. Force-stop is expected to disable background behavior until the user opens the app; the Mac must fall back to password.

**Step 5: Select a role only from recorded evidence**

Record RSSI ownership, wake latency, p50/p95 challenge latency, battery impact, false-near events, and OEM settings in `docs/validation/android-gt5-pro-results.md`. Do not claim the three-second target without these measurements.

**Step 6: Commit**

```bash
git add mobile docs/validation/android-gt5-pro-results.md
git commit -m "test: validate GT5 Pro proximity transport"
```

### Task 12: Implement the iOS native runtime when the supported Xcode host exists

**Files:**

- Create: `mobile/packages/repose_unlock_native/ios/repose_unlock_native.podspec`
- Create: `mobile/packages/repose_unlock_native/ios/repose_unlock_native/Package.swift`
- Create: `mobile/packages/repose_unlock_native/ios/repose_unlock_native/Sources/repose_unlock_native/ReposeUnlockBootstrap.swift`
- Create: `mobile/packages/repose_unlock_native/ios/repose_unlock_native/Sources/repose_unlock_native/accessory/AccessorySessionCoordinator.swift`
- Create: `mobile/packages/repose_unlock_native/ios/repose_unlock_native/Sources/repose_unlock_native/bluetooth/CoreBluetoothRuntime.swift`
- Create: `mobile/packages/repose_unlock_native/ios/repose_unlock_native/Sources/repose_unlock_native/crypto/SecureEnclaveSigner.swift`
- Create: `mobile/packages/repose_unlock_native/ios/repose_unlock_native/Sources/repose_unlock_native/protocol/*.swift`
- Modify: `mobile/ios/Runner/AppDelegate.swift`
- Modify: `mobile/ios/Runner/Info.plist`
- Test: `mobile/packages/repose_unlock_native/ios/repose_unlock_native/Tests/ReposeUnlockNativeTests/*.swift`
- Create: `docs/validation/ios-background-results.md`

**Step 1: Upgrade to a macOS/Xcode combination that includes the iOS 26 SDK**

Verify with `xcodebuild -version` and `xcrun --sdk iphoneos --show-sdk-version`. This task cannot be compiled on the current macOS 14.6.1 Command Line Tools host.

**Step 2: Write failing pure Swift tests**

Test restoration-dictionary reduction, accessory add/remove routing, protocol vectors, replay/counter persistence, key-unavailable behavior, unknown event rejection, and runtime startup before Flutter registration.

**Step 3: Implement the native bootstrap**

`AppDelegate.application(_:didFinishLaunchingWithOptions:)` starts `ReposeUnlockBootstrap` before Flutter plugin registration. AccessorySetupKit only performs foreground user-authorized setup. Core Bluetooth owns restoration and transport without Flutter. Keychain metadata uses `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; signing keys do not require per-use biometrics and never leave Secure Enclave/Keychain.

Resolve the correct AccessorySetupKit Info.plist keys against the installed iOS 26 SDK rather than copying inconsistent web documentation.

**Step 4: Run simulator build and unit tests**

```bash
cd mobile
flutter build ios --simulator --debug --no-codesign
xcrun simctl list devices available
xcodebuild -workspace ios/Runner.xcworkspace -scheme Runner -destination '<available iOS 26 simulator>' test
```

**Step 5: Run the required iPhone matrix**

Test foreground, background, suspended, system-terminated, user force-quit, Bluetooth-off, reboot-before-first-unlock, reboot-after-first-unlock, and real Core Bluetooth state restoration. Record results and platform fallbacks.

**Step 6: Commit**

```bash
git add mobile docs/validation/ios-background-results.md
git commit -m "feat: add iOS background unlock runtime"
```

### Task 13: Integrate health, pairing, calibration, and revocation into Repose

**Files:**

- Create: `src/lib/unlock.ts`
- Test: `src/lib/unlock.test.ts`
- Modify: `src/tauriBridge.ts`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`
- Modify: `src-tauri/src/lib.rs` only for narrow Tauri commands; do not touch timer transitions
- Modify: `src-tauri/capabilities/default.json`

**Step 1: Write failing TypeScript model tests**

Test health snapshots, unsupported/failed gate messaging, install disabled without a passing authorization validation record, QR expiry, calibration progress, and revocation. Keep these as pure functions so Node tests do not need a DOM.

**Step 2: Verify red**

```bash
node --import tsx --test src/lib/unlock.test.ts
```

**Step 3: Add narrow commands and UI**

Expose only `unlock_status`, `begin_pairing`, `confirm_pairing`, `begin_calibration`, `revoke_device`, and `open_unlock_diagnostics`. Do not expose shell command execution, raw signing, arbitrary paths, arbitrary authorization rights, or raw BLE frames.

The settings panel shows component health, paired phone, calibration state, known platform limitations, relay/stolen-phone risk, and password fallback. Installation remains unavailable until macOS authorization gate evidence is marked passing.

**Step 4: Verify app tests and builds**

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
```

**Step 5: Commit**

```bash
git add src src-tauri package.json package-lock.json
git commit -m "feat: add proximity unlock settings"
```

### Task 14: Final verification, documentation, and release gate

**Files:**

- Modify: `README-rust.md`
- Create: `docs/phone-unlock.md`
- Create: `docs/security/phone-unlock-threat-model.md`
- Create: `docs/validation/verification-summary.md`
- Modify: `.gitignore` as needed for generated secrets/build artifacts only

**Step 1: Document user behavior and recovery**

Document supported scope, pairing, calibration, Android/iOS background limitations, password fallback, lost-phone revocation, uninstall/repair, BLE relay risk, logs, and the fact that force-stopping a phone app disables automatic response.

**Step 2: Run the complete automated suite from a clean build state**

```bash
npm ci
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets
make -C native/macos/authorization-plugin test SANITIZE=address,undefined
./scripts/build-macos-auth-prototype.sh --configuration release --arch arm64 --sign ad-hoc
./scripts/verify-macos-auth-artifacts.sh target/macos-auth/release
cd mobile && flutter analyze && flutter test
```

Run Android compilation/instrumentation when SDK 36 and a device/emulator are present; run iOS tests only on the upgraded Xcode host.

**Step 3: Audit the diff for scope and secrets**

```bash
git diff 6a5b60e -- src/hooks/useBreakTimer.ts src/lib/timer.ts src/lib/timer.test.ts
git grep -n -I -E '(PRIVATE KEY|pairingSecret|sessionKey|recovery key)'
git status --short
```

Expected: no timer lifecycle diff, no committed secret material, no untracked generated security artifacts.

**Step 4: Write evidence-based completion status**

`docs/validation/verification-summary.md` separates:

- implemented and automatically verified;
- compiled but not loaded into system authorization;
- tested on GT5 Pro;
- tested on iPhone;
- tested on each macOS version;
- blocked by missing hardware, signing, Xcode, or dedicated test environment.

Do not call the feature production-ready until every system/hardware row passes.

**Step 5: Commit**

```bash
git add README-rust.md docs .gitignore
git commit -m "docs: add phone unlock security and validation guide"
```

## Release and MR preparation

Before creating a GitLab MR, push this branch and use the design document as the required relevant User Story/Tech Design link:

```markdown
## 相关文档
- User Story文档（必填）：https://gitlab.example.com/<group>/<project>/-/blob/codex/phone-proximity-unlock/docs/plans/2026-09-07-phone-proximity-unlock-design.md
- Tech Design文档：https://gitlab.example.com/<group>/<project>/-/blob/codex/phone-proximity-unlock/docs/plans/2026-09-07-phone-proximity-unlock.md
```

Resolve the real GitLab repository URL from `git remote -v`; do not invent it, and never use the forbidden `skip-doc-check` label.
