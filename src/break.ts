import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { createStretchScene, type StretchScene } from './lib/stretchScene.ts'
import {
  STRETCH_EXERCISES,
  getStretchStep,
  moveStretchOffset,
} from './lib/stretchRoutine.ts'
import './break.css'
import './stretch-anatomy.css'
import { getShortBreakVoice } from './lib/reposeVoice.ts'

type BreakStatus = {
  phase: string
  remaining: number
  duration: number
  breakId: string
  canPostpone: boolean
  postponeSeconds: number
  postponing: boolean
}

const element = <T extends HTMLElement>(id: string) => document.getElementById(id) as T
const shortGuide = element<HTMLElement>('short-guide')
const shortTitle = element<HTMLElement>('short-title')
const shortDescription = element<HTMLElement>('short-description')
const longGuide = element<HTMLElement>('long-guide')
const countdown = element<HTMLElement>('countdown')
const totalLabel = element<HTMLElement>('total-label')
const progress = element<HTMLElement>('progress')
const hint = element<HTMLElement>('hint')
const postpone = element<HTMLButtonElement>('postpone')
const postponeNote = element<HTMLElement>('postpone-note')
const kind = element<HTMLElement>('kind')
const actionFocus = element<HTMLElement>('action-focus')
const actionIndex = element<HTMLElement>('action-index')
const actionTitle = element<HTMLElement>('action-title')
const actionCue = element<HTMLElement>('action-cue')
const actionSafety = element<HTMLElement>('action-safety')
const actionProgress = element<HTMLElement>('action-progress')
const actionTime = element<HTMLElement>('action-time')
const actionDots = element<HTMLElement>('action-dots')
const stretchStage = element<HTMLElement>('stretch-stage')
const stretchFallback = element<HTMLElement>('stretch-fallback')

let pending = false
let manualOffset = 0
let lastStatus: BreakStatus | null = null
let stretchScene: StretchScene | null = null
const preview = !isTauri() || new URLSearchParams(location.search).has('preview')
const motionPreference = window.matchMedia('(prefers-reduced-motion: reduce)')

for (const exercise of STRETCH_EXERCISES) {
  const dot = document.createElement('span')
  dot.dataset.exercise = exercise.id
  actionDots.append(dot)
}

function formatTime(value: number) {
  const seconds = Math.max(0, Math.ceil(value))
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}

function ensureStretchScene(initialId: (typeof STRETCH_EXERCISES)[number]['id']) {
  if (stretchScene || !stretchFallback.hidden) return
  try {
    stretchScene = createStretchScene(stretchStage, initialId)
    stretchScene.setReducedMotion(motionPreference.matches || (preview && new URLSearchParams(location.search).has('still')))
    const loadingScene = stretchScene
    void loadingScene.ready.catch(() => {
      if (stretchScene !== loadingScene) return
      stretchStage.hidden = true
      stretchFallback.hidden = false
    })
  } catch {
    stretchStage.hidden = true
    stretchFallback.hidden = false
  }
}

