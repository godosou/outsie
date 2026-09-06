export type TimerPhase = 'focus' | 'short' | 'long'

export interface TimerSettings {
  shortInterval: number
  shortDuration: number
  longEvery: number
  longDuration: number
  sound: boolean
  notifications: boolean
  autoStart: boolean
}

export interface DailyStats {
  focusSeconds: number
  breakSeconds: number
  completedBreaks: number
  skippedBreaks: number
}

export interface BreakHistoryEntry {
  id: string
  type: 'short' | 'long'
  completedAt: number
  duration: number
}

export interface TimerState {
  version: 1
  settings: TimerSettings
  phase: TimerPhase
  running: boolean
  remaining: number
  phaseDuration: number
  /** Identifies one break through its initial presentation, delay, and return. */
  breakId: string | null
  deferredBreak: { type: 'short' | 'long'; duration: number } | null
  postponeUsed: boolean
  completedCycles: number
  days: Record<string, DailyStats>
  history: BreakHistoryEntry[]
  updatedAt: number
}

export interface WeeklyStats extends DailyStats {
  date: string
  label: string
}

export const STORAGE_KEY = 'repose.timer.v1'
export const MAX_RESTORE_GAP_MS = 5 * 60 * 1000
export const DEFAULT_SETTINGS: Readonly<TimerSettings> = Object.freeze({
  shortInterval: 20,
  shortDuration: 20,
  longEvery: 4,
  longDuration: 5,
  sound: true,
  notifications: false,
  autoStart: true,
})

const EMPTY_STATS: Readonly<DailyStats> = Object.freeze({
  focusSeconds: 0,
  breakSeconds: 0,
  completedBreaks: 0,
  skippedBreaks: 0,
})

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const finiteNumber = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value)

function boundedNumber(value: unknown, fallback: number, min: number, max: number) {
  return finiteNumber(value) ? Math.min(max, Math.max(min, Math.round(value))) : fallback
}

/** Reject non-numeric input, clamp editable durations, and preserve omitted settings. */
export function normalizeSettings(
  input: unknown,
  fallback: Readonly<TimerSettings> = DEFAULT_SETTINGS,
): TimerSettings {
  const data = isRecord(input) ? input : {}
  return {
    shortInterval: boundedNumber(data.shortInterval, fallback.shortInterval, 1, 120),
    shortDuration: boundedNumber(data.shortDuration, fallback.shortDuration, 5, 300),
    longEvery: boundedNumber(data.longEvery, fallback.longEvery, 1, 12),
    longDuration: boundedNumber(data.longDuration, fallback.longDuration, 1, 60),
    sound: typeof data.sound === 'boolean' ? data.sound : fallback.sound,
    notifications: typeof data.notifications === 'boolean' ? data.notifications : fallback.notifications,
    autoStart: typeof data.autoStart === 'boolean' ? data.autoStart : fallback.autoStart,
  }
}

