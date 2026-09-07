import assert from 'node:assert/strict'
import test from 'node:test'
import {
  STRETCH_EXERCISES,
  STRETCH_STEP_SECONDS,
  getStretchStep,
} from './stretchRoutine.ts'

test('the routine exposes eight unique and safe office stretches', () => {
  assert.equal(STRETCH_EXERCISES.length, 8)
  assert.equal(new Set(STRETCH_EXERCISES.map(exercise => exercise.id)).size, 8)
  assert.ok(STRETCH_EXERCISES.filter(exercise => exercise.focus === '肩颈与上背').length >= 5)
  assert.ok(STRETCH_EXERCISES.every(exercise => exercise.cue.length > 0))
  assert.ok(STRETCH_EXERCISES.every(exercise => exercise.safety.length > 0))
})

test('the routine advances every thirty seconds and loops', () => {
  assert.equal(STRETCH_STEP_SECONDS, 30)
  assert.equal(getStretchStep(300, 300).index, 0)
  assert.equal(getStretchStep(270, 300).index, 1)
  assert.equal(getStretchStep(60, 300).index, 0)
  assert.equal(getStretchStep(30, 300).index, 1)
})

test('a manual offset wraps without changing the break clock', () => {
  const previous = getStretchStep(270, 300, -2)
  const next = getStretchStep(270, 300, 9)
  assert.equal(previous.index, 7)
  assert.equal(next.index, 2)
  assert.equal(previous.breakElapsed, 30)
  assert.equal(next.breakElapsed, 30)
})

test('routine progress is clamped for late or invalid countdown updates', () => {
  assert.deepEqual(getStretchStep(500, 300), {
    index: 0,
    exercise: STRETCH_EXERCISES[0],
    progress: 0,
    stepRemaining: 30,
    breakElapsed: 0,
  })
  const late = getStretchStep(-5, 300)
  assert.equal(late.breakElapsed, 300)
  assert.equal(late.progress, 0)
  assert.equal(late.stepRemaining, 30)
})
