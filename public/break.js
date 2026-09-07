const { invoke } = window.__TAURI__.core
const { listen } = window.__TAURI__.event
const postpone = document.getElementById('postpone')
let pending = false

postpone.addEventListener('click', async () => {
  if (pending || postpone.disabled) return
  pending = true
  postpone.disabled = true
  postpone.textContent = '正在延迟…'
  try { await invoke('postpone_break') } finally { pending = false }
})

listen('repose-break-status', ({ payload }) => {
  const seconds = Math.max(0, Math.ceil(payload.remaining))
  const long = payload.phase === 'long'
  document.getElementById('countdown').textContent = `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
  document.getElementById('progress').style.width = `${Math.min(100, Math.max(0, (1 - seconds / Math.max(1, payload.duration)) * 100))}%`
  document.getElementById('title').textContent = long ? '起身走走，让身体舒展开。' : '让目光，离开屏幕一会。'
  document.getElementById('description').textContent = long ? '倒一杯水，做个伸展。屏幕以外，也有很好的风景。' : '看向远处，放松双肩。接下来的时间，留给自己。'
  const available = payload.canPostpone && !payload.postponing
  postpone.hidden = !(available || payload.postponing)
  postpone.disabled = !available
  postpone.textContent = payload.postponing ? '正在延迟…' : `延迟 ${payload.postponeSeconds / 60} 分钟 · 仅此一次`
  document.getElementById('postpone-note').textContent = available ? '稍后将完整休息，届时不能再次延迟。' : '本次休息结束前不能退出。'
  document.getElementById('kind').textContent = `强制${long ? '大' : '小'}休息 · ${available ? '可延迟一次' : '倒计时结束后恢复'}`
})

