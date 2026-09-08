import assert from 'node:assert/strict'
import test from 'node:test'
import { STRETCH_EXERCISES } from './stretchRoutine.ts'
import { JOINT_NAMES, getStretchPose } from './stretchPoses.ts'

test('neck demonstration holds the stretch before releasing and starts neutral', () => {
  assert.deepEqual(getStretchPose('neck-side-stretch', 0.19).joints, getStretchPose('neck-side-stretch', 0.28).joints)
  assert.equal(Math.abs(getStretchPose('chin-tuck', 0).headRetraction), 0)
})

test('shoulder circles animate the shoulder girdle, not only the arms', () => {
  const pose = getStretchPose('shoulder-rolls', 0)
  assert.ok('leftClavicle' in pose.joints)
  assert.notDeepEqual((pose.joints as Record<string, unknown>).leftClavicle, [0, 0, 0])
})

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

test('chin retraction keeps the head level, and upper-back turns keep the pelvis still', () => {
  const tuck = getStretchPose('chin-tuck', 0.25)
  assert.ok(tuck.headRetraction < -0.05)
  assert.equal(tuck.joints.neck[0] + tuck.joints.head[0], 0)
  assert.deepEqual(getStretchPose('upper-back-rotation', 0.25).joints.root, [0, 0, 0])
})

test('alternating movements have no discontinuity at the side-change boundary', () => {
  for (const exercise of STRETCH_EXERCISES) {
    const before = getStretchPose(exercise.id, 0.5 - 0.00001)
    const after = getStretchPose(exercise.id, 0.5 + 0.00001)
    for (const joint of JOINT_NAMES) {
      before.joints[joint].forEach((value, axis) => {
        assert.ok(Math.abs(value - after.joints[joint][axis]) < 0.01, `${exercise.id}: ${joint}`)
      })
    }
  }
})
