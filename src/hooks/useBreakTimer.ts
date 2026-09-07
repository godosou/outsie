import { useCallback, useEffect, useRef, useState } from 'react'
import {
  DEFAULT_SETTINGS,
  STORAGE_KEY,
  advanceTimer,
  changeTimerSettings,
  completeTimerBreak,
  deriveWeeklyStats,
  getTodayStats,
  getPostponeSeconds,
  localDateKey,
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
  stateRef.current = state

  useEffect(() => {
    const tick = () => setState((previous) => advanceTimer(previous, Date.now()))
    const interval = window.setInterval(tick, 1000)
    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') tick()
    }
    const onPageHide = () => saveState(advanceTimer(stateRef.current, Date.now()))
    document.addEventListener('visibilitychange', onVisibilityChange)
    window.addEventListener('pagehide', onPageHide)
    return () => {
      window.clearInterval(interval)
      document.removeEventListener('visibilitychange', onVisibilityChange)
      window.removeEventListener('pagehide', onPageHide)
    }
  }, [])

  // The countdown is reconstructed from wall-clock time, so writing the whole
  // history every second only creates needless background I/O. Page exit still
  // persists immediately; while running, a 15-second checkpoint is sufficient.
  useEffect(() => {
    const now = Date.now()
    if (now - savedAt.current < 15_000) return
    saveState(state)
    savedAt.current = now
  }, [state])

  const toggleRunning = useCallback(() => setState((previous) => toggleTimer(previous)), [])
  const startBreak = useCallback((type: 'short' | 'long') => setState((previous) => startTimerBreak(previous, type)), [])
  const completeBreak = useCallback(() => setState((previous) => completeTimerBreak(previous)), [])
  const skipBreak = useCallback(() => setState((previous) => skipTimerBreak(previous)), [])
  const postponeBreak = useCallback(() => setState((previous) => postponeTimerBreak(previous)), [])
  const updateSettings = useCallback((partial: Partial<TimerSettings>) => {
    setState((previous) => changeTimerSettings(previous, partial))
  }, [])
  const resetSettings = useCallback(() => {
    setState((previous) => changeTimerSettings(previous, DEFAULT_SETTINGS))
  }, [])
  const resetTimer = useCallback(() => setState((previous) => resetTimerState(previous)), [])

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
