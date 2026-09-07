import assert from 'node:assert/strict'
import test from 'node:test'
import {
  DEFAULT_SETTINGS,
  applyInactivityInterval,
  advanceTimerBy,
  captureInactivity,
  changeTimerSettings,
  completeTimerBreak,
  createTimerState,
  deriveWeeklyStats,
  getHourlyStats,
  getTodayStats,
  monotonicElapsedSeconds,
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

test('hourly stats expose 24 immutable-by-copy buckets', () => {
  const state = createTimerState(START)
  const hourly = getHourlyStats(state, START)
  assert.equal(hourly.focusSeconds.length, 24)
  assert.equal(hourly.breakSeconds.length, 24)
  assert.deepEqual(hourly.focusSeconds, Array(24).fill(0))
  assert.deepEqual(hourly.breakSeconds, Array(24).fill(0))
  hourly.focusSeconds[10] = 99
  hourly.breakSeconds[10] = 88
  assert.equal(getHourlyStats(state, START).focusSeconds[10], 0)
  assert.equal(getHourlyStats(state, START).breakSeconds[10], 0)
})

test('trusted focus time splits across local hour boundaries', () => {
  const at = new Date(2026, 8, 6, 10, 59, 50).getTime()
  const state = advanceTimerBy(createTimerState(at), 20, at + 20_000)
  const hourly = getHourlyStats(state, at)
  assert.equal(hourly.focusSeconds[10], 10)
  assert.equal(hourly.focusSeconds[11], 10)
})

test('hourly focus time splits across local midnight', () => {
  const beforeMidnight = new Date(2026, 8, 6, 23, 59, 50).getTime()
  const afterMidnight = beforeMidnight + 20_000
  const state = advanceTimerBy(createTimerState(beforeMidnight), 20, afterMidnight)
  assert.equal(getHourlyStats(state, beforeMidnight).focusSeconds[23], 10)
  assert.equal(getHourlyStats(state, afterMidnight).focusSeconds[0], 10)
})

test('passive rest splits into the matching local hour buckets', () => {
  const at = new Date(2026, 8, 6, 10, 59, 55).getTime()
  const focused = createTimerState(at)
  const state = applyInactivityInterval(focused, captureInactivity(focused), {
    intervalId: 'hourly-passive-rest',
    elapsedSeconds: 10,
    startedAt: at,
    endedAt: at + 10_000,
  })
  const hourly = getHourlyStats(state, at)
  assert.equal(hourly.breakSeconds[10], 5)
  assert.equal(hourly.breakSeconds[11], 5)
  assert.equal(hourly.focusSeconds[10], 0)
  assert.equal(hourly.focusSeconds[11], 0)
})

test('hourly recording preserves earlier timer snapshots', () => {
  const initial = createTimerState(START)
  const first = advanceTimerBy(initial, 5, START + 5_000)
  const second = advanceTimerBy(first, 5, START + 10_000)
  assert.equal(getHourlyStats(initial, START).focusSeconds[10], 0)
  assert.equal(getHourlyStats(first, START).focusSeconds[10], 5)
  assert.equal(getHourlyStats(second, START).focusSeconds[10], 10)
})

test('monotonic samples count only forward active process time', () => {
  assert.equal(monotonicElapsedSeconds(1_000, 6_500, false), 5.5)
  assert.equal(monotonicElapsedSeconds(1_000, 6_500, true), 0)
  assert.equal(monotonicElapsedSeconds(6_500, 1_000, false), 0)
  assert.equal(monotonicElapsedSeconds(Number.NaN, 6_500, false), 0)
  assert.equal(monotonicElapsedSeconds(1_000, Number.POSITIVE_INFINITY, false), 0)
})

test('explicit elapsed advances focus independently of wall-clock jumps', () => {
  let state = createTimerState(START)
  state = advanceTimerBy(state, 10, START + 60 * 60 * 1000)
  assert.equal(state.remaining, 1190)
  assert.equal(getTodayStats(state, START).focusSeconds, 10)
  state = advanceTimerBy(state, 5, START - 60 * 60 * 1000)
  assert.equal(state.remaining, 1185)
  assert.equal(getTodayStats(state, START).focusSeconds, 15)
})

test('restoring a snapshot never replays closed-app time', () => {
  const saved = advanceTimerBy(createTimerState(START), 10, START + 10_000)
  const restored = restoreTimerState(JSON.stringify(saved), START + 60_000)
  assert.equal(restored.remaining, 1190)
  assert.equal(getTodayStats(restored, START).focusSeconds, 10)
})

test('a short passive rest records actual break time without consuming focus', () => {
  const focused = advanceTimerBy(createTimerState(START), 300, START + 300_000)
  const context = captureInactivity(focused)
  const rested = applyInactivityInterval(focused, context, {
    intervalId: 'process-1',
    elapsedSeconds: 10,
    startedAt: START + 300_000,
    endedAt: START + 310_000,
  })
  assert.equal(rested.phase, 'focus')
  assert.equal(rested.remaining, focused.remaining)
  assert.equal(getTodayStats(rested, START).focusSeconds, 300)
  assert.equal(getTodayStats(rested, START).breakSeconds, 10)
  assert.equal(getTodayStats(rested, START).completedBreaks, 0)
})

test('a passive rest at the short-break threshold completes one break and resets focus', () => {
  const focused = advanceTimerBy(createTimerState(START), 300, START + 300_000)
  const rested = applyInactivityInterval(focused, captureInactivity(focused), {
    intervalId: 'process-short-exact',
    elapsedSeconds: 20,
    startedAt: START + 300_000,
    endedAt: START + 320_000,
  })
  assert.equal(rested.phase, 'focus')
  assert.equal(rested.remaining, 1200)
  assert.equal(rested.completedCycles, 1)
  assert.equal(getTodayStats(rested, START).breakSeconds, 20)
  assert.equal(getTodayStats(rested, START).completedBreaks, 1)
  assert.equal(rested.history[0].type, 'short')
  assert.equal(rested.history[0].duration, 20)
})

test('a long passive rest records its full duration but completes only one break', () => {
  const focused = createTimerState(START)
  const rested = applyInactivityInterval(focused, captureInactivity(focused), {
    intervalId: 'process-short-over',
    elapsedSeconds: 125,
    startedAt: START,
    endedAt: START + 125_000,
  })
  assert.equal(getTodayStats(rested, START).breakSeconds, 125)
  assert.equal(getTodayStats(rested, START).completedBreaks, 1)
  assert.equal(rested.history.length, 1)
})

test('the current cadence selects a long passive rest and resets completed cycles', () => {
  const focused = {
    ...createTimerState(START, { longEvery: 4, longDuration: 1 }),
    completedCycles: 4,
  }
  const rested = applyInactivityInterval(focused, captureInactivity(focused), {
    intervalId: 'process-long',
    elapsedSeconds: 60,
    startedAt: START,
    endedAt: START + 60_000,
  })
  assert.equal(rested.completedCycles, 0)
  assert.equal(rested.history[0].type, 'long')
  assert.equal(rested.history[0].duration, 60)
  assert.equal(getTodayStats(rested, START).completedBreaks, 1)
})

test('manual focus pause does not turn lock time into passive rest', () => {
  const paused = toggleTimer(createTimerState(START), START)
  const rested = applyInactivityInterval(paused, captureInactivity(paused), {
    intervalId: 'process-paused',
    elapsedSeconds: 300,
    startedAt: START,
    endedAt: START + 300_000,
  })
  assert.equal(rested.remaining, paused.remaining)
  assert.deepEqual(getTodayStats(rested, START), getTodayStats(paused, START))
  assert.deepEqual(rested.lifecycleIntervalIds, ['process-paused'])
})

test('passive rest during a delay completes the original occurrence without a new postpone', () => {
  const started = startTimerBreak(createTimerState(START, { shortDuration: 30 }), 'short', START)
  const current = advanceTimerBy(started, 5, START + 5_000)
  const delayed = postponeTimerBreak(current, START + 5_000)
  const rested = applyInactivityInterval(delayed, captureInactivity(delayed), {
    intervalId: 'process-deferred',
    elapsedSeconds: 30,
    startedAt: START + 5_000,
    endedAt: START + 35_000,
  })
  assert.equal(rested.phase, 'focus')
  assert.equal(rested.remaining, 1200)
  assert.equal(rested.breakId, null)
  assert.equal(rested.deferredBreak, null)
  assert.equal(rested.postponeUsed, false)
  assert.equal(rested.completedCycles, 1)
  assert.equal(rested.history[0].type, 'short')
})

test('an active break keeps counting during a shorter inactivity interval', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const ticked = advanceTimerBy(started, 5, START + 5_000)
  const rested = applyInactivityInterval(ticked, captureInactivity(ticked), {
    intervalId: 'active-short-partial',
    elapsedSeconds: 10,
    startedAt: START + 5_000,
    endedAt: START + 15_000,
  })
  assert.equal(rested.phase, 'short')
  assert.equal(rested.remaining, 5)
  assert.equal(getTodayStats(rested, START).breakSeconds, 15)
  assert.equal(getTodayStats(rested, START).completedBreaks, 0)
})

