import assert from 'node:assert/strict'
import test from 'node:test'
import { advanceStretchPlayback } from './stretchPlayback.ts'

test('pausing freezes the pose and resuming continues without a jump', () => {
  let elapsed = advanceStretchPlayback(1200, 16, true)
  assert.equal(elapsed, 1216)
  elapsed = advanceStretchPlayback(elapsed, 100, false)
  assert.equal(elapsed, 1216)
  assert.equal(advanceStretchPlayback(elapsed, 16, true), 1232)
})

test('suspension and invalid clock deltas do not replay missed movement', () => {
  for (const delta of [10000, -1, NaN, Infinity]) {
    assert.equal(advanceStretchPlayback(1200, delta, true), 1200)
  }
})
