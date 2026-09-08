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
export const STRETCH_JOINT_MAP: Record<JointName, string> = {
  root: 'root', torso: 'spine04', chest: 'spine01', neck: 'neck01', head: 'head',
  leftClavicle: 'clavicle.R', rightClavicle: 'clavicle.L',
  leftShoulder: 'upperarm01.R', rightShoulder: 'upperarm01.L',
  leftElbow: 'lowerarm01.R', rightElbow: 'lowerarm01.L',
  leftWrist: 'wrist.R', rightWrist: 'wrist.L',
  leftHip: 'upperleg01.R', rightHip: 'upperleg01.L',
  leftKnee: 'lowerleg01.R', rightKnee: 'lowerleg01.L',
}

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
    joints.neck = rotation(0, 0, wave * 0.1)
    joints.head = rotation(0, 0, wave * 0.26)
    joints.leftShoulder = rotation(0, 0, -Math.max(0, wave) * 0.08)
    joints.rightShoulder = rotation(0, 0, Math.min(0, wave) * 0.08)
  },
  'shoulder-rolls': ({ joints, wave, orbit }) => {
    // The girdle travels up → back → down → forward; the arms hang naturally.
    joints.leftClavicle = rotation(0, -wave * 0.18, -(orbit + 1) * 0.08)
    joints.rightClavicle = rotation(0, wave * 0.18, (orbit + 1) * 0.08)
    joints.leftShoulder = rotation(0, 0, -0.03)
    joints.rightShoulder = rotation(0, 0, 0.03)
  },
  'upper-trapezius': ({ joints, wave }) => {
    const left = Math.max(0, -wave)
    const right = Math.max(0, wave)
    joints.neck = rotation(0, 0, wave * 0.1)
    joints.head = rotation(0, 0, wave * 0.22)
    // Head tilts away from the arm behind the hip, on either side.
    joints.leftShoulder = rotation(0.48 * left, 0, -0.02)
    joints.rightShoulder = rotation(0.48 * right, 0, 0.02)
    joints.leftElbow = rotation(-0.1 * left)
    joints.rightElbow = rotation(-0.1 * right)
  },
  'chest-opener': ({ joints, pulse }) => {
    joints.leftClavicle = rotation(0, -pulse * 0.08)
    joints.rightClavicle = rotation(0, pulse * 0.08)
    joints.leftShoulder = rotation(pulse * 0.4, 0, -pulse * 0.06)
    joints.rightShoulder = rotation(pulse * 0.4, 0, pulse * 0.06)
    joints.leftElbow = rotation(-pulse * 0.15)
    joints.rightElbow = rotation(-pulse * 0.15)
  },
  'upper-back-rotation': ({ joints, wave }) => {
    // Rotate the upper spine; keep the pelvis still and avoid an extra arm pose.
    joints.chest = rotation(0, wave * 0.35)
    joints.leftShoulder = rotation(-0.05, 0, -0.03)
    joints.rightShoulder = rotation(-0.05, 0, 0.03)
    joints.leftElbow = rotation(-0.12)
    joints.rightElbow = rotation(-0.12)
  },
  'wrist-forearm': ({ joints, wave }) => {
    const left = Math.max(0, wave)
    const right = Math.max(0, -wave)
    joints.leftShoulder = rotation(-1.4 * left, 0, -0.08)
    joints.rightShoulder = rotation(-1.4 * right, 0, 0.08)
    joints.leftElbow = rotation(-0.08 * left)
    joints.rightElbow = rotation(-0.08 * right)
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
