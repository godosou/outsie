import assert from 'node:assert/strict'
import test from 'node:test'
import {
  DEFAULT_SETTINGS,
  MAX_RESTORE_GAP_MS,
  advanceTimer,
  changeTimerSettings,
  completeTimerBreak,
  createTimerState,
  deriveWeeklyStats,
  getTodayStats,
  localDateKey,
  normalizeSettings,
  postponeTimerBreak,
  resetTimerState,
  restoreTimerState,
  skipTimerBreak,
  startTimerBreak,
  toggleTimer,
  type TimerState,
} from './timer.ts'

const START = new Date(2026, 8, 6, 10, 0, 0).getTime()

function advanceNormally(initial: TimerState, until: number): TimerState {
  let state = initial
  for (let now = state.updatedAt + 1000; now <= until; now += 1000) state = advanceTimer(state, now)
  return state
}

test('focus transitions at the exact deadline and records elapsed seconds', () => {
  const initial = createTimerState(START, { shortInterval: 1 })
  const before = advanceTimer(initial, START + 59_000)
  assert.equal(before.phase, 'focus')
  assert.equal(before.remaining, 1)
  const due = advanceTimer(before, START + 60_000)
  assert.equal(due.phase, 'short')
  assert.equal(due.remaining, 20)
  assert.equal(getTodayStats(due, START).focusSeconds, 60)
  assert.equal(initial.days[localDateKey(START)].focusSeconds, 0)
})

test('a delayed background tick starts a full break instead of completing an unseen one', () => {
  const state = advanceTimer(createTimerState(START, { shortInterval: 1 }), START + 87_000)
  assert.equal(state.phase, 'short')
  assert.equal(state.remaining, 20)
  assert.deepEqual(getTodayStats(state, START), {
    focusSeconds: 60,
    breakSeconds: 0,
    completedBreaks: 0,
    skippedBreaks: 0,
  })
  assert.equal(state.history.length, 0)
  const completed = advanceTimer(state, START + 107_000)
  assert.equal(completed.phase, 'focus')
  assert.equal(completed.remaining, 60)
  assert.equal(completed.history[0].completedAt, START + 107_000)
})

test('a long break follows the configured number of completed short breaks', () => {
  let state = createTimerState(START, { shortInterval: 1, shortDuration: 5, longEvery: 2, longDuration: 1 })
  state = advanceNormally(state, START + 130_000)
  assert.equal(state.completedCycles, 2)
  assert.equal(state.phase, 'focus')
  state = advanceTimer(state, START + 190_000)
  assert.equal(state.phase, 'long')
  state = advanceTimer(state, START + 250_000)
  assert.equal(state.completedCycles, 0)
  assert.equal(state.phase, 'focus')
  assert.equal(state.history[0].type, 'long')
  assert.equal(state.history[0].duration, 60)
})

test('pausing excludes wall time and resuming keeps the remaining duration', () => {
  let state = toggleTimer(createTimerState(START), START + 10_000)
  assert.equal(state.running, false)
  state = advanceTimer(state, START + 1_000_000)
  assert.equal(state.remaining, 1190)
  assert.equal(getTodayStats(state, START).focusSeconds, 10)
  state = toggleTimer(state, START + 1_000_000)
  state = advanceTimer(state, START + 1_005_000)
  assert.equal(state.remaining, 1185)
})

test('auto-start off pauses at the end of a break', () => {
  let state = createTimerState(START, { autoStart: false })
  assert.equal(state.running, false)
  state = startTimerBreak(state, 'short', START)
  state = advanceTimer(state, START + 60_000)
  assert.equal(state.phase, 'focus')
  assert.equal(state.running, false)
  assert.equal(state.remaining, 1200)
  assert.equal(getTodayStats(state, START).breakSeconds, 20)
})

test('skipping a break counts the skip and does not mark it complete', () => {
  let state = startTimerBreak(createTimerState(START), 'short', START)
  state = skipTimerBreak(state, START + 5_000)
  assert.equal(state.phase, 'focus')
  assert.equal(state.running, true)
  assert.equal(state.completedCycles, 0)
  assert.deepEqual(getTodayStats(state, START), {
    focusSeconds: 0,
    breakSeconds: 5,
    completedBreaks: 0,
    skippedBreaks: 1,
  })
  assert.equal(skipTimerBreak(state, START).days[localDateKey(START)].skippedBreaks, 1)
})

