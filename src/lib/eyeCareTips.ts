// Sources: NIH News in Health, “Tired, Achy Eyes?” (September 2024),
// https://newsinhealth.nih.gov/2024/09/tired-achy-eyes
// National Eye Institute, “Keep Your Eyes Healthy”,
// https://www.nei.nih.gov/eye-health-information/healthy-vision/how-eyes-work/keep-your-eyes-healthy
export const EYE_CARE_TIPS = [
  { title: '看远处，让对焦也歇一会', body: '持续看近处会让眼睛的调节持续工作。把目光移向远处，让近距离用眼得到休息。' },
  { title: '记住 20–20–20', body: '每近距离用眼约 20 分钟，看向约 6 米外的物体至少 20 秒。现在就找一个远处的目标吧。' },
  { title: '专注时，也别忘了眨眼', body: '盯屏幕时，我们常常眨眼变少，眼睛容易干涩。轻轻眨几次眼，再把视线移向远处。' },
  { title: '休息时，让手机也等一等', body: '从电脑换到手机，仍然是在近距离用眼。放下屏幕，让目光在远处自然停留。' },
  { title: '别让风一直吹向眼睛', body: '风扇或空调直吹面部，可能加重眼睛干涩。调整一下风向，给双眼更舒适的环境。' },
  { title: '眼疲劳和老花，不是一回事', body: '老花主要与年龄增长有关，通常在 40 多岁逐渐出现。看远处可以休息，但不能逆转老花。' },
] as const

// Keep one tip throughout a break, including pause/resume and postponement.
export function getEyeCareTip(breakId: string) {
  let hash = 2166136261
  for (const character of breakId || 'repose') {
    hash = Math.imul(hash ^ character.charCodeAt(0), 16777619)
  }
  return EYE_CARE_TIPS[(hash >>> 0) % EYE_CARE_TIPS.length]
}
