# Daily Activity Chart Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a date-selectable 24-hour chart that shows locally recorded focus and rest minutes for one day.

**Architecture:** Extend the persisted timer state with validated 24-slot focus/rest buckets keyed by local date, and update those buckets through the same trusted elapsed-time path that owns daily totals. Expose immutable per-day chart data from the timer hook and replace the weekly activity chart with an accessible React/CSS stacked hourly chart.

**Tech Stack:** TypeScript 7, React 19, Node test runner, CSS, Tauri 2

---

### Task 1: Define and derive hourly activity data

**Files:**
- Modify: `src/lib/timer.ts`
- Test: `src/lib/timer.test.ts`

**Step 1: Write the failing tests**

Import `getHourlyStats` and add tests proving that a fresh state returns two defensive 24-item zero arrays and that recorded state cannot be mutated through the returned view.

```ts
test('hourly stats expose 24 immutable-by-copy buckets', () => {
  const state = createTimerState(START)
  const hourly = getHourlyStats(state, START)
  assert.equal(hourly.focusSeconds.length, 24)
  assert.equal(hourly.breakSeconds.length, 24)
  assert.deepEqual(hourly.focusSeconds, Array(24).fill(0))
  hourly.focusSeconds[10] = 99
  assert.equal(getHourlyStats(state, START).focusSeconds[10], 0)
})
```

**Step 2: Run the test to verify it fails**

Run: `node --import /Users/JingminChen/Documents/ChatGPT/strechly/node_modules/tsx/dist/loader.mjs --test src/lib/timer.test.ts`

Expected: FAIL because `getHourlyStats` is not exported.

**Step 3: Write the minimal model implementation**

In `src/lib/timer.ts`:

```ts
export interface HourlyStats {
  focusSeconds: number[]
  breakSeconds: number[]
}

export interface TimerState {
  version: 3
  // existing fields...
  hourly: Record<string, HourlyStats>
}

const emptyHourlyStats = (): HourlyStats => ({
  focusSeconds: Array(24).fill(0),
  breakSeconds: Array(24).fill(0),
})

export function getHourlyStats(state: TimerState, timestamp = Date.now()): HourlyStats {
  const value = state.hourly[localDateKey(timestamp)] ?? emptyHourlyStats()
  return { focusSeconds: [...value.focusSeconds], breakSeconds: [...value.breakSeconds] }
}
```

Initialize `hourly: {}` in `createTimerState`.

**Step 4: Run the test to verify it passes**

Run the timer test command above.

Expected: PASS.

**Step 5: Commit**

```bash
git add src/lib/timer.ts src/lib/timer.test.ts
git commit -m "feat: add hourly activity model"
```

### Task 2: Record trusted elapsed time into local hour buckets

**Files:**
- Modify: `src/lib/timer.ts`
- Test: `src/lib/timer.test.ts`

**Step 1: Write failing boundary tests**

Add focused tests that:

- split 20 seconds across 10:59:50 and 11:00:10 into two focus buckets;
- split an interval at local midnight into two different date keys;
- attribute lifecycle inactivity to rest buckets without consuming focus;
- preserve the original state's nested arrays after an update.

```ts
test('trusted focus time splits across local hour boundaries', () => {
  const at = new Date(2026, 8, 6, 10, 59, 50).getTime()
  const state = advanceTimerBy(createTimerState(at), 20, at + 20_000)
  const hourly = getHourlyStats(state, at)
  assert.equal(hourly.focusSeconds[10], 10)
  assert.equal(hourly.focusSeconds[11], 10)
})
```

**Step 2: Run the tests to verify they fail**

Run the timer test command.

Expected: FAIL because elapsed time is not yet written to hourly buckets.

**Step 3: Implement hour-boundary splitting**

Change the private recorder to accept both daily and hourly maps. At each segment:

```ts
const date = new Date(cursor)
const hour = date.getHours()
const nextHour = new Date(cursor)
nextHour.setMinutes(0, 0, 0)
nextHour.setHours(nextHour.getHours() + 1)
const end = Math.min(to, nextHour.getTime())
```

Clone a day's hourly object and its arrays before incrementing. Call the unified recorder from `advanceTimerBy`, `applyInactivityInterval`, and `completeTimerBreak`. Clone `hourly` in every mutation path so the input state remains immutable.

**Step 4: Run the tests to verify they pass**

Run the timer test command.

Expected: all timer tests PASS.

**Step 5: Commit**

```bash
git add src/lib/timer.ts src/lib/timer.test.ts
git commit -m "feat: record focus and rest by hour"
```

### Task 3: Migrate and validate persisted hourly data

**Files:**
- Modify: `src/lib/timer.ts`
- Test: `src/lib/timer.test.ts`

**Step 1: Write failing persistence tests**

Add tests proving:

