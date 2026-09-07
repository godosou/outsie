# Lifecycle Timer Semantics Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Repose count focus only while the macOS session and app are active, while treating lock/sleep as an idempotent passive or active break interval.

**Architecture:** The TypeScript timer remains the business state machine but advances only from explicit monotonic elapsed values. A testable Rust lifecycle gate merges native macOS lock/sleep/session events into uniquely identified intervals measured by a sleep-aware monotonic clock, and the React hook coordinates delivery, persistence, and acknowledgements.

**Tech Stack:** TypeScript 7, Node test runner, React 19, Tauri 2, Rust 2024, AppKit/CoreGraphics Objective-C bridge.

---

## Preconditions

- Work in branch `codex/lifecycle-timer-semantics`.
- Follow `@superpowers:test-driven-development` for every behavior change.
- Reuse the dependency-complete checkout's `node_modules` through a local ignored symlink when needed; do not modify the other checkout.
- Keep every `docs/plans/` file below 400 lines.

### Task 1: Replace wall-clock advancement with explicit elapsed time

**Files:**

- Modify: `src/lib/timer.test.ts`
- Modify: `src/lib/timer.ts`

**Step 1: Write failing tests**

Add tests that express the new API before it exists:

```ts
test('explicit elapsed advances focus independently of wall-clock jumps', () => {
  let state = createTimerState(START)
  state = advanceTimerBy(state, 10, START + 60 * 60 * 1000)
  assert.equal(state.remaining, 1190)
  assert.equal(getTodayStats(state, START).focusSeconds, 10)
  state = advanceTimerBy(state, 5, START - 60 * 60 * 1000)
  assert.equal(state.remaining, 1185)
})

test('restoring a snapshot never replays closed-app time', () => {
  const saved = advanceTimerBy(createTimerState(START), 10, START + 10_000)
  const restored = restoreTimerState(JSON.stringify(saved), START + 60_000)
  assert.equal(restored.remaining, 1190)
})
```

**Step 2: Verify RED**

Run: `npm test`

Expected: FAIL because `advanceTimerBy` is not exported.

**Step 3: Implement the minimal elapsed API**

- Add `advanceTimerBy(state, elapsedSeconds, wallNow)`.
- Validate elapsed as finite and non-negative.
- Cap the current phase at its remaining duration and discard surplus after a phase transition.
- Record exactly the capped elapsed seconds; use wall time only to choose date buckets.
- Remove restore-time catch-up and `MAX_RESTORE_GAP_MS`.
- Make actions operate on an already-current state instead of silently advancing from wall time.

**Step 4: Migrate existing tests and callers in the test file**

- Replace wall timestamps passed as elapsed with explicit seconds.
- Explicitly advance before pause, settings, reset, postpone, skip, and completion actions where the old test relied on implicit advancement.
- Preserve all existing product assertions except the obsolete five-minute restore heuristic.

**Step 5: Verify GREEN**

Run: `npm test`

Expected: all timer tests pass.

**Step 6: Commit**

```bash
git add src/lib/timer.ts src/lib/timer.test.ts
git commit -m "refactor: drive timer with explicit elapsed time"
```

### Task 2: Add passive-rest and completion idempotency rules

**Files:**

- Modify: `src/lib/timer.test.ts`
- Modify: `src/lib/timer.ts`

**Step 1: Write failing tests for focus inactivity**

Cover:

- Short rest below, equal to, and above threshold.
- Long rest selected by the current cycle.
- Deferred short/long rest keeps its `breakId` and used postponement.
- Paused focus does not record or complete passive rest.
- Actual `breakSeconds` can exceed the completion threshold while `completedBreaks` increases once.

Use the intended API:

```ts
const context = captureInactivity(state)
const result = applyInactivityInterval(state, context, {
  intervalId: 'process-1',
  elapsedSeconds: 20,
  startedAt: START,
  endedAt: START + 20_000,
})
```

**Step 2: Verify RED**

Run: `npm test`

Expected: FAIL because lifecycle APIs and persisted interval identity do not exist.

**Step 3: Implement minimal focus inactivity rules**

- Upgrade restored state to version 2 without changing the existing storage key.
- Add `lastLifecycleIntervalId`.
- Add `captureInactivity` and `applyInactivityInterval`.
- Derive due break from `deferredBreak` first, then the existing short/long cadence.
- Complete a passive break with one history record and the normal cycle update.

**Step 4: Write failing tests for active breaks and stale callbacks**

Cover:

- Active break remains incomplete when interval is shorter than remaining.
- Active break completes at the exact threshold.
- Surplus inactivity neither advances new focus nor completes another break.
- Duplicate `intervalId` is a no-op.
- `completeTimerBreak` with a stale or duplicate `breakId` is a no-op.

**Step 5: Verify RED, implement, and verify GREEN**

Run before implementation: `npm test`

Expected: new active-break tests fail.

Implement identity-checked completion and active-break interval handling, then rerun `npm test` and expect all tests to pass.

**Step 6: Commit**

```bash
git add src/lib/timer.ts src/lib/timer.test.ts
git commit -m "feat: apply lifecycle rest semantics"
```

### Task 3: Add a testable Rust lifecycle gate

**Files:**

- Modify: `src-tauri/src/lib.rs`

**Step 1: Write Rust unit tests first**

Inside `#[cfg(test)]`, cover:

