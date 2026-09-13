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

  const points = buildHourlyChart({ focusSeconds, meetingSeconds: Array(24).fill(0), breakSeconds })

  assert.equal(points.length, 24)
  assert.deepEqual(points[9], {
    hour: 9,
    focusSeconds: 1_200,
    meetingSeconds: 0,
    breakSeconds: 300,
    totalSeconds: 1_500,
    heightPercent: 100,
    focusPercent: 80,
    meetingPercent: 0,
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
  assert.equal(buildHourlyChart({ focusSeconds, meetingSeconds: Array(24).fill(0), breakSeconds })[4].heightPercent, 4)
})

test('default hour uses current hour today and the busiest hour in history', () => {
  const focusSeconds = Array(24).fill(0)
  const breakSeconds = Array(24).fill(0)
  focusSeconds[8] = 300
  breakSeconds[18] = 900
  const stats = { focusSeconds, meetingSeconds: Array(24).fill(0), breakSeconds }
  assert.equal(selectDefaultHour(stats, true, 14), 14)
  assert.equal(selectDefaultHour(stats, false, 14), 18)
  assert.equal(selectDefaultHour({ focusSeconds: Array(24).fill(0), meetingSeconds: Array(24).fill(0), breakSeconds: Array(24).fill(0) }, false, 14), 0)
})


test('meeting time is a third stacked segment and counts toward the busiest hour', () => {
  const focusSeconds = Array(24).fill(0)
  const meetingSeconds = Array(24).fill(0)
  const breakSeconds = Array(24).fill(0)
  focusSeconds[9] = 600
  meetingSeconds[9] = 300
  breakSeconds[9] = 100
  meetingSeconds[14] = 3_000

  const points = buildHourlyChart({ focusSeconds, meetingSeconds, breakSeconds })
  assert.equal(points[9].totalSeconds, 1_000)
  assert.equal(points[9].focusPercent, 60)
  assert.equal(points[9].meetingPercent, 30)
  assert.equal(points[9].breakPercent, 10)
  assert.equal(points[14].heightPercent, 100)
  assert.equal(selectDefaultHour({ focusSeconds, meetingSeconds, breakSeconds }, false, 9), 14)
})
