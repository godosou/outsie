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

export interface HourlyStats {
  focusSeconds: number[]
  breakSeconds: number[]
}

export interface TimerState {
  version: 3
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
  hourly: Record<string, HourlyStats>
  history: BreakHistoryEntry[]
  lifecycleIntervalIds: string[]
  updatedAt: number
}

export interface InactivityContext {
  phase: TimerPhase
  running: boolean
  breakId: string | null
}

export interface InactivityInterval {
  intervalId: string
  elapsedSeconds: number
  startedAt: number
  endedAt: number
}

export interface WeeklyStats extends DailyStats {
  date: string
  label: string
}

export const STORAGE_KEY = 'repose.timer.v1'
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

function emptyHourlyStats(): HourlyStats {
  return {
    focusSeconds: Array(24).fill(0),
    breakSeconds: Array(24).fill(0),
  }
}

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

/** Convert two in-process monotonic samples to trusted active elapsed seconds. */
export function monotonicElapsedSeconds(previousMs: number, currentMs: number, inactive: boolean): number {
  if (inactive || !finiteNumber(previousMs) || !finiteNumber(currentMs) || currentMs <= previousMs) return 0
  return (currentMs - previousMs) / 1000
}

function createBreakId(at: number): string {
  return `${at}-${globalThis.crypto.randomUUID()}`
}

export function createTimerState(now = Date.now(), settings?: Partial<TimerSettings>): TimerState {
  const normalizedSettings = normalizeSettings(settings)
  const duration = getPhaseDuration('focus', normalizedSettings)
  return {
    version: 3,
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
    hourly: {},
    history: [],
    lifecycleIntervalIds: [],
    updatedAt: now,
  }
}

export function getTodayStats(state: TimerState, now = Date.now()): DailyStats {
  return { ...(state.days[localDateKey(now)] ?? EMPTY_STATS) }
}

