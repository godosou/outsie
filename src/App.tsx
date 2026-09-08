import { useEffect, useRef, useState, type ReactNode } from 'react'
import { UnlockSettingsPanel } from './components/UnlockSettingsPanel'
import { WorkConsolePanel } from './components/WorkConsolePanel'
import './components/WorkConsolePanel.css'
import { Activity, ArrowDownToLine, ArrowRight, ArrowUpRight, Bell, BookOpen, CalendarDays, Check, CheckCircle2, ChevronLeft, ChevronRight, Clock3, Coffee, Droplets, Eye, Flower2, Heart, KeyRound, LayoutDashboard, Leaf, LockKeyhole, Menu, Monitor, Moon, ShieldCheck, Pause, Play, RotateCcw, Settings2, SlidersHorizontal, Sparkles, Sprout, Sun, Volume2, Wind, X, BarChart3 } from 'lucide-react'
import { useBreakTimer } from './hooks/useBreakTimer'
import { StretchTrainer3D } from './components/StretchTrainer3D'
import { buildHourlyChart, selectDefaultHour } from './lib/activityChart'
import { localDateKey } from './lib/timer'
import { getShortBreakVoice } from './lib/reposeVoice'

type Page = 'overview' | 'schedule' | 'ideas' | 'activity' | 'phoneKey' | 'workConsole' | 'settings'
type Theme = 'light' | 'dark' | 'system'
type Exercise = { id: string; category: string; title: string; subtitle: string; duration: string; type: 'short' | 'long'; art: string; color: string; icon: typeof Eye; steps: string[] }
type DesktopPreferences = { strictBreaks: boolean; idleLockEnabled: boolean; idleLockSeconds: 30 }

const APP_VERSION = '0.6.2'

const exercises: Exercise[] = [
  { id: 'eyes', category: '放松双眼', title: '目光，去远方散个步', subtitle: '暂时离开屏幕，看看窗外的风景。', duration: '短休息', type: 'short', art: 'eyes', color: 'sage', icon: Eye, steps: ['轻轻闭上眼睛，让眼周放松。', '望向窗外或房间远处，让目光自然停留。', '慢慢眨几次眼，感受眼睛重新湿润。'] },
  { id: 'stretch', category: '舒展身体', title: '把紧绷，轻轻放下', subtitle: '起身动一动，给肩颈一点空间。', duration: '长休息', type: 'long', art: 'stretch', color: 'peach', icon: Activity, steps: ['缓缓起身，双脚自然分开站稳。', '肩膀慢慢向后转圈，双臂轻柔向上伸展。', '按自己的节奏走动一下，以舒适为准。'] },
  { id: 'water', category: '补充水分', title: '喝口水，重新出发', subtitle: '一杯温水，也是照顾自己的小事。', duration: '短休息', type: 'short', art: 'water', color: 'blue', icon: Droplets, steps: ['离开一下座位，给自己倒杯水。', '小口慢饮，不必急着回到工作里。', '感受片刻停顿，然后再轻轻出发。'] },
]
const navigation: { id: Page; label: string; icon: typeof Eye }[] = [
  { id: 'overview', label: '今日概览', icon: LayoutDashboard },
  { id: 'schedule', label: '休息计划', icon: SlidersHorizontal },
  { id: 'ideas', label: '休息灵感', icon: Flower2 },
  { id: 'activity', label: '我的记录', icon: BarChart3 },
  { id: 'phoneKey', label: '手机钥匙', icon: KeyRound },
  { id: 'workConsole', label: 'App 工作台', icon: Monitor },
]
const titles: Record<Page, { title: string; subtitle: string; eyebrow: string }> = {
  overview: { title: '让休息，自然发生。', subtitle: '专注于热爱的事，也留一点时间，好好照顾自己。', eyebrow: 'A LITTLE PAUSE, A BETTER DAY' },
  schedule: { title: '找到自己的节奏。', subtitle: '没有唯一正确的频率，舒服的节奏就是好节奏。', eyebrow: 'MAKE ROOM FOR YOURSELF' },
  ideas: { title: '小小休息，大有不同。', subtitle: '离开屏幕的这一刻，可以用来做很多美好的小事。', eyebrow: 'SMALL MOMENTS, BIG DIFFERENCE' },
  activity: { title: '每一次停顿，都算数。', subtitle: '慢慢积累的好习惯，正在成为生活的一部分。', eyebrow: 'A KINDER WAY TO KEEP GOING' },
  phoneKey: { title: '手机钥匙与 BLE 调试。', subtitle: '在这里生成配对二维码，联调手机连接，并校准适合你的靠近距离。', eyebrow: 'PAIR, CONNECT, CALIBRATE' },
  workConsole: { title: '常用操作，触手可及。', subtitle: '为每个 App 配置快捷键与键盘序列，手机点一下就能执行。', eyebrow: 'YOUR APPS, ONE TAP AWAY' },
  settings: { title: '让 Repose 更懂你。', subtitle: '把提醒调成你喜欢的样子，让它安静地融入日常。', eyebrow: 'A SPACE THAT FEELS LIKE YOU' },
}

function time(value: number) {
  const seconds = Math.max(0, Math.ceil(value))
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}
function minuteLabel(seconds: number) { return seconds < 60 ? '0' : String(Math.floor(seconds / 60)) }
function activityDurationLabel(seconds: number) {
  if (seconds <= 0) return '0 分钟'
  if (seconds < 60) return '<1 分钟'
  const minutes = seconds / 60
  return `${minutes < 10 ? Math.round(minutes * 10) / 10 : Math.round(minutes)} 分钟`
}
function localNoon(timestamp = Date.now()) {
  const date = new Date(timestamp)
  date.setHours(12, 0, 0, 0)
  return date.getTime()
}
function shiftLocalDay(timestamp: number, days: number) {
  const date = new Date(timestamp)
  date.setDate(date.getDate() + days)
  return localNoon(date.getTime())
}
function clockAfter(seconds: number) { return new Date(Date.now() + seconds * 1000).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false }) }
function BrandMark({ small = false }: { small?: boolean }) {
  return <span className={`brand-mark ${small ? 'small' : ''}`} aria-hidden="true"><img src="./favicon.svg" alt="" /></span>
}
function Toggle({ enabled, onChange, label }: { enabled: boolean; onChange: () => void; label: string }) {
  return <button className={`toggle ${enabled ? 'on' : ''}`} type="button" role="switch" aria-checked={enabled} aria-label={label} onClick={onChange}><span /></button>
}
function Modal({ children, onClose, className = '', label }: { children: ReactNode; onClose: () => void; className?: string; label: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const closeRef = useRef(onClose)
  closeRef.current = onClose
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null
    const first = ref.current?.querySelector<HTMLElement>('button, [href], input, select, [tabindex="0"]')
    first?.focus()
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeRef.current()
      if (event.key !== 'Tab') return
      const items = ref.current?.querySelectorAll<HTMLElement>('button:not([disabled]), [href], input, select, [tabindex="0"]')
      if (!items?.length) return
      if (event.shiftKey && document.activeElement === items[0]) { event.preventDefault(); items[items.length - 1].focus() }
      else if (!event.shiftKey && document.activeElement === items[items.length - 1]) { event.preventDefault(); items[0].focus() }
    }
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    document.addEventListener('keydown', handler)
    return () => { document.body.style.overflow = previousOverflow; document.removeEventListener('keydown', handler); previous?.focus() }
  }, [])
  return <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}><div className={`modal ${className}`} role="dialog" aria-modal="true" aria-label={label} ref={ref}>{children}</div></div>
}
function ExerciseCard({ exercise, onClick }: { exercise: Exercise; onClick: () => void }) {
  const Icon = exercise.icon
  return <button className={`exercise-card ${exercise.color}`} onClick={onClick}>
    <div className="exercise-topline"><span><Icon size={14} />{exercise.category}</span><ArrowUpRight size={17} /></div>
    <div className="exercise-art"><img src={`./illustrations/${exercise.art}.svg`} alt="" /></div>
    <div className="exercise-caption"><h3>{exercise.title}</h3><span>{exercise.duration}<span className="small-dot">·</span>随时开始</span></div>
  </button>
}