test('settings clamp unsafe values and preserve omitted or invalid fields', () => {
  assert.deepEqual(normalizeSettings({
    shortInterval: -1,
    shortDuration: 10_000,
    longEvery: 0,
    longDuration: Infinity,
    sound: 'false',
    notifications: true,
  }), {
    ...DEFAULT_SETTINGS,
    shortInterval: 1,
    shortDuration: 300,
    longEvery: 1,
    notifications: true,
  })
  const state = changeTimerSettings(createTimerState(START), { shortInterval: 10, sound: false }, START + 60_000)
  assert.equal(state.remaining, 540)
  assert.equal(state.phaseDuration, 600)
  assert.equal(state.settings.sound, false)
})

test('resetting the countdown preserves recorded daily activity', () => {
  const state = resetTimerState(advanceNormally(createTimerState(START), START + 1_225_000), START + 1_225_000)
  assert.equal(state.phase, 'focus')
  assert.equal(state.remaining, 1200)
  assert.equal(state.completedCycles, 0)
  assert.equal(state.history.length, 1)
  assert.equal(getTodayStats(state, START).completedBreaks, 1)
})

test('local storage restores a short refresh gap but not hours of closed-app time', () => {
  const initial = advanceTimer(createTimerState(START), START + 10_000)
  const fresh = restoreTimerState(JSON.stringify(initial), START + 20_000)
  assert.equal(fresh.remaining, 1180)
  const longGap = restoreTimerState(JSON.stringify(initial), initial.updatedAt + MAX_RESTORE_GAP_MS + 1)
  assert.equal(longGap.remaining, 1190)
  assert.equal(getTodayStats(longGap, START).focusSeconds, 10)
})

test('corrupt or unknown storage safely produces a usable default state', () => {
  for (const raw of ['not json', '{}', 'null', '[]', '{"version":2}']) {
    const state = restoreTimerState(raw, START)
    assert.equal(state.phase, 'focus')
    assert.equal(state.remaining, 1200)
  }
  const state = restoreTimerState(JSON.stringify({
    version: 1,
    phase: 'invalid',
    remaining: -100,
    settings: { shortInterval: -10 },
    days: { [localDateKey(START)]: { focusSeconds: -3, breakSeconds: 'bad' } },
    history: [{ id: 'x', type: 'short', completedAt: START, duration: -1 }, null],
  }), START)
  assert.ok(state.remaining > 0)
  assert.equal(state.settings.shortInterval, 1)
  assert.deepEqual(getTodayStats(state, START), { focusSeconds: 0, breakSeconds: 0, completedBreaks: 0, skippedBreaks: 0 })
  assert.equal(state.history.length, 0)
})

test('daily totals split at midnight and the weekly view includes empty days', () => {
  const beforeMidnight = new Date(2026, 8, 6, 23, 59, 50).getTime()
  const afterMidnight = beforeMidnight + 20_000
  const state = advanceTimer(createTimerState(beforeMidnight), afterMidnight)
  assert.equal(getTodayStats(state, beforeMidnight).focusSeconds, 10)
  assert.equal(getTodayStats(state, afterMidnight).focusSeconds, 10)
  const week = deriveWeeklyStats(state, afterMidnight)
  assert.equal(week.length, 7)
  assert.equal(week[6].label, '今天')
  assert.equal(week[6].focusSeconds, 10)
  assert.equal(week[0].focusSeconds, 0)
})

test('a backwards clock adjustment never adds negative time', () => {
  const state = advanceTimer(createTimerState(START), START - 1000)
  assert.equal(state.remaining, 1200)
  assert.equal(getTodayStats(state, START).focusSeconds, 0)
})

test('sleep or a huge clock jump preserves both focus and break countdowns without invented activity', () => {
  for (const phase of ['focus', 'short', 'long'] as const) {
    const initial = createTimerState(START)
    const started = phase === 'focus' ? initial : startTimerBreak(initial, phase, START)
    const beforeSleep = advanceTimer(started, START + 5_000)
    const afterSleep = advanceTimer(beforeSleep, START + 8 * 60 * 60 * 1000)
    assert.equal(afterSleep.phase, phase)
    assert.equal(afterSleep.remaining, beforeSleep.remaining)
    assert.deepEqual(afterSleep.days, beforeSleep.days)
    assert.equal(afterSleep.completedCycles, 0)
    assert.equal(afterSleep.history.length, 0)
    const resumed = advanceTimer(afterSleep, afterSleep.updatedAt + 1000)
    assert.equal(resumed.remaining, beforeSleep.remaining - 1)
  }
})