```rust
#[test]
fn overlapping_lock_and_sleep_form_one_interval() {
    let mut gate = LifecycleGate::default();
    assert!(gate.begin(InactivityReason::ScreenLock, 10.0, 1_000).is_some());
    assert!(gate.begin(InactivityReason::SystemSleep, 12.0, 3_000).is_none());
    assert!(gate.end(InactivityReason::SystemSleep, 30.0, 21_000).is_none());
    let event = gate.end(InactivityReason::ScreenLock, 35.0, 26_000).unwrap();
    assert_eq!(event.elapsed_seconds, 25.0);
}
```

Also test duplicate starts/ends, pending interval replay, acknowledgement, and bounded pending storage.

**Step 2: Verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml lifecycle_gate`

Expected: compile failure because the gate does not exist.

**Step 3: Implement minimal gate**

- Add `InactivityReason`, active interval state, monotonic sequence, and pending completed intervals.
- Emit only on reason-set `0 → 1` and `1 → 0` transitions.
- Expose snapshot and acknowledgement Tauri commands.
- Reject acknowledgement IDs not present in the pending queue.

**Step 4: Verify GREEN and full Rust tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml lifecycle_gate
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all Rust tests pass.

**Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: add idempotent lifecycle gate"
```

### Task 4: Connect macOS lifecycle notifications and continuous time

**Files:**

- Modify: `src-tauri/native/macos.m`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/src/lib.rs`

**Step 1: Add a failing Rust test for strict-break continuous deadlines**

Extract deadline math into pure functions and test that an injected continuous time reaching the deadline returns zero and completes once.

**Step 2: Verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml continuous_deadline`

Expected: FAIL because the continuous deadline helper does not exist.

**Step 3: Implement native adapter**

- Add an Objective-C registration function accepting a Rust callback.
- Observe screen lock/unlock, workspace sleep/wake, and session inactive/active events.
- Add a function returning `mach_continuous_time` converted to seconds.
- Register observers once on the AppKit main queue.
- Feed raw reasons to the Rust gate and emit structured Tauri events.
- Hide strict-break covers on the first inactive reason and restore only if the break remains active.
- Replace `Instant` strict-break deadlines with the same continuous clock.
- Include the current `breakId` in strict-break completion commands.

**Step 4: Verify build and tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: all tests pass and the macOS bridge links successfully.

**Step 5: Commit**

```bash
git add src-tauri/native/macos.m src-tauri/build.rs src-tauri/src/lib.rs
git commit -m "feat: observe macOS lock and sleep lifecycle"
```

### Task 5: Drive the React timer from monotonic time and lifecycle events

**Files:**

- Modify: `src/tauriBridge.ts`
- Modify: `src/hooks/useBreakTimer.ts`
- Modify: `src/App.tsx`

**Step 1: Write failing orchestration tests in the pure timer layer**

Before changing the hook, add any missing reducer-level regression for the exact event order being integrated: start snapshot, active-break completion, lifecycle end, duplicate strict completion.

Run `npm test` and verify the regression fails for the expected missing guard.

**Step 2: Implement the bridge**

- Listen for structured `repose-lifecycle` payloads before rendering.
- Fetch and cache the native lifecycle snapshot.
- Deliver cached start/end events to `onLifecycle` subscribers.
- Add acknowledgement invocation.
- Change desktop command payloads so strict completion carries `breakId`.

**Step 3: Implement the hook driver**

- Sample `performance.now()` and advance by seconds only while lifecycle-active.
- Flush known activity before recording `inactive-start`.
- Do not flush a possibly sleep-spanning performance delta on `inactive-end`; reset the baseline first.
- Apply the interval with its captured start context, save immediately, then acknowledge.
- Flush before every user action and before page exit.

**Step 4: Integrate App command handling**

- Pass the expected `breakId` to native completion.
- Keep notifications, postponed break UI, and strict-break behavior unchanged.

**Step 5: Verify frontend**

Run:

```bash
npm test
npm run build
```

Expected: all tests pass and TypeScript/Vite build succeeds without warnings.

**Step 6: Commit**

```bash
git add src/tauriBridge.ts src/hooks/useBreakTimer.ts src/App.tsx src/lib/timer.test.ts
git commit -m "feat: gate timer on desktop lifecycle"
```

### Task 6: Update documentation and perform final verification

**Files:**

- Modify: `README-rust.md`
- Modify: `docs/plans/2026-09-07-lifecycle-timer-user-story.md`
- Modify: `docs/plans/2026-09-07-lifecycle-timer-design.md`

**Step 1: Update documentation**

- Mark User Stories implemented only after their tests pass.
- Describe lock/sleep, active-break, restart, clock-jump, and overlap behavior.
- Document that Electron is a historical baseline and proximity unlock is out of scope.
- Record any implementation differences from the approved design.

**Step 2: Run the full verification matrix**

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
git diff --check
git status --short
```

Expected: all automated checks pass; only intentional documentation/code changes remain.

**Step 3: Review the branch diff**

Confirm:

- No focus or break seconds derive from a wall-clock difference.
- Restore never replays closed-app time.
- Every lifecycle completion has `intervalId`; every strict completion has `breakId`.
- Active and passive break paths cannot complete the same or next occurrence twice.
- No phone proximity or automatic-unlock behavior appears in the diff.

**Step 4: Commit**

```bash
git add README-rust.md docs/plans
git commit -m "docs: document lifecycle-aware timing"
```