export default function App() {
  const timer = useBreakTimer()
  const { phase, running, remaining, settings, stats, completedCycles, breakId, canPostpone, postponedBreak, postponeSeconds } = timer
  const [page, setPage] = useState<Page>('overview')
  const [mobileMenu, setMobileMenu] = useState(false)
  const [help, setHelp] = useState(false)
  const [breathing, setBreathing] = useState(false)
  const [breathSeconds, setBreathSeconds] = useState(0)
  const [exercise, setExercise] = useState<Exercise | null>(null)
  const [toast, setToast] = useState('')
  const [postponePending, setPostponePending] = useState(false)
  const [ideaFilter, setIdeaFilter] = useState('全部灵感')
  const [activityDate, setActivityDate] = useState(() => localNoon())
  const [selectedHour, setSelectedHour] = useState(() => new Date().getHours())
  const [theme, setTheme] = useState<Theme>(() => { try { return (localStorage.getItem('repose-theme') as Theme) || 'light' } catch { return 'light' } })
  const [draft, setDraft] = useState(settings)
  const [desktopPreferences, setDesktopPreferences] = useState<DesktopPreferences>(() => {
    const defaults: DesktopPreferences = { strictBreaks: true, idleLockEnabled: Boolean(window.repose), idleLockSeconds: 30 }
    try { const saved = JSON.parse(localStorage.getItem('repose-desktop-preferences') || 'null'); return saved && typeof saved === 'object' ? { strictBreaks: typeof saved.strictBreaks === 'boolean' ? saved.strictBreaks : true, idleLockEnabled: typeof saved.idleLockEnabled === 'boolean' ? saved.idleLockEnabled : defaults.idleLockEnabled, idleLockSeconds: 30 } : defaults } catch { return defaults }
  })
  const [securityError, setSecurityError] = useState(() => { try { return localStorage.getItem('repose-security-error') === 'true' } catch { return false } })
  const strictBreak = Boolean(window.repose) && desktopPreferences.strictBreaks

  const audio = useRef<AudioContext | null>(null)
  const previousPhase = useRef(phase)
  const previousBreakId = useRef(breakId)
  const today = new Date()
  const inBreak = phase !== 'focus'
  const voiceKey = breakId ?? previousBreakId.current ?? 'short-break'
  if (breakId) previousBreakId.current = breakId
  const shortVoice = getShortBreakVoice(canPostpone ? 'enter' : 'return', voiceKey)
  const showToast = (message: string) => setToast(message)
  const initAudio = () => {
    try { const Audio = window.AudioContext || window.webkitAudioContext; if (Audio && !audio.current) audio.current = new Audio(); void audio.current?.resume() } catch { /* Sound is optional. */ }
  }
  const chime = () => {
    if (!audio.current) return
    try {
      const context = audio.current
      for (const [index, frequency] of [523.25, 659.25, 783.99].entries()) {
        const oscillator = context.createOscillator(); const gain = context.createGain(); const start = context.currentTime + index * 0.12
        oscillator.type = 'sine'; oscillator.frequency.value = frequency
        gain.gain.setValueAtTime(0, start); gain.gain.linearRampToValueAtTime(0.07, start + 0.03); gain.gain.exponentialRampToValueAtTime(0.001, start + 1.2)
        oscillator.connect(gain); gain.connect(context.destination); oscillator.start(start); oscillator.stop(start + 1.3)
      }
    } catch { /* The timer continues if audio is unavailable. */ }
  }
  const beginBreak = (type: 'short' | 'long') => { initAudio(); setExercise(null); timer.startBreak(type) }
  const postponeCurrentBreak = async () => {
    if (!canPostpone || postponePending) return
    setPostponePending(true)
    try {
      if (strictBreak && window.repose) {
        const accepted = await window.repose.postponeBreak()
        if (!accepted) showToast('本次休息暂时无法延迟，请继续休息')
      } else timer.postponeBreak()
    } catch { showToast('延迟请求未成功，请继续休息') }
    finally { setPostponePending(false) }
  }
  const navigate = (next: Page) => { setPage(next); setMobileMenu(false); window.scrollTo({ top: 0, behavior: 'smooth' }) }

  useEffect(() => { if (!toast) return; const id = window.setTimeout(() => setToast(''), 3500); return () => clearTimeout(id) }, [toast])
  useEffect(() => { setDraft(settings) }, [settings])
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => { document.documentElement.dataset.theme = theme === 'system' ? (media.matches ? 'dark' : 'light') : theme }
    apply(); try { localStorage.setItem('repose-theme', theme) } catch { /* Optional persistence. */ }
    media.addEventListener('change', apply); return () => media.removeEventListener('change', apply)
  }, [theme])
  useEffect(() => { try { localStorage.setItem('repose-security-error', String(securityError)) } catch { /* Optional persistence. */ } }, [securityError])
  useEffect(() => {
    try { localStorage.setItem('repose-desktop-preferences', JSON.stringify(desktopPreferences)) } catch { /* Optional persistence. */ }
    window.repose?.setPreferences(desktopPreferences)
  }, [desktopPreferences])
  useEffect(() => {
    document.title = `${time(remaining)} · ${inBreak ? '好好休息' : running ? '专注中' : '已暂停'} — Repose`
    window.repose?.setStatus({ running, phase, remaining, breakId, canPostpone, postponeSeconds })
  }, [phase, running, remaining, inBreak, breakId, canPostpone, postponeSeconds])
  useEffect(() => window.repose?.onCommand(({ command, breakId: completedBreakId }) => {
    if (command === 'toggle-pause' && !(strictBreak && inBreak)) timer.toggleRunning()
    if (command === 'start-short-break') timer.startBreak('short')
    if (command === 'start-long-break') timer.startBreak('long')
    if (command === 'strict-break-finished' && completedBreakId) timer.completeBreak(completedBreakId)
    if (command === 'postpone-break') timer.postponeBreak()
    if (command === 'idle-lock-failed') {
      setDesktopPreferences(previous => ({ ...previous, idleLockEnabled: false }))
      setSecurityError(true)
      showToast('安全锁屏未生效：请在系统设置中授予 Repose 辅助功能权限，再重新开启')
    }
  }), [strictBreak, inBreak, timer.toggleRunning, timer.startBreak, timer.completeBreak, timer.postponeBreak])
  useEffect(() => {
    if (previousPhase.current === phase) return
    if (phase !== 'focus') {
      setBreathing(false); setExercise(null); setHelp(false)
      if (settings.sound) chime(); window.repose?.showBreak()
      if (settings.notifications) {
        const shortNotification = getShortBreakVoice('notification', voiceKey)
        const notification = phase === 'long'
          ? { title: 'Repose · 歇一会', body: '辛苦了，起身走走，给自己一个长休息。' }
          : shortNotification
        if (window.repose) window.repose.notify(notification)
        else if ('Notification' in window && Notification.permission === 'granted') { try { new Notification(notification.title, { body: notification.body, icon: './favicon.svg' }) } catch { /* In-app reminders remain available. */ } }
      }
    } else if (postponedBreak) {
      if (postponedBreak === 'short') {
        const voice = getShortBreakVoice('postpone', voiceKey)
        showToast(`${voice.title} ${voice.body}`)
      } else showToast(`大休息已延迟 ${postponeSeconds / 60} 分钟，到时将重新开始完整休息`)
    } else {
      if (previousPhase.current === 'short') {
        const voice = getShortBreakVoice('complete', voiceKey)
        showToast(`${voice.title} ${voice.body}`)
      }
    }
    previousPhase.current = phase
  }, [phase])
  useEffect(() => {
    if (!breathing) { setBreathSeconds(0); return }
    const started = Date.now(); const id = setInterval(() => setBreathSeconds(Math.floor((Date.now() - started) / 1000)), 200)
    return () => clearInterval(id)
  }, [breathing])
  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      if (event.code !== 'Space' || event.repeat || help || exercise || breathing || inBreak || postponedBreak || ['INPUT', 'TEXTAREA', 'SELECT', 'BUTTON', 'A'].includes((event.target as HTMLElement).tagName)) return
      event.preventDefault(); initAudio(); timer.toggleRunning()
    }
    window.addEventListener('keydown', handle); return () => window.removeEventListener('keydown', handle)
  })

  const toggleNotifications = async () => {
    if (settings.notifications) { timer.updateSettings({ notifications: false }); return }
    if (window.repose) { timer.updateSettings({ notifications: true }); showToast('桌面通知已开启'); return }
    if (!('Notification' in window)) { showToast('当前浏览器不支持通知，应用内仍会提醒你'); return }
    try {
      const permission = await Notification.requestPermission()
      timer.updateSettings({ notifications: permission === 'granted' })
      showToast(permission === 'granted' ? '通知已开启，休息时会轻轻提醒你' : '未获得通知权限，应用内仍会提醒你')
    } catch { showToast('通知暂不可用，应用内仍会提醒你') }
  }
  const exportHistory = () => {
    const rows = ['日期,专注分钟,休息分钟,完成休息,跳过休息', ...timer.weeklyStats.map(day => `${day.date},${Math.floor(day.focusSeconds / 60)},${Math.floor(day.breakSeconds / 60)},${day.completedBreaks},${day.skippedBreaks}`)]
    const blob = new Blob(['\uFEFF' + rows.join('\n')], { type: 'text/csv;charset=utf-8;' })
    const url = URL.createObjectURL(blob); const anchor = document.createElement('a'); anchor.href = url; anchor.download = `repose-${today.toLocaleDateString('sv-SE')}.csv`; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000)
    showToast('最近 7 天的记录已导出')
  }
  const totalUpcoming = postponedBreak === 'long' ? remaining : remaining + Math.max(0, settings.longEvery - completedCycles) * (settings.shortInterval * 60 + settings.shortDuration)
  const cycle = breathSeconds % 14
  const breathLabel = cycle < 4 ? '慢慢吸气' : cycle < 8 ? '轻轻停留' : '缓缓呼气'
  const breathCountdown = cycle < 4 ? 4 - cycle : cycle < 8 ? 8 - cycle : 14 - cycle
  const todayNoon = localNoon(today.getTime())
  const oldestActivityDate = shiftLocalDay(todayNoon, -34)
  const activityIsToday = localDateKey(activityDate) === localDateKey(todayNoon)
  const activityStats = timer.getStatsForDate(activityDate)
  const activityHistory = timer.getHistoryForDate(activityDate)
  const activityHourly = timer.getHourlyStatsForDate(activityDate)
  const activityPoints = buildHourlyChart(activityHourly)
  const selectedActivityHour = activityPoints[selectedHour] ?? activityPoints[0]
  const activityPeak = Math.max(0, ...activityPoints.map(point => point.totalSeconds))
  const hasHourlyActivity = activityPeak > 0
  const activityDateLabel = activityIsToday
    ? '今天'
    : new Date(activityDate).toLocaleDateString('zh-CN', { month: 'long', day: 'numeric', weekday: 'short' })
  const chooseActivityDate = (timestamp: number) => {
    const target = Math.max(oldestActivityDate, Math.min(todayNoon, localNoon(timestamp)))
    if (target === activityDate) return
    const hourly = timer.getHourlyStatsForDate(target)
    setActivityDate(target)
    setSelectedHour(selectDefaultHour(hourly, target === todayNoon, today.getHours()))
  }

  return <div className="app-shell" onPointerDown={initAudio}>
    {mobileMenu && <button className="sidebar-scrim" aria-label="关闭导航" onClick={() => setMobileMenu(false)} />}
    <aside className={`sidebar ${mobileMenu ? 'mobile-open' : ''}`}>
      <button className="brand" onClick={() => navigate('overview')} aria-label="Repose 首页"><BrandMark /><span>repose<span className="brand-period">.</span></span></button>
      <p className="brand-tagline">给日常，留一点空白</p>
      <div className="nav-label">你的日常空间</div>
      <nav aria-label="主导航">{navigation.map(item => <button className={`nav-item ${page === item.id ? 'active' : ''}`} key={item.id} onClick={() => navigate(item.id)} aria-current={page === item.id ? 'page' : undefined}><item.icon size={19} strokeWidth={1.65} /><span>{item.label}</span>{page === item.id && <span className="nav-active-dot" />}</button>)}</nav>
      <div className="sidebar-bottom">
        <div className="sidebar-note"><Sprout size={29} strokeWidth={1.3} /><p>你不必时刻满格，<br />休息也是前进的一部分。</p><span>TAKE IT SLOW.</span></div>
        <button className={`nav-item ${page === 'settings' ? 'active' : ''}`} onClick={() => navigate('settings')}><Settings2 size={19} strokeWidth={1.65} /><span>偏好设置</span></button>
        <button className="nav-item help-nav" onClick={() => setHelp(true)}><BookOpen size={18} strokeWidth={1.65} /><span>认识 Repose</span><ArrowUpRight size={14} /></button>
        <div className="sidebar-status"><span className={`status-dot ${!running ? 'paused' : ''}`} /><span>{running ? '正在温柔守护你的节奏' : '暂停一下，随时再出发'}</span></div>
      </div>
    </aside>

    <main className="main-content">
      <div className="topbar"><div className="topbar-left"><button className="icon-button mobile-toggle" aria-label="打开导航" onClick={() => setMobileMenu(true)}><Menu size={20} /></button><span className="breadcrumb">我的空间</span><ChevronRight size={13} /><span>{page === 'settings' ? '偏好设置' : navigation.find(item => item.id === page)?.label}</span></div><div className="topbar-right"><span className="date-text"><CalendarDays size={14} />{today.toLocaleDateString('zh-CN', { month: 'long', day: 'numeric', weekday: 'long' })}</span><span className="topbar-separator" /><span className="welcome-mark"><Sun size={17} /></span></div></div>
      <header className="page-heading"><div><div className="eyebrow">{titles[page].eyebrow}</div><h1>{titles[page].title}</h1><p>{titles[page].subtitle}</p></div><button className={`reminder-status ${running ? '' : 'is-paused'}`} disabled={Boolean(postponedBreak)} onClick={() => { initAudio(); timer.toggleRunning() }}><span className={`status-dot ${running ? '' : 'paused'}`} />{postponedBreak ? '已延迟一次 · 即将休息' : running ? '休息提醒已开启' : '休息提醒已暂停'}<ChevronRight size={14} /></button></header>

      {window.repose && !desktopPreferences.idleLockEnabled && <div className="security-alert" role="alert"><ShieldCheck size={19} /><div><strong>{securityError ? '安全锁屏需要系统授权' : '安全锁屏尚未开启'}</strong><p>{securityError ? '当前自动锁屏未生效。请在系统设置 → 隐私与安全性 → 辅助功能中允许 Repose，然后重新开启 30 秒安全锁屏。' : '目前离开电脑后不会自动锁屏。请在偏好设置中开启 30 秒无操作安全锁屏。'}</p></div><button className="text-button" onClick={() => navigate('settings')}>前往设置<ArrowRight size={15} /></button></div>}
      {page === 'overview' && <div className="page-enter">
        <div className="hero-grid">
          <section className="timer-card" aria-label="休息计时器">
            <div className="timer-grain" />
            <div className="timer-card-top"><span className="focus-label"><span className={`status-dot ${running ? '' : 'paused'}`} />{inBreak ? '享受片刻休息' : running ? '心无旁骛，专注当下' : '慢一点，也没关系'}</span><button className="icon-button timer-reset" aria-label="重置计时" disabled={Boolean(postponedBreak)} title={postponedBreak ? '本次延迟不能重复或重置' : '重新开始这一轮计时'} onClick={() => { timer.resetTimer(); showToast('已重新开始这一轮专注') }}><RotateCcw size={17} /></button></div>
            <div className="timer-main"><div className="timer-copy"><p className="timer-kicker">{inBreak ? '这一刻，属于你' : postponedBreak ? `距离已延迟的${postponedBreak === 'long' ? '大' : '小'}休息` : '距离下一次小憩'}</p><div className="countdown" role="timer" aria-label={`剩余 ${time(remaining)}`}>{time(remaining).split(':')[0]}<span>:</span>{time(remaining).split(':')[1]}</div><p className="timer-description">{inBreak ? '放下手中的事，让身体轻轻松下来' : <><span>{phase === 'focus' && (postponedBreak === 'long' || (!postponedBreak && completedCycles >= settings.longEvery)) ? `${settings.longDuration} 分钟长休息` : `${settings.shortDuration} 秒短休息`}</span><span className="small-dot">·</span>让身心重新充电</>}</p><div className="timer-actions"><button className="button primary" disabled={Boolean(postponedBreak)} onClick={() => { initAudio(); timer.toggleRunning() }}>{running ? <Pause size={16} fill="currentColor" /> : <Play size={16} fill="currentColor" />}{postponedBreak ? '已延迟一次' : running ? '暂停计时' : '继续计时'}</button><button className="button light" onClick={() => beginBreak('short')}><Coffee size={17} />{postponedBreak ? '提前开始休息' : '现在休息'}</button></div></div><div className="hero-art"><img src="./illustrations/still-life.svg" alt="绿叶与平衡的石头，安静地享受阳光" /><span className="art-caption">a moment for yourself</span></div></div>
            <div className="timer-footer"><div className="cycle-dots" aria-label={`已完成 ${completedCycles} / ${settings.longEvery} 次短休息`}>{Array.from({ length: Math.min(settings.longEvery, 12) }).map((_, i) => <span key={i} className={i < completedCycles ? 'complete' : i === completedCycles ? 'current' : ''}>{i < completedCycles && <Check size={8} strokeWidth={3} />}</span>)}</div><span>每 {settings.longEvery} 次短休息，享受一次长休息</span><span className="cycle-count">{completedCycles}<span> / {settings.longEvery}</span></span></div>
            <div className="timer-progress" style={{ width: `${timer.progress * 100}%` }} />
          </section>
          <button className="calm-card" onClick={() => setBreathing(true)}><img src="./illustrations/forest.svg" alt="" /><div className="calm-card-content"><span className="calm-kicker"><Wind size={15} /> A MOMENT OF CALM</span><h2>不赶路的时候，<br />也在好好生活。</h2><p>把注意力，交还给呼吸。</p><span className="calm-link">一起深呼吸<ArrowUpRight size={16} /></span></div><span className="calm-index">01 — SLOW DOWN</span></button>
        </div>

        <div className="stats-grid">
          <div className="stat-card"><div className="stat-icon sage"><Coffee size={20} strokeWidth={1.6} /></div><div><span className="stat-label">今日休息</span><div className="stat-number">{stats.completedBreaks}<span>次</span></div></div><div className="stat-aside"><span className="tiny-leaf"><Leaf size={15} /></span><span>{stats.completedBreaks ? '每次停顿，都有意义' : '从第一次小憩开始'}</span></div></div>
          <div className="stat-card"><div className="stat-icon peach"><Clock3 size={20} strokeWidth={1.6} /></div><div><span className="stat-label">专注时光</span><div className="stat-number">{minuteLabel(stats.focusSeconds)}<span>分钟</span></div></div><div className="stat-aside"><div className="mini-bars" aria-hidden="true">{[10, 19, 15, 25, 20, 30, 24].map((h, i) => <i key={i} style={{ height: h }} />)}</div><span>一步一步，正在前进</span></div></div>
          <div className="stat-card"><div className="stat-icon lavender"><Heart size={20} strokeWidth={1.6} /></div><div><span className="stat-label">为自己留白</span><div className="stat-number">{minuteLabel(stats.breakSeconds)}<span>分钟</span></div></div><div className="stat-aside"><span className="little-sun"><Sun size={24} strokeWidth={1.3} /></span><span>照顾自己，也很重要</span></div></div>
        </div>

        <div className="lower-grid"><section className="inspiration-section"><div className="section-heading"><div><h2>小休息，换个好状态<span className="heading-dot">.</span></h2><p>不用做很多，做一点就很好。</p></div><button className="text-button" onClick={() => navigate('ideas')}>全部灵感<ArrowRight size={15} /></button></div><div className="exercise-grid">{exercises.map(item => <ExerciseCard key={item.id} exercise={item} onClick={() => setExercise(item)} />)}</div></section>
          <section className="rhythm-card"><div className="section-heading"><h2>接下来的节奏</h2><span className="small-muted">从容一点</span></div><div className="timeline"><div className="timeline-item current"><span className="timeline-dot" /><div><h3>{running ? '当下，安心专注' : '计时已暂停'}</h3><p>{running ? '让灵感慢慢流动' : '准备好后，继续你的节奏'}</p></div><span className="timeline-time">现在</span></div><div className="timeline-item"><span className="timeline-dot" /><div><h3>{completedCycles >= settings.longEvery ? '起身走走' : '给自己一个小憩'}</h3><p>{completedCycles >= settings.longEvery ? `${settings.longDuration} 分钟长休息` : `${settings.shortDuration} 秒短休息`}</p></div><span className="timeline-time">{running ? clockAfter(remaining) : '待继续'}</span></div><div className="timeline-item"><span className="timeline-dot" /><div><h3>好好放松，重新出发</h3><p>{settings.longDuration} 分钟长休息</p></div><span className="timeline-time">{!running ? '待继续' : settings.autoStart ? clockAfter(totalUpcoming) : '依节奏安排'}</span></div></div><button className="rhythm-edit" onClick={() => navigate('schedule')}><SlidersHorizontal size={14} />调整休息计划<ArrowRight size={15} /></button></section>
        </div>
      </div>}

      {page === 'schedule' && <div className="page-enter schedule-page"><section className="preset-section"><div className="section-heading"><div><h2>从一个适合你的节奏开始</h2><p>选择预设，也可以按自己的习惯慢慢调整。</p></div><span className="subtle-badge"><Sparkles size={13} /> 为日常而设计</span></div><div className="preset-grid">{[
        { title: '轻松办公', desc: '短暂休息，让状态持续在线', icon: Sprout, values: { shortInterval: 20, shortDuration: 20, longEvery: 4, longDuration: 5 } },
        { title: '深度专注', desc: '留出更完整的心流时间', icon: Coffee, values: { shortInterval: 25, shortDuration: 30, longEvery: 4, longDuration: 5 } },
        { title: '温柔节奏', desc: '更频繁地关照身体与情绪', icon: Heart, values: { shortInterval: 15, shortDuration: 30, longEvery: 3, longDuration: 5 } },
      ].map(preset => { const selected = Object.entries(preset.values).every(([key, value]) => draft[key as keyof typeof draft] === value); return <button className={`preset-card ${selected ? 'selected' : ''}`} key={preset.title} onClick={() => setDraft({ ...draft, ...preset.values })}><preset.icon size={24} strokeWidth={1.5} /><span className="preset-check">{selected && <Check size={12} />}</span><h3>{preset.title}</h3><p>{preset.desc}</p><span>每 {preset.values.shortInterval} 分钟，休息 {preset.values.shortDuration} 秒</span></button> })}</div></section>
        <div className="settings-pair"><section className="panel schedule-setting"><div className="panel-title"><span className="stat-icon sage"><Leaf size={20} /></span><div><h2>短休息</h2><p>为专注的日常，按下轻轻的暂停键。</p></div></div><label className="range-label" htmlFor="short-interval"><span>每隔多久提醒</span><strong>{draft.shortInterval}<small>分钟</small></strong></label><input id="short-interval" type="range" min="5" max="60" step="5" value={draft.shortInterval} onChange={e => setDraft({ ...draft, shortInterval: Number(e.target.value) })} /><div className="range-ends"><span>5 分钟</span><span>60 分钟</span></div><div className="select-row"><label htmlFor="short-duration">每次休息时长</label><select id="short-duration" value={draft.shortDuration} onChange={e => setDraft({ ...draft, shortDuration: Number(e.target.value) })}>{[20, 30, 45, 60, 90, 120].map(value => <option value={value} key={value}>{value < 60 ? `${value} 秒` : `${value / 60} 分钟`}</option>)}</select></div></section>
          <section className="panel schedule-setting"><div className="panel-title"><span className="stat-icon peach"><Coffee size={20} /></span><div><h2>长休息</h2><p>离开座位，给身体更完整的放松。</p></div></div><label className="range-label" htmlFor="long-duration"><span>每次休息时长</span><strong>{draft.longDuration}<small>分钟</small></strong></label><input id="long-duration" type="range" min="1" max="30" value={draft.longDuration} onChange={e => setDraft({ ...draft, longDuration: Number(e.target.value) })} /><div className="range-ends"><span>1 分钟</span><span>30 分钟</span></div><div className="select-row"><label htmlFor="long-every">长休息频率</label><select id="long-every" value={draft.longEvery} onChange={e => setDraft({ ...draft, longEvery: Number(e.target.value) })}>{[2, 3, 4, 5, 6, 8].map(value => <option value={value} key={value}>每 {value} 次短休息后</option>)}</select></div></section></div>
        <section className="schedule-preview panel"><div><h3>你的节奏，一目了然</h3><p>专注与休息交替，刚刚好。</p></div><div className="schedule-blocks">{Array.from({ length: Math.min(draft.longEvery, 8) }).map((_, index) => <div className="schedule-cycle" key={index}><span className="focus-block">{draft.shortInterval}m</span><span className="break-block" title={`${draft.shortDuration} 秒短休息`} /></div>)}<span className="focus-block last-focus">{draft.shortInterval}m</span><span className="long-block">{draft.longDuration}m</span></div><div className="schedule-legend"><span><i />专注</span><span><i />短休息</span><span><i />长休息</span></div></section>
        <div className="save-bar"><p><CheckCircle2 size={15} />计划会保存在这台设备上，修改后开始新一轮计时。</p><button className="button primary" onClick={() => { timer.updateSettings(draft); timer.resetTimer(); showToast(postponedBreak ? '新节奏已保存，将在本次休息结束后生效' : '新节奏已保存，从这一刻开始') }}><Check size={17} />保存我的节奏</button></div>
      </div>}

      {page === 'ideas' && <div className="page-enter ideas-page"><section className="ideas-banner"><div><span className="eyebrow">LESS DOING. MORE BEING.</span><h2>这一分钟，不必有所产出。</h2><p>抬头、伸展、呼吸。让自己重新回到当下。</p><button className="button primary" onClick={() => setBreathing(true)}><Wind size={17} />开始呼吸练习</button></div><img src="./illustrations/still-life.svg" alt="" /></section><div className="filter-row" role="group" aria-label="筛选休息灵感">{['全部灵感', '放松双眼', '舒展身体', '补充水分'].map(filter => <button className={ideaFilter === filter ? 'selected' : ''} key={filter} onClick={() => setIdeaFilter(filter)}>{filter}</button>)}</div><div className="exercise-grid large">{exercises.filter(item => ideaFilter === '全部灵感' || item.category === ideaFilter).map(item => <ExerciseCard key={item.id} exercise={item} onClick={() => setExercise(item)} />)}</div><div className="gentle-note"><Heart size={18} /><p>所有动作都以舒适为准。你也可以什么都不做，只是安静地待一会。</p></div></div>}

      {page === 'activity' && <div className="page-enter activity-page">
        <div className="activity-summary">
          <div>
            <span>{activityIsToday ? '今天的你，已经为自己留出了' : `${activityDateLabel}，你为自己留出了`}</span>
            <h2>{minuteLabel(activityStats.breakSeconds)}<small>分钟</small><Leaf size={30} strokeWidth={1.4} /></h2>
            <p>{activityStats.completedBreaks ? `完成了 ${activityStats.completedBreaks} 次休息。谢谢你，有认真照顾自己。` : '这一天还没有完成休息记录，慢慢来就好。'}</p>
          </div>
          <button className="button outline" onClick={exportHistory}><ArrowDownToLine size={16} />导出记录</button>
        </div>

        <section className="panel chart-panel">
          <div className="section-heading activity-chart-heading">
            <div><h2>一天的节奏</h2><p>看看专注与休息，在一天里如何自然交替。</p></div>
            <div className="activity-date-switcher" aria-label="选择记录日期">
              <button type="button" aria-label="前一天" disabled={activityDate <= oldestActivityDate} onClick={() => chooseActivityDate(shiftLocalDay(activityDate, -1))}><ChevronLeft size={15} /></button>
              <span><CalendarDays size={14} />{activityDateLabel}</span>
              <button type="button" aria-label="后一天" disabled={activityIsToday} onClick={() => chooseActivityDate(shiftLocalDay(activityDate, 1))}><ChevronRight size={15} /></button>
            </div>
          </div>
          <div className="activity-chart-legend" aria-label="图例"><span><i className="focus" />专注</span><span><i className="rest" />休息</span><small>本地记录 · 每小时</small></div>
          {hasHourlyActivity ? <>
            <div className="daily-chart" aria-label={`${activityDateLabel}每小时专注与休息图表`}>
              <div className="daily-chart-y" aria-hidden="true"><span>{activityDurationLabel(activityPeak)}</span><span>0</span></div>
              <div className="daily-chart-plot">
                {activityPoints.map(point => <button
                  type="button"
                  className={`daily-chart-column ${selectedHour === point.hour ? 'selected' : ''}`}
                  key={point.hour}
                  aria-label={`${String(point.hour).padStart(2, '0')}:00 至 ${String((point.hour + 1) % 24).padStart(2, '0')}:00，专注 ${activityDurationLabel(point.focusSeconds)}，休息 ${activityDurationLabel(point.breakSeconds)}`}
                  aria-pressed={selectedHour === point.hour}
                  onClick={() => setSelectedHour(point.hour)}
                >
                  <span className="daily-chart-track">
                    {point.totalSeconds > 0 && <span className="daily-chart-stack" style={{ height: `${point.heightPercent}%` }}>
                      <i className="focus" style={{ height: `${point.focusPercent}%` }} />
                      <i className="rest" style={{ height: `${point.breakPercent}%` }} />
                    </span>}
                  </span>
                  <span className="daily-chart-tick" aria-hidden="true">{point.hour % 2 === 0 ? String(point.hour).padStart(2, '0') : ''}</span>
                </button>)}
              </div>
            </div>
            <div className="hour-detail" aria-live="polite">
              <div className="hour-detail-title"><Clock3 size={17} /><span>{String(selectedActivityHour.hour).padStart(2, '0')}:00–{String((selectedActivityHour.hour + 1) % 24).padStart(2, '0')}:00</span></div>
              <div><i className="focus" /><span>专注</span><strong>{activityDurationLabel(selectedActivityHour.focusSeconds)}</strong></div>
              <div><i className="rest" /><span>休息</span><strong>{activityDurationLabel(selectedActivityHour.breakSeconds)}</strong></div>
              <span className="hour-detail-total">合计 {activityDurationLabel(selectedActivityHour.totalSeconds)}</span>
            </div>
          </> : <div className="chart-empty-state"><Activity size={27} strokeWidth={1.3} /><div><h3>这一天还没有分时记录</h3><p>{activityIsToday ? '从现在开始，专注与休息会在这里慢慢留下痕迹。' : '升级前的每日总量仍会保留，但不会猜测它发生在哪个小时。'}</p></div></div>}
        </section>

        <section className="panel history-panel">
          <div className="section-heading"><h2>{activityIsToday ? '今天的休息足迹' : '这一天的休息足迹'}</h2><span className="small-muted">已完成 {activityStats.completedBreaks} 次</span></div>
          {activityHistory.length ? <div className="history-list">{activityHistory.map(item => <div className="history-row" key={item.id}><span className={`stat-icon ${item.type === 'short' ? 'sage' : 'peach'}`}>{item.type === 'short' ? <Leaf size={18} /> : <Coffee size={18} />}</span><div><h3>{item.type === 'short' ? '片刻小憩' : '好好放松'}</h3><p>{new Date(item.completedAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}</p></div><span>{item.duration < 60 ? `${item.duration} 秒` : `${Math.round(item.duration / 60 * 10) / 10} 分钟`}</span><span className="history-complete"><CheckCircle2 size={14} />已完成</span></div>)}</div> : <div className="empty-state"><div className="empty-flower"><Sprout size={34} strokeWidth={1.2} /></div><h3>好习惯，从一个小小的停顿开始。</h3><p>{activityIsToday ? '完成第一次休息，让今天的留白在这里生根。' : '这一天没有完成的休息足迹。'}</p>{activityIsToday ? <button className="text-button" onClick={() => beginBreak('short')}>现在，歇一会<ArrowRight size={15} /></button> : <button className="text-button" onClick={() => chooseActivityDate(todayNoon)}>回到今天<ArrowRight size={15} /></button>}</div>}
        </section>
      </div>}

      {page === 'workConsole' && <WorkConsolePanel />}

      {page === 'phoneKey' && <div className="page-enter phone-key-page"><UnlockSettingsPanel bridge={window.repose?.unlock} /></div>}

      {page === 'settings' && <div className="page-enter preferences-page">
        <section className="panel preferences-panel security-panel">
          <div className="section-heading"><div><h2>Mac 屏幕保护</h2><p>休息时专心休息，离开时安心离开。</p></div><span className="subtle-badge"><Monitor size={13} />{window.repose ? 'Mac 桌面版' : '桌面版专属'}</span></div>
          <div className="preference-row"><span className="preference-icon"><ShieldCheck size={21} /></span><div><h3>强制休息</h3><p>覆盖全部显示器，屏蔽应用切换。每次可延迟一次；重新提醒后，倒计时完成前无法跳过、暂停或退出。</p></div><Toggle label="强制休息" enabled={desktopPreferences.strictBreaks} onChange={() => { if (!window.repose) { showToast('全屏强制休息需要打开 Repose Mac App'); return }; setDesktopPreferences(previous => ({ ...previous, strictBreaks: !previous.strictBreaks })) }} /></div>
          <div className="preference-row"><span className="preference-icon"><LockKeyhole size={21} /></span><div><h3>30 秒无操作，安全锁屏<span className="security-tag">系统级锁屏</span></h3><p>检测全局键盘和鼠标活动。连续 30 秒无操作后锁定 macOS，会话需正常认证解锁。暂停休息提醒不会关闭此保护。</p></div><Toggle label="30 秒无操作安全锁屏" enabled={desktopPreferences.idleLockEnabled} onChange={() => { if (!window.repose) { showToast('全局键鼠检测与系统锁屏需要使用 Repose Mac App'); return }; setSecurityError(false); setDesktopPreferences(previous => ({ ...previous, idleLockEnabled: !previous.idleLockEnabled })) }} /></div>
          <div className="security-permission"><LockKeyhole size={15} /><p>{window.repose ? '首次使用安全锁屏，请在系统设置中允许 Repose 使用辅助功能；如果系统询问自动化权限，也请允许。锁屏只检测空闲时长，不读取或记录按键内容。' : '网页仅预览界面。全局活动检测、跨屏遮罩和 macOS 安全锁屏均在 Mac App 中运行。'}</p>{window.repose && <button className="text-button" onClick={() => window.repose?.openSecuritySettings()}>打开系统设置<ArrowUpRight size={14} /></button>}</div>
          <p className="security-limit">强制休息限制日常操作；系统级结束进程或关机仍由 macOS 管理。</p>
        </section>
<section className="panel preferences-panel"><div className="section-heading"><h2>提醒与声音</h2></div><div className="preference-row"><span className="preference-icon"><Volume2 size={20} /></span><div><h3>温柔的提示音</h3><p>休息开始时，播放一声轻柔的和弦。</p></div><button className="text-button sound-preview" onClick={() => { initAudio(); setTimeout(chime, 50); showToast('这是休息开始时的提示音') }}>试听</button><Toggle label="温柔的提示音" enabled={settings.sound} onChange={() => { initAudio(); timer.updateSettings({ sound: !settings.sound }) }} /></div><div className="preference-row"><span className="preference-icon"><Bell size={20} /></span><div><h3>桌面通知</h3><p>{window.repose ? '休息开始时，在系统通知中提醒你。' : '休息开始时发送浏览器通知，需要允许通知权限。'}</p></div><Toggle label="桌面通知" enabled={settings.notifications} onChange={() => void toggleNotifications()} /></div><div className="preference-row"><span className="preference-icon"><Play size={20} /></span><div><h3>自动开启下一轮</h3><p>休息结束后，自动开始新的专注计时。</p></div><Toggle label="自动开启下一轮" enabled={settings.autoStart} onChange={() => timer.updateSettings({ autoStart: !settings.autoStart })} /></div></section><section className="panel preferences-panel"><div className="section-heading"><div><h2>你的空间，你的颜色</h2><p>选一个让眼睛舒服、让心情放松的外观。</p></div></div><div className="theme-grid">{([{ id: 'light', title: '日光暖白', subtitle: '明亮而温柔', icon: Sun }, { id: 'dark', title: '静谧森林', subtitle: '安静的深色空间', icon: Moon }, { id: 'system', title: '跟随系统', subtitle: '随你的设备自动切换', icon: Settings2 }] as const).map(item => <button className={`theme-option ${theme === item.id ? 'selected' : ''}`} key={item.id} onClick={() => setTheme(item.id)}><div className={`theme-preview ${item.id}`}><span /><div><i /><i /><i /></div></div><div><item.icon size={15} /><span>{item.title}</span>{theme === item.id && <CheckCircle2 size={15} />}</div><p>{item.subtitle}</p></button>)}</div></section><section className="panel about-panel"><BrandMark /><div><h3>Repose · 歇一会<span>v{APP_VERSION}</span></h3><p>给日常，留一点空白。{window.repose ? '桌面版 · 托盘持续运行' : '浏览器版 · 保持页面打开以接收提醒'}</p></div><button className="text-button" onClick={() => setHelp(true)}>使用指南<ArrowUpRight size={15} /></button></section><div className="preferences-footer"><span><CheckCircle2 size={14} />偏好设置会自动保存到这台设备</span><button className="text-button" onClick={() => { timer.resetSettings(); setTheme('light'); showToast('已恢复默认偏好与休息计划，休息记录保留') }}><RotateCcw size={13} />恢复默认设置</button></div></div>}

      <footer className="page-footer"><span><Leaf size={13} strokeWidth={1.5} />更好的状态，来自恰到好处的停顿。</span><span>MADE FOR A SLOWER, BETTER DAY<span className="footer-flower">✳</span></span></footer>
    </main>

    {toast && <div className="toast" role="status"><CheckCircle2 size={17} />{toast}</div>}
    {help && <Modal label="认识 Repose" onClose={() => setHelp(false)} className="help-modal"><button className="modal-close icon-button" aria-label="关闭使用指南" onClick={() => setHelp(false)}><X size={21} /></button><BrandMark /><div className="eyebrow">WELCOME TO YOUR LITTLE PAUSE</div><h2>嗨，这里是 Repose<span>.</span></h2><p className="modal-intro">一位安静的休息伙伴，陪你在忙碌日常里，找回舒服的节奏。</p><div className="help-step"><span>01</span><div><h3>专注的时候，放心投入</h3><p>计时会自动进行。你可以随时暂停，或按空格键切换。</p></div></div><div className="help-step"><span>02</span><div><h3>到点了，温柔地歇一会</h3><p>{window.repose ? '默认每 20 分钟短休息 20 秒，完成 4 次后安排长休息。小休息可延迟 1 分钟，大休息可延迟 5 分钟，每次仅一次。再次提醒后须完成完整休息。' : '默认每 20 分钟短休息 20 秒，完成 4 次后安排长休息。'}</p></div></div><div className="help-step"><span>03</span><div><h3>让休息，变成你的习惯</h3><p>在「休息计划」调整节奏，在「我的记录」查看真实的休息足迹。所有记录只保存在本机。</p></div></div><div className="help-platform"><Leaf size={17} /><p>{window.repose ? '关闭窗口后，Repose 会留在托盘继续提醒。通过托盘菜单可完整退出。' : '浏览器版需要保持页面打开；关闭页面后无法提醒。桌面版支持托盘持续运行。'}</p></div><button className="button primary full-width" onClick={() => setHelp(false)}>好的，慢慢来<ArrowRight size={16} /></button></Modal>}
    {exercise && <Modal label={exercise.title} onClose={() => setExercise(null)} className="exercise-modal"><button className="modal-close icon-button" aria-label="关闭休息灵感" onClick={() => setExercise(null)}><X size={21} /></button><div className={`exercise-modal-art ${exercise.color}`}><img src={`./illustrations/${exercise.art}.svg`} alt="" /></div><div className="exercise-modal-body"><span className="subtle-badge"><exercise.icon size={14} />{exercise.category}<span className="small-dot">·</span>{exercise.type === 'short' ? `${settings.shortDuration} 秒` : `${settings.longDuration} 分钟`}</span><h2>{exercise.title}</h2><p>{exercise.subtitle}</p><ol>{exercise.steps.map(step => <li key={step}>{step}</li>)}</ol><button className="button primary full-width" onClick={() => beginBreak(exercise.type)}><Play size={16} fill="currentColor" />开始这次休息</button></div></Modal>}
    {breathing && <Modal label="呼吸练习" onClose={() => setBreathing(false)} className="breathing-modal"><button className="modal-close icon-button" aria-label="结束呼吸练习" onClick={() => setBreathing(false)}><X size={21} /></button><span className="eyebrow">JUST BREATHE</span><h2>现在，只需要呼吸。</h2><p>不必追赶什么，跟随舒服的节奏。</p><div className={`breathing-orbit ${cycle < 4 ? 'inhale' : cycle < 8 ? 'hold' : 'exhale'}`}><div className="breathing-ring outer" /><div className="breathing-ring middle" /><div className="breathing-circle"><Wind size={28} strokeWidth={1.2} /><span aria-live="polite">{breathLabel}</span><strong>{breathCountdown}</strong></div></div><div className="breathing-steps"><span className={cycle < 4 ? 'active' : ''}>吸气 4 秒</span><span className={cycle >= 4 && cycle < 8 ? 'active' : ''}>停留 4 秒</span><span className={cycle >= 8 ? 'active' : ''}>呼气 6 秒</span></div><p className="breathing-count">已完成 {Math.floor(breathSeconds / 14)} 轮<span className="small-dot">·</span>按自己的舒适程度呼吸</p><button className="button outline" onClick={() => { setBreathing(false); showToast('把这份从容，带回接下来的时光') }}>带着平静，继续</button></Modal>}
    {inBreak && <Modal label="休息时间" onClose={() => {}} className={`break-modal ${phase === 'long' ? 'long-break-modal' : ''}`}><div className="break-modal-top"><BrandMark small /><span>REPOSE · A LITTLE TIME FOR YOU</span><span className="subtle-badge">{phase === 'long' ? '长休息 · 跟练模式' : '短休息'}</span></div><div className={`break-content ${phase === 'long' ? 'long-break-content' : ''}`}>{phase === 'long' ? <StretchTrainer3D key={breakId} remaining={remaining} duration={timer.phaseDuration} running={running} /> : <><div className="break-art short-break-mascot"><img src="./favicon.svg" alt="" /></div><span className="eyebrow">REPOSE HAS ENTERED THE CHAT</span><h2>{shortVoice.title}</h2><p>{shortVoice.body}</p></>}<div className="break-total-label">{phase === 'long' ? '大休息剩余' : '本次休息剩余'}</div><div className="break-timer" role="timer" aria-label={`休息剩余 ${time(remaining)}`}>{time(remaining)}</div><div className="break-progress"><span style={{ width: `${timer.progress * 100}%` }} /></div><span className="break-encouragement">{running ? phase === 'long' ? '跟着舒服的幅度慢慢活动，不必追求标准。' : '二十秒而已。我相信你和工作都撑得住。' : '休息计时已暂停。你很会给休息再安排一次休息。'}</span><div className="break-actions">
      {canPostpone && <button className="button postpone-button" disabled={postponePending} onClick={() => void postponeCurrentBreak()}><Clock3 size={16} />{postponePending ? '正在延迟…' : `延迟 ${postponeSeconds / 60} 分钟`}<span>仅此一次</span></button>}
      {strictBreak ? <span className="strict-break-note"><ShieldCheck size={15} />{canPostpone ? '准备好后，安心休息' : '已使用延迟机会，倒计时结束后自动恢复'}</span> : <><button className="button primary" onClick={timer.toggleRunning}>{running ? <Pause size={16} /> : <Play size={16} />}{running ? '暂停休息' : '继续休息'}</button><button className="text-button" onClick={() => { timer.skipBreak(); showToast('已跳过这次休息，记得稍后照顾一下自己') }}>跳过这次<ArrowRight size={15} /></button></>}
    </div></div><div className="break-bottom"><Heart size={13} />{phase === 'long' ? '动作以舒适为准；如有疼痛或眩晕，请立即停止。' : '不必做得完美，照顾自己就好。'}</div></Modal>}
  </div>
}