export function getHourlyStats(state: TimerState, now = Date.now()): HourlyStats {
  const stats = state.hourly[localDateKey(now)] ?? emptyHourlyStats()
  return {
    focusSeconds: [...stats.focusSeconds],
    breakSeconds: [...stats.breakSeconds],
  }
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

function getHour(hourly: Record<string, HourlyStats>, timestamp: number): HourlyStats {
  const key = localDateKey(timestamp)
  const existing = hourly[key] ?? emptyHourlyStats()
  const stats = {
    focusSeconds: [...existing.focusSeconds],
    breakSeconds: [...existing.breakSeconds],
  }
  hourly[key] = stats
  return stats
}

/** Split trusted elapsed time at local hour boundaries, including midnight. */
function recordTime(
  days: Record<string, DailyStats>,
  hourly: Record<string, HourlyStats>,
  phase: TimerPhase,
  from: number,
  to: number,
) {
  let cursor = from
  while (cursor < to) {
    const nextHour = new Date(cursor)
    nextHour.setMinutes(0, 0, 0)
    nextHour.setHours(nextHour.getHours() + 1)
    const end = Math.min(to, nextHour.getTime())
    const hour = new Date(cursor).getHours()
    const stats = getDay(days, cursor)
    const hourlyStats = getHour(hourly, cursor)
    const seconds = (end - cursor) / 1000
    if (phase === 'focus') {
      stats.focusSeconds += seconds
      hourlyStats.focusSeconds[hour] += seconds
    } else {
      stats.breakSeconds += seconds
      hourlyStats.breakSeconds[hour] += seconds
    }
    cursor = end
  }
}

function trimRecords(state: TimerState, now: number) {
  const oldest = new Date(now)
  oldest.setDate(oldest.getDate() - 34)
  const cutoff = localDateKey(oldest.getTime())
  state.days = Object.fromEntries(Object.entries(state.days).filter(([key]) => key >= cutoff))
  state.hourly = Object.fromEntries(Object.entries(state.hourly).filter(([key]) => key >= cutoff))
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

/** Advance only by a trusted elapsed duration; wall time is for attribution. */
export function advanceTimerBy(original: TimerState, elapsedSeconds: number, now = Date.now()): TimerState {
  if (!finiteNumber(elapsedSeconds) || elapsedSeconds <= 0 || !original.running) {
    return { ...original, updatedAt: now }
  }
  const state: TimerState = {
    ...original,
    days: { ...original.days },
    hourly: { ...original.hourly },
    history: [...original.history],
    updatedAt: now,
  }
  const elapsed = Math.min(elapsedSeconds, state.remaining)
  recordTime(state.days, state.hourly, state.phase, now - elapsed * 1000, now)
  state.remaining = Math.max(0, state.remaining - elapsed)
  if (state.remaining <= 0.000001) transition(state, now)
  trimRecords(state, now)
  return state
}

export function captureInactivity(state: TimerState): InactivityContext {
  return { phase: state.phase, running: state.running, breakId: state.breakId }
}

function getDueBreak(state: TimerState): { type: 'short' | 'long'; duration: number } {
  if (state.deferredBreak) return state.deferredBreak
  const type = state.completedCycles >= state.settings.longEvery ? 'long' : 'short'
  return { type, duration: getPhaseDuration(type, state.settings) }
}

export function applyInactivityInterval(
  original: TimerState,
  context: InactivityContext,
  interval: InactivityInterval,
): TimerState {
  if (!interval.intervalId || original.lifecycleIntervalIds.includes(interval.intervalId)
    || !finiteNumber(interval.elapsedSeconds) || interval.elapsedSeconds < 0) return original
  const lifecycleIntervalIds = [...original.lifecycleIntervalIds, interval.intervalId].slice(-32)
  const state: TimerState = {
    ...original,
    lifecycleIntervalIds,
    updatedAt: interval.endedAt,
  }
  if (!context.running || interval.elapsedSeconds === 0) return state
  if (context.phase !== 'focus') {
    if (original.phase !== context.phase || original.breakId !== context.breakId) return state
    return {
      ...advanceTimerBy(original, interval.elapsedSeconds, interval.endedAt),
      lifecycleIntervalIds,
    }
  }
  if (original.phase !== 'focus') return state
  state.days = { ...original.days }
  state.hourly = { ...original.hourly }
  state.history = [...original.history]
  recordTime(
    state.days,
    state.hourly,
    'short',
    interval.endedAt - interval.elapsedSeconds * 1000,
    interval.endedAt,
  )
  const due = getDueBreak(state)
  if (interval.elapsedSeconds + 0.000001 >= due.duration) {
    getDay(state.days, interval.endedAt).completedBreaks += 1
    state.history.unshift({
      id: `passive-${interval.intervalId}`,
      type: due.type,
      completedAt: interval.endedAt,
      duration: due.duration,
    })
    state.completedCycles = due.type === 'long' ? 0 : state.completedCycles + 1
    state.phase = 'focus'
    state.running = state.settings.autoStart
    state.phaseDuration = getPhaseDuration('focus', state.settings)
    state.remaining = state.phaseDuration
    state.breakId = null
    state.deferredBreak = null
    state.postponeUsed = false
  }
  trimRecords(state, interval.endedAt)
  return state
}

export function toggleTimer(original: TimerState, now = Date.now(), wasRunning = original.running): TimerState {
  if (original.deferredBreak) return original
  // Preserve the user's pause/resume intent if a phase ended between render and click.
  return { ...original, running: !wasRunning, updatedAt: now }
}

export function startTimerBreak(original: TimerState, type: 'short' | 'long', now = Date.now()): TimerState {
  // Repeated native/menu actions must not replace an already active occurrence.
  if (original.phase !== 'focus') return original
  if (original.deferredBreak) {
    return {
      ...original,
      phase: original.deferredBreak.type,
      remaining: original.deferredBreak.duration,
      phaseDuration: original.deferredBreak.duration,
      deferredBreak: null,
      running: true,
      updatedAt: now,
    }
  }
  const duration = getPhaseDuration(type, original.settings)
  return {
    ...original,
    phase: type,
    remaining: duration,
    phaseDuration: duration,
    running: true,
    breakId: createBreakId(now),
    deferredBreak: null,
    postponeUsed: false,
    updatedAt: now,
  }
}

/** Delay this occurrence once without completing it or advancing its cycle. */
export function postponeTimerBreak(original: TimerState, now = Date.now()): TimerState {
  if (original.phase === 'focus' || original.postponeUsed) return original
  const duration = getPostponeSeconds(original.phase)
  return {
    ...original,
    phase: 'focus',
    remaining: duration,
    phaseDuration: duration,
    running: true,
    deferredBreak: { type: original.phase, duration: original.phaseDuration },
    postponeUsed: true,
    updatedAt: now,
  }
}

/** Accept completion from the native strict-break deadline, even if renderer ticks stalled. */
export function completeTimerBreak(
  original: TimerState,
  expectedBreakId: string,
  now = Date.now(),
): TimerState {
  if (original.phase === 'focus' || !original.breakId || original.breakId !== expectedBreakId) return original
  const state: TimerState = {
    ...original,
    days: { ...original.days },
    hourly: { ...original.hourly },
    history: [...original.history],
    updatedAt: now,
  }
  // The native deadline proves this break finished. Credit its outstanding duration,
  // not an unbounded wall-clock gap, and retain any seconds already recorded by ticks.
  recordTime(state.days, state.hourly, state.phase, now - state.remaining * 1000, now)
  transition(state, now)
  trimRecords(state, now)
  return state
}

export function skipTimerBreak(original: TimerState, now = Date.now()): TimerState {
  if (original.phase === 'focus' && !original.deferredBreak) return original
  const days = { ...original.days }
  getDay(days, now).skippedBreaks += 1
  const duration = getPhaseDuration('focus', original.settings)
  return {
    ...original, days, phase: 'focus', remaining: duration, phaseDuration: duration, running: true,
    breakId: null, deferredBreak: null, postponeUsed: false, updatedAt: now,
  }
}

export function changeTimerSettings(
  original: TimerState,
  partial: Partial<TimerSettings>,
  now = Date.now(),
): TimerState {
  const settings = normalizeSettings(partial, original.settings)
  // Configuration changes apply to future occurrences; they cannot push back a delay
  // or shorten the full break promised when the user postponed this occurrence.
  if (original.deferredBreak || (original.phase !== 'focus' && original.postponeUsed)) {
    return { ...original, settings, updatedAt: now }
  }
  const phaseDuration = getPhaseDuration(original.phase, settings)
  const elapsed = original.phaseDuration - original.remaining
  // A shorter duration takes effect on the next clock tick; avoid a zero-length phase.
  const remaining = Math.max(1, phaseDuration - elapsed)
  return { ...original, settings, phaseDuration, remaining, updatedAt: now }
}

export function resetTimerState(original: TimerState, now = Date.now()): TimerState {
  if (original.deferredBreak || (original.phase !== 'focus' && original.postponeUsed)) return original
  const duration = getPhaseDuration('focus', original.settings)
  return {
    ...original,
    phase: 'focus',
    remaining: duration,
    phaseDuration: duration,
    running: original.settings.autoStart,
    completedCycles: 0,
    breakId: null,
    deferredBreak: null,
    postponeUsed: false,
    updatedAt: now,
  }
}

/** Local storage is untrusted: accept only the known version and validate every field. */
export function restoreTimerState(serialized: string | null, now = Date.now()): TimerState {
  if (!serialized) return createTimerState(now)
  try {
    const data: unknown = JSON.parse(serialized)
    if (!isRecord(data) || (data.version !== 1 && data.version !== 2 && data.version !== 3)) {
      return createTimerState(now)
    }
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
    const hourly: Record<string, HourlyStats> = {}
    if (data.version === 3 && isRecord(data.hourly)) {
      const restoreSeries = (value: unknown): number[] => {
        if (!Array.isArray(value) || value.length !== 24) return Array(24).fill(0)
        return value.map(item => finiteNumber(item) && item >= 0 ? item : 0)
      }
      for (const [key, value] of Object.entries(data.hourly)) {
        if (!/^\d{4}-\d{2}-\d{2}$/.test(key) || !isRecord(value)) continue
        hourly[key] = {
          focusSeconds: restoreSeries(value.focusSeconds),
          breakSeconds: restoreSeries(value.breakSeconds),
        }
      }
    }
    const state: TimerState = {
      ...fallback,
      version: 3,
      phase,
      running: deferredBreak ? true : typeof data.running === 'boolean' ? data.running : settings.autoStart,
      remaining: finiteNumber(data.remaining) ? Math.max(0.001, Math.min(duration, data.remaining)) : duration,
      phaseDuration: duration,
      breakId,
      deferredBreak,
      postponeUsed,
      completedCycles: boundedNumber(data.completedCycles, 0, 0, 12),
      days,
      hourly,
      history: history.sort((a, b) => b.completedAt - a.completedAt),
      lifecycleIntervalIds: data.version !== 1
        ? Array.from(new Set([
          ...(Array.isArray(data.lifecycleIntervalIds) ? data.lifecycleIntervalIds : []),
          data.lastLifecycleIntervalId,
        ].filter((id): id is string => typeof id === 'string' && id.length > 0 && id.length <= 200))).slice(-32)
        : [],
      updatedAt: finiteNumber(data.updatedAt) && data.updatedAt >= 0 ? data.updatedAt : now,
    }
    trimRecords(state, now)
    // A new process resumes the saved countdown without inventing activity.
    return { ...state, updatedAt: now }
  } catch {
    return createTimerState(now)
  }
}
