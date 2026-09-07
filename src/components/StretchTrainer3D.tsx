import { useEffect, useMemo, useRef, useState } from 'react'
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
}

export function StretchTrainer3D({ remaining, duration, running }: StretchTrainer3DProps) {
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
        setSceneUnavailable(false)
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

  const move = (direction: 'previous' | 'next') => {
    setManualOffset(current => moveStretchOffset(current, direction))
  }

  return <section className={`stretch-trainer ${running ? '' : 'is-paused'}`} aria-label="大休息拉伸训练">
    <div className={`stretch-visual ${sceneUnavailable ? 'scene-unavailable' : ''}`}>
      <div className="stretch-stage-glow" aria-hidden="true" />
      <div ref={stageRef} className="stretch-stage" aria-hidden="true" />
      {sceneUnavailable && <div className="stretch-fallback-person" aria-hidden="true"><i className="head" /><i className="body" /><i className="arm left" /><i className="arm right" /><i className="leg left" /><i className="leg right" /></div>}
      <span className="stretch-3d-badge"><Sparkles size={12} />3D 动作示范</span>
    </div>
    <div className="stretch-guide" aria-live="polite">
      <div className="stretch-guide-topline"><span>{step.exercise.focus}</span><span>{step.index + 1} / {STRETCH_EXERCISES.length}</span></div>
      <h2>{step.exercise.title}</h2>
      <p className="stretch-cue">{step.exercise.cue}</p>
      <p className="stretch-safety">{step.exercise.safety}</p>
      <div className="stretch-step-progress" aria-label={`当前动作已完成 ${Math.round(step.progress * 100)}%`}><span style={{ width: `${step.progress * 100}%` }} /></div>
      <div className="stretch-step-meta">
        <span>{running ? `${Math.max(1, Math.ceil(step.stepRemaining))} 秒后换动作` : <><Pause size={11} />动作轮播已暂停</>}</span>
        <span>缓慢呼吸 · 左右交替</span>
      </div>
      <div className="stretch-navigation">
        <button type="button" onClick={() => move('previous')} aria-label="上一个拉伸动作"><ChevronLeft size={18} /></button>
        <div className="stretch-dots" aria-label={`当前是第 ${step.index + 1} 个动作`}>{STRETCH_EXERCISES.map((exercise, index) => <span key={exercise.id} className={index === step.index ? 'active' : ''} />)}</div>
        <button type="button" onClick={() => move('next')} aria-label="下一个拉伸动作"><ChevronRight size={18} /></button>
      </div>
    </div>
  </section>
}