export function localDateKey(timestamp: number): string {
  const date = new Date(timestamp)
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

export function getPhaseDuration(phase: TimerPhase, settings: TimerSettings): number {
  if (phase === 'short') return settings.shortDuration
  if (phase === 'long') return settings.longDuration * 60
  return settings.shortInterval * 60
}

export function getPostponeSeconds(type: 'short' | 'long'): number {
  return type === 'short' ? 60 : 300
}

function createBreakId(at: number): string {
  return `${at}-${globalThis.crypto.randomUUID()}`
}

export function createTimerState(now = Date.now(), settings?: Partial<TimerSettings>): TimerState {
  const normalizedSettings = normalizeSettings(settings)
  const duration = getPhaseDuration('focus', normalizedSettings)
  return {
    version: 1,
    settings: normalizedSettings,
    phase: 'focus',
    running: normalizedSettings.autoStart,
    remaining: duration,
    phaseDuration: duration,
    breakId: null,
    deferredBreak: null,
    postponeUsed: false,
    completedCycles: 0,
    days: { [localDateKey(now)]: { ...EMPTY_STATS } },
    history: [],
    updatedAt: now,
  }
}

export function getTodayStats(state: TimerState, now = Date.now()): DailyStats {
  return { ...(state.days[localDateKey(now)] ?? EMPTY_STATS) }
}

export function deriveWeeklyStats(state: TimerState, now = Date.now()): WeeklyStats[] {
  const today = new Date(now)
  today.setHours(12, 0, 0, 0)
  return Array.from({ length: 7 }, (_, index) => {
    const date = new Date(today)
    date.setDate(date.getDate() - (6 - index))
    const key = localDateKey(date.getTime())
    return {
      date: key,
      label: index === 6 ? '今天' : ['周日', '周一', '周二', '周三', '周四', '周五', '周六'][date.getDay()],
      ...(state.days[key] ?? EMPTY_STATS),
    }
  })
}

function getDay(days: Record<string, DailyStats>, timestamp: number): DailyStats {
  const key = localDateKey(timestamp)
  const stats = { ...(days[key] ?? EMPTY_STATS) }
  days[key] = stats
  return stats
}

/** Split elapsed time at local midnight, including daylight-saving boundaries. */
function recordTime(days: Record<string, DailyStats>, phase: TimerPhase, from: number, to: number) {
  let cursor = from
  while (cursor < to) {
    const nextMidnight = new Date(cursor)
    nextMidnight.setHours(24, 0, 0, 0)
    const end = Math.min(to, nextMidnight.getTime())
    const stats = getDay(days, cursor)
    const seconds = (end - cursor) / 1000
    if (phase === 'focus') stats.focusSeconds += seconds
    else stats.breakSeconds += seconds
    cursor = end
  }
}

function trimRecords(state: TimerState, now: number) {
  const oldest = new Date(now)
  oldest.setDate(oldest.getDate() - 34)
  const cutoff = localDateKey(oldest.getTime())
  state.days = Object.fromEntries(Object.entries(state.days).filter(([key]) => key >= cutoff))
  state.history = state.history.slice(0, 300)
}

function transition(state: TimerState, at: number): void {
  if (state.phase === 'focus') {
    if (state.deferredBreak) {
      state.phase = state.deferredBreak.type
      state.phaseDuration = state.deferredBreak.duration
      state.deferredBreak = null
    } else {
      state.phase = state.completedCycles >= state.settings.longEvery ? 'long' : 'short'
      state.phaseDuration = getPhaseDuration(state.phase, state.settings)
      state.breakId = createBreakId(at)
      state.postponeUsed = false
    }
    state.running = true
  } else {
    const type = state.phase
    getDay(state.days, at).completedBreaks += 1
    state.history.unshift({
      id: `${at}-${type}-${state.history.length}`,
      type,
      completedAt: at,
      duration: state.phaseDuration,
    })
    state.completedCycles = type === 'long' ? 0 : state.completedCycles + 1
    state.phase = 'focus'
    state.running = state.settings.autoStart
    state.phaseDuration = getPhaseDuration('focus', state.settings)
    state.breakId = null
    state.deferredBreak = null
    state.postponeUsed = false
  }
  state.remaining = state.phaseDuration
}

/**
 * Use wall-clock time within a phase, but never replay unseen breaks after suspension.
 * A newly due phase gets its full duration from the moment the app processes it.
 */
export function advanceTimer(original: TimerState, now: number): TimerState {
  if (now === original.updatedAt) return original
  const gap = now - original.updatedAt
  if (gap < 0 || gap > MAX_RESTORE_GAP_MS || !original.running) return { ...original, updatedAt: now }
  const state: TimerState = {
    ...original,
    days: { ...original.days },
    history: [...original.history],
    updatedAt: now,
  }
  const segmentEnd = Math.min(now, original.updatedAt + state.remaining * 1000)
  recordTime(state.days, state.phase, original.updatedAt, segmentEnd)
  state.remaining = Math.max(0, state.remaining - (segmentEnd - original.updatedAt) / 1000)
  if (state.remaining <= 0.000001) transition(state, segmentEnd)
  trimRecords(state, now)
  return state
}

export function toggleTimer(original: TimerState, now = Date.now()): TimerState {
  const state = advanceTimer(original, now)
  if (original.deferredBreak) return state
  // Preserve the user's pause/resume intent if a phase ended between render and click.
  return { ...state, running: !original.running, updatedAt: now }
}

export function startTimerBreak(original: TimerState, type: 'short' | 'long', now = Date.now()): TimerState {
  const state = advanceTimer(original, now)
  // Repeated native/menu actions must not replace an already active occurrence.
  if (state.phase !== 'focus') return state
  if (state.deferredBreak) {
    return {
      ...state,
      phase: state.deferredBreak.type,
      remaining: state.deferredBreak.duration,
      phaseDuration: state.deferredBreak.duration,
      deferredBreak: null,
      running: true,
    }
  }
  const duration = getPhaseDuration(type, state.settings)
  return {
    ...state,
    phase: type,
    remaining: duration,
    phaseDuration: duration,
    running: true,
    breakId: createBreakId(now),
    deferredBreak: null,
    postponeUsed: false,
  }
}

/** Delay this occurrence once without completing it or advancing its cycle. */
export function postponeTimerBreak(original: TimerState, now = Date.now()): TimerState {
  if (original.phase === 'focus' || original.postponeUsed) return original
  const state = advanceTimer(original, now)
  // A click racing the deadline cannot postpone the next occurrence or completed break.
  if (state.phase === 'focus' || now >= original.updatedAt + original.remaining * 1000) return state
  const duration = getPostponeSeconds(state.phase)
  return {
    ...state,
    phase: 'focus',
    remaining: duration,
    phaseDuration: duration,
    running: true,
    deferredBreak: { type: state.phase, duration: state.phaseDuration },
    postponeUsed: true,
  }
}

/** Accept completion from the native strict-break deadline, even if renderer ticks stalled. */
export function completeTimerBreak(original: TimerState, now = Date.now()): TimerState {
  if (original.phase === 'focus') return original
  const state: TimerState = {
    ...original,
    days: { ...original.days },
    history: [...original.history],
    updatedAt: now,
  }
  // The native deadline proves this break finished. Credit its outstanding duration,
  // not an unbounded wall-clock gap, and retain any seconds already recorded by ticks.
  recordTime(state.days, state.phase, now - state.remaining * 1000, now)
  transition(state, now)
  trimRecords(state, now)
  return state
}

export function skipTimerBreak(original: TimerState, now = Date.now()): TimerState {
  const state = advanceTimer(original, now)
  if (state.phase === 'focus' && !state.deferredBreak) return state
  const days = { ...state.days }
  getDay(days, now).skippedBreaks += 1
  const duration = getPhaseDuration('focus', state.settings)
  return {
    ...state, days, phase: 'focus', remaining: duration, phaseDuration: duration, running: true,
    breakId: null, deferredBreak: null, postponeUsed: false,
  }
}

export function changeTimerSettings(
  original: TimerState,
  partial: Partial<TimerSettings>,
  now = Date.now(),
): TimerState {
  const state = advanceTimer(original, now)
  const settings = normalizeSettings(partial, state.settings)
  // Configuration changes apply to future occurrences; they cannot push back a delay
  // or shorten the full break promised when the user postponed this occurrence.
  if (state.deferredBreak || (state.phase !== 'focus' && state.postponeUsed)) return { ...state, settings }
  const phaseDuration = getPhaseDuration(state.phase, settings)
  const elapsed = state.phaseDuration - state.remaining
  // A shorter duration takes effect on the next clock tick; avoid a zero-length phase.
  const remaining = Math.max(1, phaseDuration - elapsed)
  return { ...state, settings, phaseDuration, remaining }
}

export function resetTimerState(original: TimerState, now = Date.now()): TimerState {
  const state = advanceTimer(original, now)
  if (state.deferredBreak || (state.phase !== 'focus' && state.postponeUsed)) return state
  const duration = getPhaseDuration('focus', state.settings)
  return {
    ...state,
    phase: 'focus',
    remaining: duration,
    phaseDuration: duration,
    running: state.settings.autoStart,
    completedCycles: 0,
    breakId: null,
    deferredBreak: null,
    postponeUsed: false,
  }
}

/** Local storage is untrusted: accept only the known version and validate every field. */
export function restoreTimerState(serialized: string | null, now = Date.now()): TimerState {
  if (!serialized) return createTimerState(now)
  try {
    const data: unknown = JSON.parse(serialized)
    if (!isRecord(data) || data.version !== 1) return createTimerState(now)
    const settings = normalizeSettings(data.settings)
    const fallback = createTimerState(now, settings)
    const phase = data.phase === 'focus' || data.phase === 'short' || data.phase === 'long' ? data.phase : 'focus'
    let deferredBreak: TimerState['deferredBreak'] = null
    if (phase === 'focus' && isRecord(data.deferredBreak)) {
      const deferred = data.deferredBreak
      if ((deferred.type === 'short' || deferred.type === 'long') && finiteNumber(deferred.duration)
        && deferred.duration >= (deferred.type === 'short' ? 5 : 60)
        && deferred.duration <= (deferred.type === 'short' ? 300 : 3600)) {
        deferredBreak = { type: deferred.type, duration: deferred.duration }
      }
    }
    const postponeUsed = deferredBreak !== null || (phase !== 'focus' && data.postponeUsed === true)
    let duration = deferredBreak ? getPostponeSeconds(deferredBreak.type) : getPhaseDuration(phase, settings)
    if (phase !== 'focus' && postponeUsed && finiteNumber(data.phaseDuration)
      && data.phaseDuration >= (phase === 'short' ? 5 : 60)
      && data.phaseDuration <= (phase === 'short' ? 300 : 3600)) duration = data.phaseDuration
    const hasOccurrence = phase !== 'focus' || deferredBreak !== null
    const breakId = hasOccurrence
      ? typeof data.breakId === 'string' && data.breakId.length > 0 && data.breakId.length <= 200
        ? data.breakId : createBreakId(now)
      : null
    const days: Record<string, DailyStats> = {}
    if (isRecord(data.days)) {
      for (const [key, value] of Object.entries(data.days)) {
        if (!/^\d{4}-\d{2}-\d{2}$/.test(key) || !isRecord(value)) continue
        const stats = { ...EMPTY_STATS }
        for (const field of Object.keys(stats) as (keyof DailyStats)[]) {
          stats[field] = finiteNumber(value[field]) ? Math.max(0, value[field]) : 0
        }
        days[key] = stats
      }
    }
    const history: BreakHistoryEntry[] = []
    if (Array.isArray(data.history)) {
      for (const item of data.history) {
        if (!isRecord(item) || typeof item.id !== 'string' || (item.type !== 'short' && item.type !== 'long')) continue
        if (!finiteNumber(item.completedAt) || item.completedAt < 0 || item.completedAt > now) continue
        if (!finiteNumber(item.duration) || item.duration <= 0 || item.duration > 3600) continue
        history.push({ id: item.id, type: item.type, completedAt: item.completedAt, duration: item.duration })
      }
    }
    const state: TimerState = {
      ...fallback,
      phase,
      running: deferredBreak ? true : typeof data.running === 'boolean' ? data.running : settings.autoStart,
      remaining: finiteNumber(data.remaining) ? Math.max(0.001, Math.min(duration, data.remaining)) : duration,
      phaseDuration: duration,
      breakId,
      deferredBreak,
      postponeUsed,
      completedCycles: boundedNumber(data.completedCycles, 0, 0, 12),
      days,
      history: history.sort((a, b) => b.completedAt - a.completedAt),
      updatedAt: finiteNumber(data.updatedAt) && data.updatedAt >= 0 ? data.updatedAt : now,
    }
    trimRecords(state, now)
    const gap = now - state.updatedAt
    // A new page after a long absence resumes the saved countdown instead of inventing work.
    if (gap < 0 || gap > MAX_RESTORE_GAP_MS) return { ...state, updatedAt: now }
    return advanceTimer(state, now)
  } catch {
    return createTimerState(now)
  }
}
