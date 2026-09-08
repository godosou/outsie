import assert from 'node:assert/strict'
import test from 'node:test'
import { buildHourlyChart, selectDefaultHour } from './activityChart.ts'

test('hourly chart builds 24 stacked points scaled to the busiest hour', () => {
  const focusSeconds = Array(24).fill(0)
  const breakSeconds = Array(24).fill(0)
  focusSeconds[9] = 1_200
  breakSeconds[9] = 300
  focusSeconds[10] = 300
  breakSeconds[10] = 300

  const points = buildHourlyChart({ focusSeconds, breakSeconds })

  assert.equal(points.length, 24)
  assert.deepEqual(points[9], {
    hour: 9,
    focusSeconds: 1_200,
    breakSeconds: 300,
    totalSeconds: 1_500,
    heightPercent: 100,
    focusPercent: 80,
    breakPercent: 20,
  })
  assert.equal(points[10].heightPercent, 40)
  assert.equal(points[8].heightPercent, 0)
})

test('tiny non-zero activity remains visible in the hourly chart', () => {
  const focusSeconds = Array(24).fill(0)
  const breakSeconds = Array(24).fill(0)
  focusSeconds[4] = 1
  focusSeconds[12] = 3_600
  assert.equal(buildHourlyChart({ focusSeconds, breakSeconds })[4].heightPercent, 4)
})

test('default hour uses current hour today and the busiest hour in history', () => {
  const focusSeconds = Array(24).fill(0)
  const breakSeconds = Array(24).fill(0)
  focusSeconds[8] = 300
  breakSeconds[18] = 900
  const stats = { focusSeconds, breakSeconds }
  assert.equal(selectDefaultHour(stats, true, 14), 14)
  assert.equal(selectDefaultHour(stats, false, 14), 18)
  assert.equal(selectDefaultHour({ focusSeconds: Array(24).fill(0), breakSeconds: Array(24).fill(0) }, false, 14), 0)
})
