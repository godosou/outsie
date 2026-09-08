import type { StretchExerciseId } from './stretchRoutine.ts'

export function getStretchFraming(id: StretchExerciseId, aspect: number) {
  const safeAspect = Number.isFinite(aspect) && aspect > 0 ? aspect : 1
  const fullBody = id === 'standing-side-bend'
  const wrist = id === 'wrist-forearm'
  const back = id === 'upper-back-rotation' || id === 'upper-trapezius' || id === 'shoulder-rolls'
  return {
    yaw: back ? 2.8 : id === 'chin-tuck' ? 1.2 : wrist ? 0.9 : id === 'chest-opener' ? 0.65 : 0.12,
    // Forward-reaching hands need more horizontal room than a resting torso.
    viewHeight: Math.max(fullBody ? 6.1 : wrist ? 3.5 : 2.9, (fullBody ? 5.2 : wrist ? 4.8 : 2.8) / safeAspect),
    targetY: fullBody ? 2.7 : 3.05,
  }
}