function renderStatus(status: BreakStatus) {
  if (lastStatus?.breakId !== status.breakId) manualOffset = 0
  lastStatus = status
  const seconds = Math.max(0, Math.ceil(status.remaining))
  const duration = Math.max(1, status.duration)
  const long = status.phase.toLowerCase().includes('long')
  document.body.dataset.breakType = long ? 'long' : 'short'
  countdown.textContent = formatTime(seconds)
  countdown.setAttribute('aria-label', `休息剩余 ${formatTime(seconds)}`)
  progress.style.width = `${Math.min(100, Math.max(0, (1 - seconds / duration) * 100))}%`
  totalLabel.textContent = long ? '大休息剩余' : '本次休息剩余'
  hint.textContent = long ? '跟着舒服的幅度慢慢活动，倒计时结束后屏幕自动恢复' : '休息结束后，屏幕会自动恢复'
  shortGuide.hidden = long
  longGuide.hidden = !long

  if (!long) {
    const voice = getShortBreakVoice(status.canPostpone ? 'enter' : 'return', status.breakId)
    shortTitle.textContent = voice.title
    shortDescription.textContent = voice.body
  }

  if (long) {
    const step = getStretchStep(seconds, duration, manualOffset)
    ensureStretchScene(step.exercise.id)
    stretchScene?.setExercise(step.exercise.id)
    actionFocus.textContent = step.exercise.focus
    actionIndex.textContent = `${step.index + 1} / ${STRETCH_EXERCISES.length}`
    actionTitle.textContent = step.exercise.title
    actionCue.textContent = step.exercise.cue
    actionSafety.textContent = step.exercise.safety
    actionProgress.style.width = `${step.progress * 100}%`
    actionProgress.parentElement?.setAttribute('aria-label', `当前动作已完成 ${Math.round(step.progress * 100)}%`)
    actionTime.textContent = `${Math.max(1, Math.ceil(step.stepRemaining))} 秒后换动作`
    Array.from(actionDots.children).forEach((dot, index) => dot.classList.toggle('active', index === step.index))
  } else if (stretchScene) {
    stretchScene.dispose()
    stretchScene = null
  }

  const available = status.canPostpone && !status.postponing
  postpone.hidden = !(available || status.postponing)
  postpone.disabled = !available
  postpone.textContent = status.postponing ? '正在延迟…' : `延迟 ${status.postponeSeconds / 60} 分钟 · 仅此一次`
  postponeNote.textContent = available ? '稍后将完整休息，届时不能再次延迟。' : '本次休息结束前不能退出。'
  kind.textContent = `强制${long ? '大' : '小'}休息 · ${available ? '可延迟一次' : '倒计时结束后恢复'}`
  if (preview) {
    postpone.hidden = true
    postponeNote.textContent = '动作预览 · 可随时关闭此窗口'
    kind.textContent = '拉伸动作预览'
    hint.textContent = '动作以舒适为准；如有疼痛或眩晕，请立即停止。'
  }
}

function move(direction: 'previous' | 'next') {
  manualOffset = moveStretchOffset(manualOffset, direction)
  if (lastStatus) renderStatus(lastStatus)
}

element<HTMLButtonElement>('previous-action').addEventListener('click', () => move('previous'))
element<HTMLButtonElement>('next-action').addEventListener('click', () => move('next'))
motionPreference.addEventListener('change', () => stretchScene?.setReducedMotion(motionPreference.matches))
window.addEventListener('beforeunload', () => stretchScene?.dispose())

postpone.addEventListener('click', async () => {
  if (pending || postpone.disabled || !isTauri()) return
  pending = true
  postpone.disabled = true
  postpone.textContent = '正在延迟…'
  try {
    const accepted = await invoke<boolean>('postpone_break')
    if (accepted && lastStatus && !lastStatus.phase.toLowerCase().includes('long')) {
      const voice = getShortBreakVoice('postpone', lastStatus.breakId)
      postponeNote.textContent = `${voice.title} ${voice.body}`
    }
    if (!accepted && lastStatus) renderStatus(lastStatus)
  } catch {
    if (lastStatus) renderStatus(lastStatus)
    postponeNote.textContent = '延迟未成功，请重试或继续休息。'
  } finally { pending = false }
})

if (!preview) {
  void listen<BreakStatus>('repose-break-status', event => renderStatus(event.payload))
} else {
  const previewLong = new URLSearchParams(location.search).get('preview') === 'long'
  const previewDuration = previewLong ? 300 : 20
  const requestedExercise = new URLSearchParams(location.search).get('exercise')
  const requestedIndex = STRETCH_EXERCISES.findIndex(exercise => exercise.id === requestedExercise)
  let previewRemaining = previewDuration
  const previewStatus = (): BreakStatus => ({
    phase: previewLong ? 'long' : 'short',
    remaining: previewRemaining,
    duration: previewDuration,
    breakId: 'browser-preview',
    canPostpone: !new URLSearchParams(location.search).has('returned'),
    postponeSeconds: previewLong ? 300 : 60,
    postponing: false,
  })
  renderStatus(previewStatus())
  if (requestedIndex >= 0) { manualOffset = requestedIndex; renderStatus(previewStatus()) }
  window.setInterval(() => {
    if (!new URLSearchParams(location.search).has('still')) previewRemaining = previewRemaining > 0 ? previewRemaining - 1 : previewDuration
    renderStatus(previewStatus())
  }, 1000)
}