test('an active break completes once when inactivity reaches its deadline', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const rested = applyInactivityInterval(started, captureInactivity(started), {
    intervalId: 'active-short-complete',
    elapsedSeconds: 100,
    startedAt: START,
    endedAt: START + 100_000,
  })
  assert.equal(rested.phase, 'focus')
  assert.equal(rested.remaining, 1200)
  assert.equal(rested.completedCycles, 1)
  assert.equal(getTodayStats(rested, START).breakSeconds, 20)
  assert.equal(getTodayStats(rested, START).completedBreaks, 1)
  assert.equal(rested.history.length, 1)
})

test('a duplicate lifecycle interval cannot advance or complete anything twice', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const context = captureInactivity(started)
  const interval = {
    intervalId: 'active-duplicate',
    elapsedSeconds: 20,
    startedAt: START,
    endedAt: START + 20_000,
  }
  const completed = applyInactivityInterval(started, context, interval)
  assert.equal(applyInactivityInterval(completed, context, interval), completed)
})

test('an older lifecycle interval stays idempotent after newer intervals are applied', () => {
  const focused = createTimerState(START)
  const context = captureInactivity(focused)
  const firstInterval = {
    intervalId: 'passive-first',
    elapsedSeconds: 1,
    startedAt: START,
    endedAt: START + 1_000,
  }
  const afterFirst = applyInactivityInterval(focused, context, firstInterval)
  const afterSecond = applyInactivityInterval(afterFirst, context, {
    intervalId: 'passive-second',
    elapsedSeconds: 1,
    startedAt: START + 2_000,
    endedAt: START + 3_000,
  })
  assert.equal(applyInactivityInterval(afterSecond, context, firstInterval), afterSecond)
  assert.equal(getTodayStats(afterSecond, START).breakSeconds, 2)
  const restored = restoreTimerState(JSON.stringify(afterSecond), START + 4_000)
  assert.equal(applyInactivityInterval(restored, context, firstInterval), restored)
})

