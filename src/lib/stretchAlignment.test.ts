import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import * as THREE from 'three'
import { getStretchPose, STRETCH_JOINT_MAP, type JointName } from './stretchPoses.ts'
import { getStretchFraming } from './stretchFraming.ts'
import { STRETCH_EXERCISES, type StretchExerciseId } from './stretchRoutine.ts'

type BoneDefinition = { name: string; parent: string | null; position: [number, number, number] }
const asset = JSON.parse(readFileSync(new URL('../assets/stretch-human.json', import.meta.url), 'utf8')) as { bones: BoneDefinition[] }

// Check rendered skeleton landmarks, rather than merely asserting Euler angles.
function landmarks(id: StretchExerciseId, phase: number) {
  const group = new THREE.Group()
  const bones = Object.fromEntries(asset.bones.map(def => {
    const bone = new THREE.Bone()
    bone.position.fromArray(def.position)
    return [def.name, bone]
  }))
  for (const def of asset.bones) (def.parent ? bones[def.parent] : group).add(bones[def.name])
  const pose = getStretchPose(id, phase)
  for (const name of Object.keys(STRETCH_JOINT_MAP) as JointName[]) {
    bones[STRETCH_JOINT_MAP[name]].rotation.set(...pose.joints[name])
  }
  bones.head.position.z += pose.headRetraction * 0.55
  group.updateMatrixWorld(true)
  const point = (name: string) => bones[name].getWorldPosition(new THREE.Vector3())
  return { point, bones }
}

test('trapezius guide tilts away from the hand behind the hip on both sides', () => {
  for (const [phase, hand] of [[0.25, 'wrist.L'], [0.75, 'wrist.R']] as const) {
    const { point } = landmarks('upper-trapezius', phase)
    const handPoint = point(hand)
    assert.ok(handPoint.z < point('root').z - 0.2, 'active hand must be behind the hip')
    assert.ok((point('head').x - point('neck01').x) * handPoint.x < 0, 'head must tilt away from the active arm')
  }
})

test('shoulder roll landmarks move up, back, down and forward while hands stay low', () => {
  for (const side of ['L', 'R']) {
    const phases = [0, 0.25, 0.5, 0.75].map(phase => landmarks('shoulder-rolls', phase))
    const shoulders = phases.map(({ point }) => point(`upperarm01.${side}`))
    assert.ok(shoulders[0].y > shoulders[2].y + 0.04, 'shoulder lifts before lowering')
    assert.ok(shoulders[1].z < shoulders[3].z - 0.06, 'backward half precedes forward half')
    for (const { point } of phases) assert.ok(point(`wrist.${side}`).y < point(`upperarm01.${side}`).y - 0.7)
  }
})

test('upper-back rotation leaves the pelvis fixed and arms down', () => {
  const a = landmarks('upper-back-rotation', 0.25)
  const b = landmarks('upper-back-rotation', 0.75)
  assert.ok(a.point('root').distanceTo(b.point('root')) < 0.00001)
  for (const rig of [a, b]) {
    for (const side of ['L', 'R']) assert.ok(rig.point(`wrist.${side}`).y < rig.point(`upperarm01.${side}`).y - 0.8)
  }
  assert.ok(a.point('upperarm01.L').distanceTo(b.point('upperarm01.L')) > 0.2)
})

test('chest opener reaches behind without arching the spine', () => {
  const { point } = landmarks('chest-opener', 0.25)
  for (const side of ['L', 'R']) assert.ok(point(`wrist.${side}`).z < point('root').z - 0.15)
  const pose = getStretchPose('chest-opener', 0.25)
  assert.deepEqual(pose.joints.torso, [0, 0, 0])
  assert.deepEqual(pose.joints.chest, [0, 0, 0])
})

test('side bends raise the arm opposite the lean, with the feet fixed', () => {
  const neutral = landmarks('standing-side-bend', 0)
  for (const [phase, hand] of [[0.25, 'wrist.L'], [0.75, 'wrist.R']] as const) {
    const { point } = landmarks('standing-side-bend', phase)
    assert.ok(point(hand).y > point('head').y + 0.3)
    const side = hand.endsWith('.L') ? 'L' : 'R'
    assert.ok((point('head').x - point('root').x) * neutral.point(`upperarm01.${side}`).x < 0)
    for (const foot of ['foot.L', 'foot.R']) assert.ok(point(foot).distanceTo(neutral.point(foot)) < 0.00001)
  }
})

test('wrist guide reaches forward near shoulder height and bends the fingers downward', () => {
  for (const [phase, side] of [[0.25, 'R'], [0.75, 'L']] as const) {
    const { point } = landmarks('wrist-forearm', phase)
    const wrist = point(`wrist.${side}`)
    const shoulder = point(`upperarm01.${side}`)
    assert.ok(wrist.z > shoulder.z + 1)
    assert.ok(Math.abs(wrist.y - shoulder.y) < 0.35)
    assert.ok(point(`finger3-3.${side}`).y < wrist.y - 0.05)
  }
})

test('neck movements use a gentle tilt and chin tuck keeps the face level', () => {
  for (const phase of [0.25, 0.75]) {
    for (const id of ['neck-side-stretch', 'upper-trapezius'] as const) {
      const pose = getStretchPose(id, phase)
      assert.ok(Math.abs(pose.joints.neck[2] + pose.joints.head[2]) <= Math.PI / 8)
    }
    const { bones } = landmarks('chin-tuck', phase)
    const forward = new THREE.Vector3(0, 0, 1).applyQuaternion(bones.head.getWorldQuaternion(new THREE.Quaternion()))
    assert.ok(Math.abs(forward.y) < 0.01)
  }
})

test('framing retains head and active hands throughout each movement on wide and narrow stages', () => {
  for (const { id } of STRETCH_EXERCISES) {
    for (const aspect of [0.65, 0.9, 1.4, 2]) {
      const { yaw, viewHeight, targetY } = getStretchFraming(id, aspect)
      const camera = new THREE.OrthographicCamera(-viewHeight * aspect / 2, viewHeight * aspect / 2, viewHeight / 2, -viewHeight / 2, 0.1, 30)
      camera.position.set(Math.sin(yaw) * 9, targetY + 0.12, Math.cos(yaw) * 9)
      camera.lookAt(0, targetY, 0)
      camera.updateMatrixWorld(true)
      for (let sample = 0; sample < 32; sample++) {
        const { point } = landmarks(id, sample / 32)
        for (const name of ['head', 'wrist.L', 'wrist.R', 'finger3-3.L', 'finger3-3.R']) {
          const projected = point(name).project(camera)
          assert.ok(Math.abs(projected.x) < 0.9, `${id}: ${name} clipped horizontally at ${aspect}, phase ${sample / 32}`)
          assert.ok(Math.abs(projected.y) < 0.94, `${id}: ${name} clipped vertically at ${aspect}, phase ${sample / 32}`)
        }
      }
    }
  }
})
