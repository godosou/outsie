import { useCallback, useEffect, useRef, useState } from 'react'
import {
  DEFAULT_SETTINGS,
  STORAGE_KEY,
  advanceTimerBy,
  applyInactivityInterval,
  captureInactivity,
  changeTimerSettings,
  completeTimerBreak,
  deriveWeeklyStats,
  getHourlyStats,
  getTodayStats,
  getPostponeSeconds,
  localDateKey,
  monotonicElapsedSeconds,
  postponeTimerBreak,
  resetTimerState,
  restoreTimerState,
  skipTimerBreak,
  startTimerBreak,
  toggleTimer,
  type TimerSettings,
  type TimerState,
} from '../lib/timer'

function loadState(): TimerState {
  try {
    return restoreTimerState(window.localStorage.getItem(STORAGE_KEY))
  } catch {
    return restoreTimerState(null)
  }
}

function saveState(state: TimerState) {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
  } catch {
    // The timer also works when storage is unavailable or full.
  }
}

export function useBreakTimer() {
  const [state, setState] = useState<TimerState>(loadState)
  const stateRef = useRef(state)
  const savedAt = useRef(0)
  const monotonicAt = useRef(performance.now())
  const inactivityRef = useRef<{
    intervalId: string
    context: ReturnType<typeof captureInactivity>
  } | null>(null)
  stateRef.current = state

  const commit = useCallback((transform: (previous: TimerState) => TimerState) => {
    const next = transform(stateRef.current)
    stateRef.current = next
    setState(next)
    return next
  }, [])

  const sampleActiveTime = useCallback((wallNow = Date.now()) => {
    const current = performance.now()
    const elapsed = monotonicElapsedSeconds(monotonicAt.current, current, inactivityRef.current !== null)
    monotonicAt.current = current
    if (elapsed <= 0) return stateRef.current
    return commit(previous => advanceTimerBy(previous, elapsed, wallNow))
  }, [commit])

  const checkpoint = useCallback((next: TimerState) => {
    saveState(next)
    savedAt.current = Date.now()
  }, [])

  useEffect(() => {
    const tick = () => sampleActiveTime()
    const interval = window.setInterval(tick, 1000)
    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') tick()
    }
    const onPageHide = () => checkpoint(sampleActiveTime())
    const unsubscribeLifecycle = window.repose?.onLifecycle(event => {
      if (event.type === 'inactive-start') {
        if (inactivityRef.current) return
        const frozen = sampleActiveTime(event.startedAt)
        inactivityRef.current = {
          intervalId: event.intervalId,
          context: captureInactivity(frozen),
        }
        checkpoint(frozen)
        return
      }

      const active = inactivityRef.current
      const context = active?.intervalId === event.intervalId
        ? active.context
        : captureInactivity(stateRef.current)
      const next = commit(previous => applyInactivityInterval(previous, context, event))
      if (!active || active.intervalId === event.intervalId) inactivityRef.current = null
      // performance.now() may include sleep on some platforms. Reset the sample
      // boundary so the lifecycle interval is the only source of inactive time.
      monotonicAt.current = performance.now()
      checkpoint(next)
      void window.repose?.acknowledgeLifecycle(event.intervalId)
    })
    document.addEventListener('visibilitychange', onVisibilityChange)
    window.addEventListener('pagehide', onPageHide)
    return () => {
      window.clearInterval(interval)
      unsubscribeLifecycle?.()
      document.removeEventListener('visibilitychange', onVisibilityChange)
      window.removeEventListener('pagehide', onPageHide)
    }
  }, [checkpoint, commit, sampleActiveTime])

  // Monotonic samples update memory every second. A 15-second storage checkpoint
  // avoids needless background I/O; lifecycle edges and page exit persist at once.
  useEffect(() => {
    const now = Date.now()
    if (now - savedAt.current < 15_000) return
    saveState(state)
    savedAt.current = now
  }, [state])

  const toggleRunning = useCallback(() => {
    const wasRunning = stateRef.current.running
    const now = Date.now()
    sampleActiveTime(now)
    commit(previous => toggleTimer(previous, now, wasRunning))
  }, [commit, sampleActiveTime])
  const startBreak = useCallback((type: 'short' | 'long') => {
    const now = Date.now()
    sampleActiveTime(now)
    commit(previous => startTimerBreak(previous, type, now))
  }, [commit, sampleActiveTime])
  const completeBreak = useCallback((expectedBreakId: string) => {
    const now = Date.now()
    sampleActiveTime(now)
    commit(previous => completeTimerBreak(previous, expectedBreakId, now))
  }, [commit, sampleActiveTime])
  const skipBreak = useCallback(() => {
    const now = Date.now()
    sampleActiveTime(now)
    commit(previous => skipTimerBreak(previous, now))
  }, [commit, sampleActiveTime])
  const postponeBreak = useCallback(() => {
    const now = Date.now()
    sampleActiveTime(now)
    commit(previous => postponeTimerBreak(previous, now))
  }, [commit, sampleActiveTime])
  const updateSettings = useCallback((partial: Partial<TimerSettings>) => {
    const now = Date.now()
    sampleActiveTime(now)
    commit(previous => changeTimerSettings(previous, partial, now))
  }, [commit, sampleActiveTime])
  const resetSettings = useCallback(() => {
    const now = Date.now()
    sampleActiveTime(now)
    commit(previous => changeTimerSettings(previous, DEFAULT_SETTINGS, now))
  }, [commit, sampleActiveTime])
  const resetTimer = useCallback(() => {
    const now = Date.now()
    sampleActiveTime(now)
    commit(previous => resetTimerState(previous, now))
  }, [commit, sampleActiveTime])
  const getStatsForDate = useCallback((timestamp: number) => getTodayStats(stateRef.current, timestamp), [])
  const getHourlyStatsForDate = useCallback((timestamp: number) => getHourlyStats(stateRef.current, timestamp), [])
  const getHistoryForDate = useCallback((timestamp: number) => {
    const key = localDateKey(timestamp)
    return stateRef.current.history.filter(entry => localDateKey(entry.completedAt) === key)
  }, [])

  const now = Date.now()
  const today = localDateKey(now)
  return {
    phase: state.phase,
    breakId: state.breakId,
    canPostpone: state.phase !== 'focus' && !state.postponeUsed,
    postponedBreak: state.deferredBreak?.type ?? null,
    postponeSeconds: getPostponeSeconds(state.deferredBreak?.type ?? (state.phase === 'long' ? 'long' : 'short')),
    running: state.running,
    remaining: Math.ceil(state.remaining),
    progress: Math.max(0, Math.min(1, 1 - state.remaining / state.phaseDuration)),
    settings: state.settings,
    stats: getTodayStats(state, now),
    history: state.history.filter((entry) => localDateKey(entry.completedAt) === today),
    completedCycles: state.completedCycles,
    weeklyStats: deriveWeeklyStats(state, now),
    getStatsForDate,
    getHourlyStatsForDate,
    getHistoryForDate,
    toggleRunning,
    startBreak,
    completeBreak,
    skipBreak,
    postponeBreak,
    updateSettings,
    resetSettings,
    resetTimer,
  }
}