test('native completion requires the current break identity', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  assert.equal(completeTimerBreak(started, 'stale-break', START + 20_000), started)
  const completed = completeTimerBreak(started, started.breakId!, START + 20_000)
  assert.equal(completed.phase, 'focus')
  assert.equal(completed.history.length, 1)
  assert.equal(completeTimerBreak(completed, started.breakId!, START + 25_000), completed)
})

function advanceNormally(initial: TimerState, until: number): TimerState {
  let state = initial
  for (let now = state.updatedAt + 1000; now <= until; now += 1000) state = advanceTimerBy(state, 1, now)
  return state
}

function advanceTo(initial: TimerState, until: number): TimerState {
  return advanceTimerBy(initial, Math.max(0, (until - initial.updatedAt) / 1000), until)
}

test('focus transitions at the exact deadline and records elapsed seconds', () => {
  const initial = createTimerState(START, { shortInterval: 1 })
  const before = advanceTo(initial, START + 59_000)
  assert.equal(before.phase, 'focus')
  assert.equal(before.remaining, 1)
  const due = advanceTo(before, START + 60_000)
  assert.equal(due.phase, 'short')
  assert.equal(due.remaining, 20)
  assert.equal(getTodayStats(due, START).focusSeconds, 60)
  assert.equal(initial.days[localDateKey(START)].focusSeconds, 0)
})

