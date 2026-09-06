const postponeButton = document.getElementById('postpone')
const postponeNote = document.getElementById('postpone-note')
let requestingPostpone = false
let failedPostpone = false

postponeButton.addEventListener('click', async () => {
  if (postponeButton.disabled || requestingPostpone) return
  requestingPostpone = true
  failedPostpone = false
  postponeButton.disabled = true
  postponeButton.textContent = '正在延迟…'
  try {
    failedPostpone = !(await window.reposeBreak.postpone())
  } catch {
    failedPostpone = true
  } finally {
    requestingPostpone = false
  }
})

window.reposeBreak.onStatus(({ phase, remaining, duration, canPostpone, postponeSeconds, postponing }) => {
  const seconds = Math.max(0, Math.ceil(remaining))
  document.getElementById('countdown').textContent = `${Math.floor(seconds / 60).toString().padStart(2, '0')}:${(seconds % 60).toString().padStart(2, '0')}`
  document.getElementById('progress').style.width = `${Math.min(100, Math.max(0, (1 - seconds / Math.max(1, duration)) * 100))}%`
  const isLong = phase.toLowerCase().includes('long')
  document.getElementById('title').textContent = isLong ? '起身走走，让身体舒展开。' : '让目光，离开屏幕一会。'
  document.getElementById('description').textContent = isLong ? '倒一杯水，做个伸展。屏幕以外，也有很好的风景。' : '看向远处，放松双肩。接下来的时间，留给自己。'
  document.getElementById('kind').textContent = `强制${isLong ? '长' : '短'}休息 · ${canPostpone ? '可延迟一次' : postponing ? '正在延迟' : '倒计时结束后恢复'}`
  const wasHidden = postponeButton.hidden
  const pending = postponing || requestingPostpone
  postponeButton.hidden = !(canPostpone || pending)
  postponeButton.disabled = !canPostpone || pending
  postponeButton.textContent = pending ? '正在延迟…' : `延迟 ${postponeSeconds / 60} 分钟 · 仅此一次`
  postponeNote.textContent = pending ? '正在恢复工作，稍后会重新开始本次休息。' : failedPostpone && canPostpone ? '暂未能延迟，请重试。' : canPostpone ? '稍后将完整休息，届时不能再次延迟。' : '本次已延迟过，请完成休息。'
  if (wasHidden && !postponeButton.hidden && !postponeButton.disabled) postponeButton.focus({ preventScroll: true })
})
