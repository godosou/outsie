import type { HourlyStats } from './timer'

export interface HourlyChartPoint {
  hour: number
  focusSeconds: number
  breakSeconds: number
  totalSeconds: number
  heightPercent: number
  focusPercent: number
  breakPercent: number
}

export function buildHourlyChart(stats: HourlyStats): HourlyChartPoint[] {
  const totals = Array.from({ length: 24 }, (_, hour) =>
    Math.max(0, stats.focusSeconds[hour] ?? 0) + Math.max(0, stats.breakSeconds[hour] ?? 0),
  )
  const maximum = Math.max(1, ...totals)
  return totals.map((totalSeconds, hour) => {
    const focusSeconds = Math.max(0, stats.focusSeconds[hour] ?? 0)
    const breakSeconds = Math.max(0, stats.breakSeconds[hour] ?? 0)
    return {
      hour,
      focusSeconds,
      breakSeconds,
      totalSeconds,
      heightPercent: totalSeconds === 0 ? 0 : Math.max(4, totalSeconds / maximum * 100),
      focusPercent: totalSeconds === 0 ? 0 : focusSeconds / totalSeconds * 100,
      breakPercent: totalSeconds === 0 ? 0 : breakSeconds / totalSeconds * 100,
    }
  })
}

export function selectDefaultHour(stats: HourlyStats, isToday: boolean, currentHour: number): number {
  if (isToday) return Math.max(0, Math.min(23, Math.floor(currentHour)))
  let selected = 0
  let maximum = 0
  for (let hour = 0; hour < 24; hour += 1) {
    const total = Math.max(0, stats.focusSeconds[hour] ?? 0) + Math.max(0, stats.breakSeconds[hour] ?? 0)
    if (total > maximum) {
      maximum = total
      selected = hour
    }
  }
  return selected
}