test('tick, pause, transition, and suspension preserve settings object identity', () => {
  const initial = createTimerState(START, { shortInterval: 1 })
  const tick = advanceTimer(initial, START + 1000)
  const paused = toggleTimer(tick, START + 2000)
  const transition = advanceTimer(tick, START + 60_000)
  const suspended = advanceTimer(tick, START + MAX_RESTORE_GAP_MS + 2000)
  for (const state of [tick, paused, transition, suspended]) assert.equal(state.settings, initial.settings)
})

test('a pause click at the break boundary does not accidentally start a new focus session', () => {
  const initial = createTimerState(START, { autoStart: false })
  const started = startTimerBreak(initial, 'short', START)
  const state = toggleTimer(started, START + 20_000)
  assert.equal(state.phase, 'focus')
  assert.equal(state.running, false)
  assert.equal(state.history.length, 1)
})

test('native completion credits exactly one full break including seconds already recorded by ticks', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const ticked = advanceTimer(started, START + 5_000)
  const completed = completeTimerBreak(ticked, START + 20_000)
  assert.equal(completed.phase, 'focus')
  assert.equal(completed.running, true)
  assert.equal(completed.remaining, 1200)
  assert.equal(completed.completedCycles, 1)
  assert.deepEqual(getTodayStats(completed, START), {
    focusSeconds: 0,
    breakSeconds: 20,
    completedBreaks: 1,
    skippedBreaks: 0,
  })
  assert.equal(completed.history.length, 1)
  assert.equal(completed.history[0].completedAt, START + 20_000)
  assert.equal(completed.history[0].duration, 20)
  assert.equal(completed.settings, started.settings)
  assert.equal(ticked.phase, 'short')
  assert.equal(ticked.history.length, 0)
})

test('duplicate native completion and callbacks after a renderer completion are no-ops in focus', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const nativeCompleted = completeTimerBreak(started, START + 20_000)
  assert.equal(completeTimerBreak(nativeCompleted, START + 25_000), nativeCompleted)
  const rendererCompleted = advanceTimer(started, START + 20_000)
  assert.equal(completeTimerBreak(rendererCompleted, START + 25_000), rendererCompleted)
  const focus = createTimerState(START)
  assert.equal(completeTimerBreak(focus, START + 25_000), focus)
})

test('native long-break completion resets the cycle and honors auto-start off', () => {
  const initial = { ...createTimerState(START, { autoStart: false, longDuration: 1 }), completedCycles: 4 }
  const started = startTimerBreak(initial, 'long', START)
  const completed = completeTimerBreak(started, START + 60_000)
  assert.equal(completed.completedCycles, 0)
  assert.equal(completed.running, false)
  assert.equal(completed.history[0].type, 'long')
  assert.equal(getTodayStats(completed, START).breakSeconds, 60)
})

test('a native completion following renderer suspension credits only the known break duration', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const ticked = advanceTimer(started, START + 5_000)
  const completed = completeTimerBreak(ticked, START + 8 * 60 * 60 * 1000)
  assert.equal(completed.history.length, 1)
  assert.equal(getTodayStats(completed, START).breakSeconds, 20)
  assert.equal(getTodayStats(completed, START).focusSeconds, 0)
  assert.equal(getTodayStats(completed, START).skippedBreaks, 0)
})

test('a short break can be postponed for one minute and returns at its full duration', () => {
  const initial = startTimerBreak(createTimerState(START, { shortDuration: 30 }), 'short', START)
  const postponed = postponeTimerBreak(initial, START + 5_000)
  assert.equal(postponed.phase, 'focus')
  assert.equal(postponed.remaining, 60)
  assert.equal(postponed.running, true)
  assert.equal(postponed.postponeUsed, true)
  assert.equal(postponed.breakId, initial.breakId)
  assert.deepEqual(postponed.deferredBreak, { type: 'short', duration: 30 })
  assert.equal(postponed.completedCycles, 0)
  assert.deepEqual(getTodayStats(postponed, START), {
    focusSeconds: 0, breakSeconds: 5, completedBreaks: 0, skippedBreaks: 0,
  })
  const returned = advanceTimer(postponed, START + 65_000)
  assert.equal(returned.phase, 'short')
  assert.equal(returned.remaining, 30)
  assert.equal(returned.phaseDuration, 30)
  assert.equal(returned.breakId, initial.breakId)
  assert.equal(returned.deferredBreak, null)
  assert.equal(returned.postponeUsed, true)
  assert.equal(returned.history.length, 0)
  assert.equal(getTodayStats(returned, START).focusSeconds, 60)
  assert.equal(postponeTimerBreak(returned, START + 65_000), returned)
})