test('a delayed background tick starts a full break instead of completing an unseen one', () => {
  const state = advanceTo(createTimerState(START, { shortInterval: 1 }), START + 87_000)
  assert.equal(state.phase, 'short')
  assert.equal(state.remaining, 20)
  assert.deepEqual(getTodayStats(state, START), {
    focusSeconds: 60,
    breakSeconds: 0,
    completedBreaks: 0,
    skippedBreaks: 0,
  })
  assert.equal(state.history.length, 0)
  const completed = advanceTo(state, START + 107_000)
  assert.equal(completed.phase, 'focus')
  assert.equal(completed.remaining, 60)
  assert.equal(completed.history[0].completedAt, START + 107_000)
})

test('a long break follows the configured number of completed short breaks', () => {
  let state = createTimerState(START, { shortInterval: 1, shortDuration: 5, longEvery: 2, longDuration: 1 })
  state = advanceNormally(state, START + 130_000)
  assert.equal(state.completedCycles, 2)
  assert.equal(state.phase, 'focus')
  state = advanceTo(state, START + 190_000)
  assert.equal(state.phase, 'long')
  state = advanceTo(state, START + 250_000)
  assert.equal(state.completedCycles, 0)
  assert.equal(state.phase, 'focus')
  assert.equal(state.history[0].type, 'long')
  assert.equal(state.history[0].duration, 60)
})

test('pausing excludes wall time and resuming keeps the remaining duration', () => {
  let state = toggleTimer(advanceTo(createTimerState(START), START + 10_000), START + 10_000)
  assert.equal(state.running, false)
  state = advanceTo(state, START + 1_000_000)
  assert.equal(state.remaining, 1190)
  assert.equal(getTodayStats(state, START).focusSeconds, 10)
  state = toggleTimer(state, START + 1_000_000)
  state = advanceTo(state, START + 1_005_000)
  assert.equal(state.remaining, 1185)
})

test('auto-start off pauses at the end of a break', () => {
  let state = createTimerState(START, { autoStart: false })
  assert.equal(state.running, false)
  state = startTimerBreak(state, 'short', START)
  state = advanceTo(state, START + 60_000)
  assert.equal(state.phase, 'focus')
  assert.equal(state.running, false)
  assert.equal(state.remaining, 1200)
  assert.equal(getTodayStats(state, START).breakSeconds, 20)
})

