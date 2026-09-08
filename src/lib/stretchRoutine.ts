export const STRETCH_STEP_SECONDS = 30

export const STRETCH_EXERCISES = [
  {
    id: 'chin-tuck',
    title: '下巴微收',
    focus: '肩颈与上背',
    cue: '目光平视前方，下巴水平向后收，感受后颈慢慢变长。',
    safety: '不要低头或仰头，动作幅度小而轻柔。',
  },
  {
    id: 'neck-side-stretch',
    title: '颈部侧向拉伸',
    focus: '肩颈与上背',
    cue: '一侧耳朵缓缓靠近肩膀，另一侧肩膀自然下沉，左右交替。',
    safety: '只借助头部自身重量，不要用手强压。',
  },
  {
    id: 'shoulder-rolls',
    title: '肩部向后环绕',
    focus: '肩颈与上背',
    cue: '双肩轻轻提起，缓缓向后、向下，再回到起点，小幅画圈。',
    safety: '保持呼吸自然，肩部疼痛时缩小圆圈。',
  },
  {
    id: 'upper-trapezius',
    title: '上斜方肌拉伸',
    focus: '肩颈与上背',
    cue: '一侧手臂轻放到身后，头部向另一侧轻倾，回正后换边。',
    safety: '肩膀保持放松，不要拉扯颈部。',
  },
  {
    id: 'chest-opener',
    title: '胸肩打开',
    focus: '肩颈与上背',
    cue: '双臂轻轻向身后伸展，肩胛骨向中间靠近，胸口自然打开。',
    safety: '腰部不要过度后仰，以肩前侧舒展为准。',
  },
  {
    id: 'upper-back-rotation',
    title: '上背旋转',
    focus: '肩颈与上背',
    cue: '双臂自然下垂，骨盆保持朝前，胸口带动上半身小幅左右转动。',
    safety: '转动来自上背，不要憋气或猛然扭腰。',
  },
  {
    id: 'wrist-forearm',
    title: '手腕与前臂拉伸',
    focus: '手腕与前臂',
    cue: '一只手臂向前伸展，手腕缓缓向下弯曲，再放松，左右交替。',
    safety: '手肘保持微松，手腕出现刺痛时立即停止。',
  },
  {
    id: 'standing-side-bend',
    title: '站立侧弯',
    focus: '躯干与体侧',
    cue: '双脚站稳，一只手臂举过头顶，身体向对侧缓缓延伸。',
    safety: '身体保持在同一平面，不要向前塌腰。',
  },
] as const

export type StretchExercise = (typeof STRETCH_EXERCISES)[number]
export type StretchExerciseId = StretchExercise['id']

function modulo(value: number, length: number) {
  return ((value % length) + length) % length
}

export function moveStretchOffset(offset: number, direction: 'previous' | 'next') {
  const safeOffset = Number.isFinite(offset) ? Math.trunc(offset) : 0
  return modulo(safeOffset + (direction === 'previous' ? -1 : 1), STRETCH_EXERCISES.length)
}

export function getStretchStep(remaining: number, duration: number, offset = 0) {
  const safeDuration = Number.isFinite(duration) ? Math.max(0, duration) : 0
  const safeRemaining = Number.isFinite(remaining)
    ? Math.min(safeDuration, Math.max(0, remaining))
    : safeDuration
  const breakElapsed = safeDuration - safeRemaining
  const elapsedInStep = breakElapsed % STRETCH_STEP_SECONDS
  const automaticIndex = Math.floor(breakElapsed / STRETCH_STEP_SECONDS)
  const index = modulo(automaticIndex + (Number.isFinite(offset) ? Math.trunc(offset) : 0), STRETCH_EXERCISES.length)

  return {
    index,
    exercise: STRETCH_EXERCISES[index],
    progress: elapsedInStep / STRETCH_STEP_SECONDS,
    stepRemaining: STRETCH_STEP_SECONDS - elapsedInStep,
    breakElapsed,
  }
}
