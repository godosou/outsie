import assert from 'node:assert/strict'
import test from 'node:test'
import { STRETCH_EXERCISES } from './stretchRoutine.ts'
import { JOINT_NAMES, getStretchPose } from './stretchPoses.ts'

test('every exercise produces a complete set of finite joint rotations', () => {
  for (const exercise of STRETCH_EXERCISES) {
    for (const phase of [0, 0.25, 0.5, 0.75, 1]) {
      const pose = getStretchPose(exercise.id, phase)
      assert.deepEqual(Object.keys(pose.joints).sort(), [...JOINT_NAMES].sort())
      for (const rotation of Object.values(pose.joints)) {
        assert.equal(rotation.length, 3)
        assert.ok(rotation.every(Number.isFinite))
      }
      assert.ok(pose.rootPosition.every(Number.isFinite))
      assert.ok(Number.isFinite(pose.cameraYaw))
    }
  }
})

test('reduced motion returns one representative pose throughout the cycle', () => {
  for (const exercise of STRETCH_EXERCISES) {
    assert.deepEqual(
      getStretchPose(exercise.id, 0.1, true),
      getStretchPose(exercise.id, 0.9, true),
    )
  }
})

test('animated poses loop seamlessly and visibly move the targeted joints', () => {
  for (const exercise of STRETCH_EXERCISES) {
    assert.deepEqual(getStretchPose(exercise.id, 0), getStretchPose(exercise.id, 1))
    assert.notDeepEqual(
      getStretchPose(exercise.id, 0.25).joints,
      getStretchPose(exercise.id, 0.75).joints,
      `${exercise.title} should visibly change during its cycle`,
    )
  }
})

test('unknown animation phases are clamped to a safe finite pose', () => {
  const before = getStretchPose('chin-tuck', Number.NEGATIVE_INFINITY)
  const after = getStretchPose('chin-tuck', Number.POSITIVE_INFINITY)
  assert.deepEqual(before, after)
  for (const rotation of Object.values(before.joints)) assert.ok(rotation.every(Number.isFinite))
})