- version 2 snapshots restore as version 3 with empty hourly data;
- valid version 3 buckets survive restore;
- malformed dates, wrong array lengths, negative values, strings, `NaN`, and infinities cannot enter restored state;
- hourly keys older than the existing 35-day retention cutoff are trimmed with daily totals.

**Step 2: Run the tests to verify they fail**

Run the timer test command.

Expected: FAIL because restore only understands versions 1 and 2 and does not parse hourly data.

**Step 3: Implement version 3 restoration**

Accept state versions 1, 2, and 3. Parse `hourly` only for version 3, accepting exactly 24 entries per series and mapping invalid entries to zero. Return `version: 3`. Extend `trimRecords`:

```ts
state.hourly = Object.fromEntries(
  Object.entries(state.hourly).filter(([key]) => key >= cutoff),
)
```

Do not synthesize historical hourly data for version 1/2 snapshots.

**Step 4: Run the tests to verify they pass**

Run the timer test command.

Expected: all timer tests PASS.

**Step 5: Commit**

```bash
git add src/lib/timer.ts src/lib/timer.test.ts
git commit -m "feat: persist hourly activity safely"
```

### Task 4: Expose selected-day data and build the chart UI

**Files:**
- Modify: `src/hooks/useBreakTimer.ts`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`

**Step 1: Add the selected-day hook API**

Return the state-safe accessors needed by the page:

```ts
getStatsForDate(timestamp: number) {
  return getTodayStats(stateRef.current, timestamp)
},
getHourlyStatsForDate(timestamp: number) {
  return getHourlyStats(stateRef.current, timestamp)
},
getHistoryForDate(timestamp: number) {
  const key = localDateKey(timestamp)
  return stateRef.current.history.filter(entry => localDateKey(entry.completedAt) === key)
},
```

Use callbacks so date selection does not change timer ownership.

**Step 2: Replace the weekly chart with the approved daily chart**

In `App.tsx`:

- keep `activityDate` and `selectedHour` state;
- clamp navigation to today and 34 days ago;
- show the selected date's summary and history;
- render 24 accessible stacked-bar buttons with focus/rest segments;
- scale all bars against the largest hourly total, preserving a small visible minimum for non-zero values;
- show a legend and selected-hour detail row;
- display a calm empty state when all buckets are zero.

Use local date construction instead of parsing `YYYY-MM-DD` as UTC. Reset the selected hour to the current hour for today or the busiest recorded hour for historical dates.

**Step 3: Style desktop, dark, and narrow layouts**

In `src/styles.css`, replace `.chart`, `.chart-columns`, `.bar-track`, and `.bar-value` rules with `.daily-chart-*` styles. Ensure 24 bars fit without horizontal page overflow, use `var(...)` theme colors, retain visible keyboard focus, and simplify labels below 760px.

**Step 4: Build the frontend**

Temporarily link the known dependency directory, then run:

```bash
./node_modules/.bin/tsc -b
./node_modules/.bin/vite build --configLoader runner
```

Expected: both commands exit 0. Remove only the temporary `node_modules` symlink afterward.

**Step 5: Commit**

```bash
git add src/hooks/useBreakTimer.ts src/App.tsx src/styles.css
git commit -m "feat: add daily activity chart"
```

### Task 5: Release, package, and verify the formal Mac app

**Files:**
- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src/App.tsx`
- Modify: `README-rust.md`

**Step 1: Bump the feature release version**

Change all app-owned version fields from `0.3.0` to `0.4.0`. Document the daily chart, local hourly aggregation, 35-day retention, and absence of reconstructed pre-upgrade hourly history.

**Step 2: Run complete verification**

Run:

```bash
node --import /Users/JingminChen/Documents/ChatGPT/strechly/node_modules/tsx/dist/loader.mjs --test src/lib/timer.test.ts
cargo test --manifest-path src-tauri/Cargo.toml
./node_modules/.bin/tsc -b
./node_modules/.bin/vite build --configLoader runner
git diff --check
```

Expected: all unit tests and builds PASS with no whitespace errors.

**Step 3: Build the formal `.app`**

Run:

```bash
/Users/JingminChen/Documents/ChatGPT/strechly/node_modules/.bin/tauri build --bundles app --config '{"build":{"beforeBuildCommand":""}}'
```

Expected bundle: `src-tauri/target/release/bundle/macos/Repose Lite.app`.

**Step 4: Launch and inspect the formal `.app`**

Launch `Repose Lite.app`, open 「我的记录」, verify date navigation, 24 hourly bars, selected-hour details, empty state, dark mode, current timer updates, and displayed version `0.4.0`. Never use the bare Rust binary as the installed deliverable.

**Step 5: Commit**

```bash
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/tauri.conf.json src/App.tsx README-rust.md
git commit -m "chore: release daily chart in 0.4.0"
```

