import type { HourlyStats } from './timer'

export interface HourlyChartPoint {
  hour: number
  focusSeconds: number
  meetingSeconds: number
  breakSeconds: number
  totalSeconds: number
  heightPercent: number
  focusPercent: number
  meetingPercent: number
  breakPercent: number
}

function hourTotal(stats: HourlyStats, hour: number): number {
  return Math.max(0, stats.focusSeconds[hour] ?? 0)
    + Math.max(0, stats.meetingSeconds?.[hour] ?? 0)
    + Math.max(0, stats.breakSeconds[hour] ?? 0)
}

export function buildHourlyChart(stats: HourlyStats): HourlyChartPoint[] {
  const totals = Array.from({ length: 24 }, (_, hour) => hourTotal(stats, hour))
  const maximum = Math.max(1, ...totals)
  return totals.map((totalSeconds, hour) => {
    const focusSeconds = Math.max(0, stats.focusSeconds[hour] ?? 0)
    const meetingSeconds = Math.max(0, stats.meetingSeconds?.[hour] ?? 0)
    const breakSeconds = Math.max(0, stats.breakSeconds[hour] ?? 0)
    return {
      hour,
      focusSeconds,
      meetingSeconds,
      breakSeconds,
      totalSeconds,
      heightPercent: totalSeconds === 0 ? 0 : Math.max(4, totalSeconds / maximum * 100),
      focusPercent: totalSeconds === 0 ? 0 : focusSeconds / totalSeconds * 100,
      meetingPercent: totalSeconds === 0 ? 0 : meetingSeconds / totalSeconds * 100,
      breakPercent: totalSeconds === 0 ? 0 : breakSeconds / totalSeconds * 100,
    }
  })
}

export function selectDefaultHour(stats: HourlyStats, isToday: boolean, currentHour: number): number {
  if (isToday) return Math.max(0, Math.min(23, Math.floor(currentHour)))
  let selected = 0
  let maximum = 0
  for (let hour = 0; hour < 24; hour += 1) {
    const total = hourTotal(stats, hour)
    if (total > maximum) {
      maximum = total
      selected = hour
    }
  }
  return selected
}