test('a manually started long break returns as long after five minutes outside the normal cadence', () => {
  const started = startTimerBreak(createTimerState(START, { longDuration: 2 }), 'long', START)
  const postponed = postponeTimerBreak(started, START + 10_000)
  assert.equal(postponed.completedCycles, 0)
  assert.equal(postponed.remaining, 300)
  assert.deepEqual(postponed.deferredBreak, { type: 'long', duration: 120 })
  const returned = advanceTimer(postponed, START + 310_000)
  assert.equal(returned.phase, 'long')
  assert.equal(returned.remaining, 120)
  assert.equal(returned.breakId, started.breakId)
  assert.equal(returned.postponeUsed, true)
  assert.equal(getTodayStats(returned, START).completedBreaks, 0)
  assert.equal(getTodayStats(returned, START).skippedBreaks, 0)
})

test('reloads retain the delay, original duration, stable identity, and used postponement', () => {
  for (const type of ['short', 'long'] as const) {
    const started = startTimerBreak(createTimerState(START, { shortInterval: 1, shortDuration: 30, longDuration: 2 }), type, START)
    const postponed = postponeTimerBreak(started, START + 5_000)
    const restored = restoreTimerState(JSON.stringify(postponed), START + 15_000)
    assert.equal(restored.remaining, postponed.remaining - 10)
    assert.equal(restored.phaseDuration, postponed.phaseDuration)
    assert.equal(restored.breakId, started.breakId)
    assert.deepEqual(restored.deferredBreak, postponed.deferredBreak)
    assert.equal(restored.postponeUsed, true)
    assert.equal(postponeTimerBreak(restored, START + 16_000), restored)
    const returned = advanceTimer(restored, restored.updatedAt + restored.remaining * 1000)
    const returnedReloaded = restoreTimerState(JSON.stringify(returned), returned.updatedAt)
    assert.equal(returnedReloaded.phase, type)
    assert.equal(returnedReloaded.remaining, started.phaseDuration)
    assert.equal(returnedReloaded.breakId, started.breakId)
    assert.equal(returnedReloaded.postponeUsed, true)
    assert.equal(postponeTimerBreak(returnedReloaded, returned.updatedAt), returnedReloaded)
  }
})

test('duplicate postponement requests cannot restart the delay', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const postponed = postponeTimerBreak(started, START + 1000)
  assert.equal(postponeTimerBreak(postponed, START + 1000), postponed)
  assert.equal(postponeTimerBreak(postponed, START + 30_000), postponed)
  const ticked = advanceTimer(postponed, START + 30_000)
  assert.equal(ticked.remaining, 31)
  assert.equal(postponeTimerBreak(ticked, START + 30_000), ticked)
})

test('settings, reset, and pause cannot extend a pending delay or erase its original break', () => {
  const started = startTimerBreak(createTimerState(START, { shortDuration: 30 }), 'short', START)
  const postponed = postponeTimerBreak(started, START + 5_000)
  const changed = changeTimerSettings(postponed, { shortDuration: 5, shortInterval: 120, autoStart: false }, START + 15_000)
  assert.equal(changed.remaining, 50)
  assert.equal(changed.phaseDuration, 60)
  assert.deepEqual(changed.deferredBreak, { type: 'short', duration: 30 })
  const reset = resetTimerState(changed, START + 25_000)
  assert.equal(reset.remaining, 40)
  assert.equal(reset.breakId, started.breakId)
  const paused = toggleTimer(reset, START + 35_000)
  assert.equal(paused.running, true)
  assert.equal(paused.remaining, 30)
  const returned = advanceTimer(paused, START + 65_000)
  assert.equal(returned.remaining, 30)
  const changedAgain = changeTimerSettings(returned, { shortDuration: 15 }, START + 70_000)
  assert.equal(changedAgain.remaining, 25)
  assert.equal(changedAgain.phaseDuration, 30)
  const reloaded = restoreTimerState(JSON.stringify(changedAgain), START + 70_000)
  assert.equal(reloaded.remaining, 25)
  assert.equal(reloaded.phaseDuration, 30)
  assert.equal(reloaded.postponeUsed, true)
  assert.equal(resetTimerState(reloaded, START + 70_000), reloaded)
})

