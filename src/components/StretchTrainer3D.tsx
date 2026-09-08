import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { ChevronLeft, ChevronRight, Pause, Sparkles } from 'lucide-react'
import type { StretchScene } from '../lib/stretchScene.ts'
import {
  STRETCH_EXERCISES,
  getStretchStep,
  moveStretchOffset,
} from '../lib/stretchRoutine.ts'

type StretchTrainer3DProps = {
  remaining: number
  duration: number
  running: boolean
  children?: ReactNode
}

export function StretchTrainer3D({ remaining, duration, running, children }: StretchTrainer3DProps) {
  const [manualOffset, setManualOffset] = useState(0)
  const [sceneUnavailable, setSceneUnavailable] = useState(false)
  const stageRef = useRef<HTMLDivElement>(null)
  const sceneRef = useRef<StretchScene | null>(null)
  const step = useMemo(
    () => getStretchStep(remaining, duration, manualOffset),
    [remaining, duration, manualOffset],
  )
  const stepIdRef = useRef(step.exercise.id)
  stepIdRef.current = step.exercise.id
  const runningRef = useRef(running)
  runningRef.current = running

  useEffect(() => {
    const stage = stageRef.current
    if (!stage) return
    const motionPreference = window.matchMedia('(prefers-reduced-motion: reduce)')
    const syncMotionPreference = () => sceneRef.current?.setReducedMotion(motionPreference.matches)
    let cancelled = false
    void import('../lib/stretchScene.ts').then(({ createStretchScene }) => {
      if (cancelled) return
      try {
        const scene = createStretchScene(stage, stepIdRef.current)
        sceneRef.current = scene
        scene.setReducedMotion(motionPreference.matches)
        scene.setRunning(runningRef.current)
        setSceneUnavailable(false)
        void scene.ready.catch(() => { if (!cancelled) setSceneUnavailable(true) })
      } catch {
        setSceneUnavailable(true)
      }
    }).catch(() => { if (!cancelled) setSceneUnavailable(true) })
    motionPreference.addEventListener('change', syncMotionPreference)
    return () => {
      cancelled = true
      motionPreference.removeEventListener('change', syncMotionPreference)
      sceneRef.current?.dispose()
      sceneRef.current = null
    }
  }, [])

  useEffect(() => {
    sceneRef.current?.setExercise(step.exercise.id)
  }, [step.exercise.id])

  useEffect(() => { sceneRef.current?.setRunning(running) }, [running])

  const move = (direction: 'previous' | 'next') => {
    setManualOffset(current => moveStretchOffset(current, direction))
  }

  return <section className={`stretch-trainer ${running ? '' : 'is-paused'}`} aria-label="大休息拉伸训练">
    <div className={`stretch-visual ${sceneUnavailable ? 'scene-unavailable' : ''}`}>
      <div ref={stageRef} className="stretch-stage" aria-hidden="true" />
      {sceneUnavailable && <div className="stretch-fallback-message">动画暂不可用<br /><small>请参考右侧动作说明</small></div>}
      <span className="stretch-3d-badge"><Sparkles size={12} />3D 动作示范</span>
      <span className="stretch-region-legend"><i />拉伸区域示意 · 非精确肌肉解剖</span>
    </div>
    <div className="stretch-sidebar">
    <div className="stretch-guide">
      <div className="stretch-guide-topline"><span>{step.exercise.focus}</span><span>{step.index + 1} / {STRETCH_EXERCISES.length}</span></div>
      <h2 aria-live="polite">{step.exercise.title}</h2>
      <p className="stretch-cue">{step.exercise.cue}</p>
      <p className="stretch-safety">{step.exercise.safety}</p>
      <div className="stretch-step-progress" aria-label={`当前动作已完成 ${Math.round(step.progress * 100)}%`}><span style={{ width: `${step.progress * 100}%` }} /></div>
      <div className="stretch-step-meta">
        <span>{running ? `${Math.max(1, Math.ceil(step.stepRemaining))} 秒后换动作` : <><Pause size={11} />动作轮播已暂停</>}</span>
        <span>自然呼吸 · 轻柔舒展</span>
      </div>
      <div className="stretch-navigation">
        <button type="button" onClick={() => move('previous')} aria-label="上一个拉伸动作"><ChevronLeft size={18} /></button>
        <div className="stretch-dots" aria-label={`当前是第 ${step.index + 1} 个动作`}>{STRETCH_EXERCISES.map((exercise, index) => <span key={exercise.id} className={index === step.index ? 'active' : ''} />)}</div>
        <button type="button" onClick={() => move('next')} aria-label="下一个拉伸动作"><ChevronRight size={18} /></button>
      </div>
    </div>
    {children}
    </div>
  </section>
}