test('skipping a break counts the skip and does not mark it complete', () => {
  let state = startTimerBreak(createTimerState(START), 'short', START)
  state = advanceTo(state, START + 5_000)
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
  const current = advanceTo(createTimerState(START), START + 60_000)
  const state = changeTimerSettings(current, { shortInterval: 10, sound: false }, START + 60_000)
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

test('local storage never replays closed-app time', () => {
  const initial = advanceTo(createTimerState(START), START + 10_000)
  const fresh = restoreTimerState(JSON.stringify(initial), START + 20_000)
  assert.equal(fresh.remaining, 1190)
  const longGap = restoreTimerState(JSON.stringify(initial), initial.updatedAt + 8 * 60 * 60 * 1000)
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
  const state = advanceTo(createTimerState(beforeMidnight), afterMidnight)
  assert.equal(getTodayStats(state, beforeMidnight).focusSeconds, 10)
  assert.equal(getTodayStats(state, afterMidnight).focusSeconds, 10)
  const week = deriveWeeklyStats(state, afterMidnight)
  assert.equal(week.length, 7)
  assert.equal(week[6].label, '今天')
  assert.equal(week[6].focusSeconds, 10)
  assert.equal(week[0].focusSeconds, 0)
})

test('a backwards clock adjustment never adds negative time', () => {
  const state = advanceTimerBy(createTimerState(START), 0, START - 1000)
  assert.equal(state.remaining, 1200)
  assert.equal(getTodayStats(state, START).focusSeconds, 0)
})

test('sleep or a huge clock jump preserves both focus and break countdowns without invented activity', () => {
  for (const phase of ['focus', 'short', 'long'] as const) {
    const initial = createTimerState(START)
    const started = phase === 'focus' ? initial : startTimerBreak(initial, phase, START)
    const beforeSleep = advanceTo(started, START + 5_000)
    const afterSleep = advanceTimerBy(beforeSleep, 0, START + 8 * 60 * 60 * 1000)
    assert.equal(afterSleep.phase, phase)
    assert.equal(afterSleep.remaining, beforeSleep.remaining)
    assert.deepEqual(afterSleep.days, beforeSleep.days)
    assert.equal(afterSleep.completedCycles, 0)
    assert.equal(afterSleep.history.length, 0)
    const resumed = advanceTo(afterSleep, afterSleep.updatedAt + 1000)
    assert.equal(resumed.remaining, beforeSleep.remaining - 1)
  }
})

test('tick, pause, transition, and suspension preserve settings object identity', () => {
  const initial = createTimerState(START, { shortInterval: 1 })
  const tick = advanceTo(initial, START + 1000)
  const paused = toggleTimer(advanceTo(tick, START + 2000), START + 2000)
  const transition = advanceTo(tick, START + 60_000)
  const suspended = advanceTimerBy(tick, 0, START + 8 * 60 * 60 * 1000)
  for (const state of [tick, paused, transition, suspended]) assert.equal(state.settings, initial.settings)
})

test('a pause click at the break boundary does not accidentally start a new focus session', () => {
  const initial = createTimerState(START, { autoStart: false })
  const started = startTimerBreak(initial, 'short', START)
  const due = advanceTo(started, START + 20_000)
  const state = toggleTimer(due, START + 20_000, started.running)
  assert.equal(state.phase, 'focus')
  assert.equal(state.running, false)
  assert.equal(state.history.length, 1)
})

test('native completion credits exactly one full break including seconds already recorded by ticks', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const ticked = advanceTo(started, START + 5_000)
  const completed = completeTimerBreak(ticked, ticked.breakId!, START + 20_000)
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
  const nativeCompleted = completeTimerBreak(started, started.breakId!, START + 20_000)
  assert.equal(completeTimerBreak(nativeCompleted, started.breakId!, START + 25_000), nativeCompleted)
  const rendererCompleted = advanceTo(started, START + 20_000)
  assert.equal(completeTimerBreak(rendererCompleted, started.breakId!, START + 25_000), rendererCompleted)
  const focus = createTimerState(START)
  assert.equal(completeTimerBreak(focus, 'missing', START + 25_000), focus)
})

test('native long-break completion resets the cycle and honors auto-start off', () => {
  const initial = { ...createTimerState(START, { autoStart: false, longDuration: 1 }), completedCycles: 4 }
  const started = startTimerBreak(initial, 'long', START)
  const completed = completeTimerBreak(started, started.breakId!, START + 60_000)
  assert.equal(completed.completedCycles, 0)
  assert.equal(completed.running, false)
  assert.equal(completed.history[0].type, 'long')
  assert.equal(getTodayStats(completed, START).breakSeconds, 60)
})

test('a native completion following renderer suspension credits only the known break duration', () => {
  const started = startTimerBreak(createTimerState(START), 'short', START)
  const ticked = advanceTo(started, START + 5_000)
  const completed = completeTimerBreak(ticked, ticked.breakId!, START + 8 * 60 * 60 * 1000)
  assert.equal(completed.history.length, 1)
  assert.equal(getTodayStats(completed, START).breakSeconds, 20)
  assert.equal(getTodayStats(completed, START).focusSeconds, 0)
  assert.equal(getTodayStats(completed, START).skippedBreaks, 0)
})

test('a short break can be postponed for one minute and returns at its full duration', () => {
  const initial = startTimerBreak(createTimerState(START, { shortDuration: 30 }), 'short', START)
  const postponed = postponeTimerBreak(advanceTo(initial, START + 5_000), START + 5_000)
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
  const returned = advanceTo(postponed, START + 65_000)
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
  const postponed = postponeTimerBreak(advanceTo(started, START + 10_000), START + 10_000)
  assert.equal(postponed.completedCycles, 0)
  assert.equal(postponed.remaining, 300)
  assert.deepEqual(postponed.deferredBreak, { type: 'long', duration: 120 })
  const returned = advanceTo(postponed, START + 310_000)
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
    const postponed = postponeTimerBreak(advanceTo(started, START + 5_000), START + 5_000)
    const restored = restoreTimerState(JSON.stringify(postponed), START + 15_000)
    assert.equal(restored.remaining, postponed.remaining)
    assert.equal(restored.phaseDuration, postponed.phaseDuration)
    assert.equal(restored.breakId, started.breakId)
    assert.deepEqual(restored.deferredBreak, postponed.deferredBreak)
    assert.equal(restored.postponeUsed, true)
    assert.equal(postponeTimerBreak(restored, START + 16_000), restored)
    const returned = advanceTo(restored, restored.updatedAt + restored.remaining * 1000)
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
  const postponed = postponeTimerBreak(advanceTo(started, START + 1000), START + 1000)
  assert.equal(postponeTimerBreak(postponed, START + 1000), postponed)
  assert.equal(postponeTimerBreak(postponed, START + 30_000), postponed)
  const ticked = advanceTo(postponed, START + 30_000)
  assert.equal(ticked.remaining, 31)
  assert.equal(postponeTimerBreak(ticked, START + 30_000), ticked)
})

test('settings, reset, and pause cannot extend a pending delay or erase its original break', () => {
  const started = startTimerBreak(createTimerState(START, { shortDuration: 30 }), 'short', START)
  const postponed = postponeTimerBreak(advanceTo(started, START + 5_000), START + 5_000)
  const changed = changeTimerSettings(advanceTo(postponed, START + 15_000), { shortDuration: 5, shortInterval: 120, autoStart: false }, START + 15_000)
  assert.equal(changed.remaining, 50)
  assert.equal(changed.phaseDuration, 60)
  assert.deepEqual(changed.deferredBreak, { type: 'short', duration: 30 })
  const reset = resetTimerState(advanceTo(changed, START + 25_000), START + 25_000)
  assert.equal(reset.remaining, 40)
  assert.equal(reset.breakId, started.breakId)
  const paused = toggleTimer(advanceTo(reset, START + 35_000), START + 35_000)
  assert.equal(paused.running, true)
  assert.equal(paused.remaining, 30)
  const returned = advanceTo(paused, START + 65_000)
  assert.equal(returned.remaining, 30)
  const changedAgain = changeTimerSettings(advanceTo(returned, START + 70_000), { shortDuration: 15 }, START + 70_000)
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
    const postponed = postponeTimerBreak(advanceTo(started, START + 5_000), START + 5_000)
    const resumed = startTimerBreak(advanceTo(postponed, START + 15_000), type === 'short' ? 'long' : 'short', START + 15_000)
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
  const postponed = postponeTimerBreak(advanceTo(started, START + 5_000), START + 5_000)
  const returned = advanceTo(postponed, START + 65_000)
  const completed = advanceTo(returned, START + 85_000)
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
  const next = advanceTo(completed, START + 145_000)
  assert.equal(next.phase, 'short')
  assert.notEqual(next.breakId, started.breakId)
  assert.equal(next.postponeUsed, false)
  assert.equal(postponeTimerBreak(next, START + 146_000).postponeUsed, true)
})

test('postponement at or after the deadline cannot defer a completed break', () => {
  for (const elapsed of [20_000, 21_000]) {
    const started = startTimerBreak(createTimerState(START), 'short', START)
    const result = postponeTimerBreak(advanceTo(started, START + elapsed), START + elapsed)
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
  const postponed = postponeTimerBreak(advanceTo(started, START + 5_000), START + 5_000)
  const returned = advanceTo(postponed, START + 65_000)
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
  const { breakId: _id, deferredBreak: _pending, postponeUsed: _used, lifecycleIntervalIds: _intervals, ...old } = started
  const restored = restoreTimerState(JSON.stringify({ ...old, version: 1 }), START)
  assert.equal(restored.version, 2)
  assert.deepEqual(restored.lifecycleIntervalIds, [])
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