test('manual start during delay always resumes the same break without a second chance', () => {
  for (const type of ['short', 'long'] as const) {
    const started = startTimerBreak(createTimerState(START), type, START)
    const postponed = postponeTimerBreak(started, START + 5_000)
    const resumed = startTimerBreak(postponed, type === 'short' ? 'long' : 'short', START + 15_000)
    assert.equal(resumed.phase, type)
    assert.equal(resumed.remaining, started.phaseDuration)
    assert.equal(resumed.breakId, started.breakId)
    assert.equal(resumed.postponeUsed, true)
    assert.equal(resumed.deferredBreak, null)
    assert.equal(startTimerBreak(resumed, 'long', START + 15_000), resumed)
    assert.equal(postponeTimerBreak(resumed, START + 15_000), resumed)
  }
})

test('completion clears the occurrence and gives only the next scheduled break a fresh postponement', () => {
  const started = startTimerBreak(createTimerState(START, { shortInterval: 1 }), 'short', START)
  const postponed = postponeTimerBreak(started, START + 5_000)
  const returned = advanceTimer(postponed, START + 65_000)
  const completed = advanceTimer(returned, START + 85_000)
  assert.equal(completed.phase, 'focus')
  assert.equal(completed.breakId, null)
  assert.equal(completed.postponeUsed, false)
  assert.equal(completed.deferredBreak, null)
  assert.equal(completed.completedCycles, 1)
  assert.equal(completed.history.length, 1)
  assert.equal(completed.history[0].duration, 20)
  assert.equal(getTodayStats(completed, START).breakSeconds, 25)
  assert.equal(getTodayStats(completed, START).completedBreaks, 1)
  assert.equal(getTodayStats(completed, START).skippedBreaks, 0)
  const next = advanceTimer(completed, START + 145_000)
  assert.equal(next.phase, 'short')
  assert.notEqual(next.breakId, started.breakId)
  assert.equal(next.postponeUsed, false)
  assert.equal(postponeTimerBreak(next, START + 146_000).postponeUsed, true)
})

test('postponement at or after the deadline cannot defer a completed break', () => {
  for (const elapsed of [20_000, 21_000]) {
    const started = startTimerBreak(createTimerState(START), 'short', START)
    const result = postponeTimerBreak(started, START + elapsed)
    assert.equal(result.phase, 'focus')
    assert.equal(result.deferredBreak, null)
    assert.equal(result.breakId, null)
    assert.equal(result.remaining, 1200)
    assert.equal(result.history.length, 1)
    assert.equal(result.completedCycles, 1)
  }
  const focus = createTimerState(START, { shortInterval: 1 })
  assert.equal(postponeTimerBreak(focus, START + 60_000), focus)
})

test('skipping an active or deferred occurrence clears its one-time state', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const postponed = postponeTimerBreak(started, START + 5_000)
  const returned = advanceTimer(postponed, START + 65_000)
  for (const state of [postponed, returned]) {
    const skipped = skipTimerBreak(state, state.updatedAt)
    assert.equal(skipped.breakId, null)
    assert.equal(skipped.deferredBreak, null)
    assert.equal(skipped.postponeUsed, false)
    assert.equal(skipped.completedCycles, 0)
    assert.equal(getTodayStats(skipped, START).skippedBreaks, 1)
    assert.equal(getTodayStats(skipped, START).completedBreaks, 0)
  }
})

test('old v1 states gain an occurrence identity and invalid pending data is discarded', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const { breakId: _id, deferredBreak: _pending, postponeUsed: _used, ...old } = started
  const restored = restoreTimerState(JSON.stringify(old), START)
  assert.equal(restored.phase, 'short')
  assert.ok(restored.breakId)
  assert.equal(restored.postponeUsed, false)
  assert.equal(restored.deferredBreak, null)
  assert.equal(restoreTimerState(JSON.stringify(restored), START).breakId, restored.breakId)
  const invalid = restoreTimerState(JSON.stringify({ ...createTimerState(START), deferredBreak: { type: 'wrong', duration: 500 }, postponeUsed: true, breakId: 'bad' }), START)
  assert.equal(invalid.deferredBreak, null)
  assert.equal(invalid.postponeUsed, false)
  assert.equal(invalid.breakId, null)
  assert.equal(invalid.remaining, 1200)
})
