import type { StretchExerciseId } from './stretchRoutine.ts'

export const JOINT_NAMES = [
  'root',
  'torso',
  'chest',
  'neck',
  'head',
  'leftClavicle',
  'rightClavicle',
  'leftShoulder',
  'rightShoulder',
  'leftElbow',
  'rightElbow',
  'leftWrist',
  'rightWrist',
  'leftHip',
  'rightHip',
  'leftKnee',
  'rightKnee',
] as const

export type JointName = (typeof JOINT_NAMES)[number]
export type Rotation = readonly [number, number, number]
export type StretchPose = {
  joints: Record<JointName, Rotation>
  rootPosition: Rotation
  cameraYaw: number
  headRetraction: number
}

const rotation = (x = 0, y = 0, z = 0): Rotation => [x, y, z]

function neutralJoints(): Record<JointName, Rotation> {
  return Object.fromEntries(JOINT_NAMES.map(name => [name, rotation()])) as Record<JointName, Rotation>
}

type PoseContext = {
  joints: Record<JointName, Rotation>
  wave: number
  pulse: number
  orbit: number
  cameraYaw: number
  headRetraction: number
  rootPosition: Rotation
}

type PoseFactory = (context: PoseContext) => void

const poseFactories: Record<StretchExerciseId, PoseFactory> = {
  'chin-tuck': context => {
    context.headRetraction = -0.1 * context.pulse
    context.joints.neck = rotation(-0.025 * context.pulse)
    context.joints.head = rotation(0.025 * context.pulse)
  },
  'neck-side-stretch': ({ joints, wave }) => {
    joints.neck = rotation(0, 0, wave * 0.15)
    joints.head = rotation(0.02, 0, wave * 0.38)
    joints.leftShoulder = rotation(0, 0, -Math.max(0, wave) * 0.08)
    joints.rightShoulder = rotation(0, 0, Math.min(0, wave) * 0.08)
  },
  'shoulder-rolls': ({ joints, wave, orbit }) => {
    const lift = orbit * 0.14
    joints.leftClavicle = rotation(0, wave * 0.12, -(orbit + 1) * 0.065)
    joints.rightClavicle = rotation(0, -wave * 0.12, (orbit + 1) * 0.065)
    joints.leftShoulder = rotation(-wave * 0.22, 0, -0.08 + lift)
    joints.rightShoulder = rotation(-wave * 0.22, 0, 0.08 - lift)
    joints.chest = rotation(wave * 0.025)
  },
  'upper-trapezius': ({ joints, wave }) => {
    const side = wave >= 0 ? 1 : -1
    const reach = Math.abs(wave)
    joints.neck = rotation(0, -wave * 0.03, wave * 0.12)
    joints.head = rotation(0, -wave * 0.04, wave * 0.32)
    joints.leftShoulder = rotation(side > 0 ? 0.32 * reach : 0, side > 0 ? -0.18 * reach : 0, -0.05)
    joints.rightShoulder = rotation(side < 0 ? 0.32 * reach : 0, side < 0 ? 0.18 * reach : 0, 0.05)
    joints.leftElbow = rotation(side > 0 ? -0.18 * reach : 0)
    joints.rightElbow = rotation(side < 0 ? -0.18 * reach : 0)
  },
  'chest-opener': ({ joints, pulse }) => {
    const open = 0.25 + pulse * 0.75
    joints.chest = rotation(-open * 0.08)
    joints.leftShoulder = rotation(open * 0.68, -open * 0.25, -open * 0.16)
    joints.rightShoulder = rotation(open * 0.68, open * 0.25, open * 0.16)
    joints.leftElbow = rotation(-open * 0.42, 0, -open * 0.22)
    joints.rightElbow = rotation(-open * 0.42, 0, open * 0.22)
  },
  'upper-back-rotation': ({ joints, wave }) => {
    joints.torso = rotation(0, wave * 0.18)
    joints.chest = rotation(0, wave * 0.34)
    joints.neck = rotation(0, -wave * 0.12)
    joints.leftShoulder = rotation(-0.5, 0, -0.82)
    joints.rightShoulder = rotation(-0.5, 0, 0.82)
    joints.leftElbow = rotation(0, 0, -1.18)
    joints.rightElbow = rotation(0, 0, 1.18)
  },
  'wrist-forearm': ({ joints, wave }) => {
    const left = Math.max(0, wave)
    const right = Math.max(0, -wave)
    joints.leftShoulder = rotation(-1.4 * left, 0, -0.08)
    joints.rightShoulder = rotation(-1.4 * right, 0, 0.08)
    joints.leftWrist = rotation(0.5 * left)
    joints.rightWrist = rotation(0.5 * right)
  },
  'standing-side-bend': ({ joints, wave }) => {
    joints.torso = rotation(0, 0, wave * 0.2)
    joints.chest = rotation(0, 0, wave * 0.18)
    joints.neck = rotation(0, 0, -wave * 0.12)
    joints.leftShoulder = rotation(0, 0, -2.9 * Math.max(0, -wave))
    joints.rightShoulder = rotation(0, 0, 2.9 * Math.max(0, wave))
    joints.leftElbow = rotation(0, 0, -0.08)
    joints.rightElbow = rotation(0, 0, 0.08)
  },
}

export function getStretchPose(id: StretchExerciseId, phase: number, reducedMotion = false): StretchPose {
  const safePhase = Number.isFinite(phase) ? phase : 0
  const wrappedPhase = ((safePhase % 1) + 1) % 1
  const sidePhase = (wrappedPhase * 2) % 1
  const ramp = Math.min(1, Math.max(0, Math.min(sidePhase / 0.3, (1 - sidePhase) / 0.3)))
  const ease = ramp * ramp * (3 - 2 * ramp)
  const animatedWave = id === 'shoulder-rolls'
    ? Math.sin(wrappedPhase * Math.PI * 2)
    : ease * (wrappedPhase < 0.5 ? 1 : -1)
  const wave = reducedMotion ? 1 : animatedWave
  const pulse = Math.max(0, wave)
  const context: PoseContext = {
    joints: neutralJoints(),
    wave,
    pulse,
    orbit: reducedMotion ? 0 : Math.cos(wrappedPhase * Math.PI * 2),
    headRetraction: 0,
    cameraYaw: id === 'chin-tuck' ? 0.95 : id === 'upper-back-rotation' ? -0.4 : id === 'chest-opener' ? 0.65 : 0.15,
    rootPosition: rotation(0, 0, 0),
  }
  poseFactories[id](context)
  return {
    joints: context.joints,
    rootPosition: context.rootPosition,
    cameraYaw: context.cameraYaw,
    headRetraction: context.headRetraction,
  }
}
