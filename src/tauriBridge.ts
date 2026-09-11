import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { UnlockDesktopBridge } from './lib/unlock'
import type { ConsoleDesktopBridge } from './lib/console'

export type DesktopCommandName =
  | 'toggle-pause'
  | 'start-short-break'
  | 'start-long-break'
  | 'postpone-break'
  | 'strict-break-finished'
  | 'idle-lock-failed'
  // Phone Key drill results (§6.4). Must also appear in `commandNames` below,
  // or the allowlist in parseCommand silently drops them.
  | 'unlock-drill-passed'
  | 'unlock-drill-failed'
  | 'unlock-drill-not-observed'

export type DesktopCommand = { command: DesktopCommandName; breakId: string | null }
export type DesktopLifecycleEvent =
  | { type: 'inactive-start'; intervalId: string; sequence: number; reason: string; startedAt: number }
  | { type: 'inactive-end'; intervalId: string; sequence: number; elapsedSeconds: number; startedAt: number; endedAt: number }

type LifecycleSnapshot = {
  inactive: boolean
  activeInterval: Extract<DesktopLifecycleEvent, { type: 'inactive-start' }> | null
  pendingIntervals: Extract<DesktopLifecycleEvent, { type: 'inactive-end' }>[]
}
type Status = { running: boolean; phase: string; remaining: number; breakId: string | null; canPostpone: boolean; postponeSeconds: number }
type Preferences = { strictBreaks: boolean; idleLockEnabled: boolean; idleLockSeconds: 30 }

declare global {
  interface Window {
    repose?: {
      isDesktop: boolean
      onCommand: (callback: (event: DesktopCommand) => void) => () => void
      onLifecycle: (callback: (event: DesktopLifecycleEvent) => void) => () => void
      acknowledgeLifecycle: (intervalId: string) => Promise<boolean>
      setStatus: (status: Status) => void
      setPreferences: (preferences: Preferences) => void
      notify: (notification: { title: string; body: string }) => void
      showBreak: () => void
      postponeBreak: () => Promise<boolean>
      openSecuritySettings: () => void
      unlock?: UnlockDesktopBridge
      console?: ConsoleDesktopBridge
    }
    webkitAudioContext?: typeof AudioContext
  }
}

const consoleCommandCallbacks = new Set<(e: { action: string | null; app?: string; ok: boolean; detail?: string }) => void>()

const commandNames = new Set<DesktopCommandName>([
  'toggle-pause',
  'start-short-break',
  'start-long-break',
  'postpone-break',
  'strict-break-finished',
  'idle-lock-failed',
  'unlock-drill-passed',
  'unlock-drill-failed',
  'unlock-drill-not-observed',
])

function parseCommand(value: unknown): DesktopCommand | null {
  if (!value || typeof value !== 'object') return null
  const event = value as Partial<DesktopCommand>
  if (!event.command || !commandNames.has(event.command)) return null
  if (event.breakId !== null && typeof event.breakId !== 'string') return null
  return { command: event.command, breakId: event.breakId ?? null }
}

function parseLifecycle(value: unknown): DesktopLifecycleEvent | null {
  if (!value || typeof value !== 'object') return null
  const event = value as Partial<DesktopLifecycleEvent>
  if (typeof event.intervalId !== 'string' || !event.intervalId || event.intervalId.length > 200) return null
  if (!Number.isSafeInteger(event.sequence) || event.sequence! < 0) return null
  if (event.type === 'inactive-start') {
    if (typeof event.reason !== 'string' || !Number.isFinite(event.startedAt)) return null
    return { type: event.type, intervalId: event.intervalId, sequence: event.sequence!, reason: event.reason, startedAt: event.startedAt! }
  }
  if (event.type === 'inactive-end') {
    if (!Number.isFinite(event.elapsedSeconds) || event.elapsedSeconds! < 0
      || !Number.isFinite(event.startedAt) || !Number.isFinite(event.endedAt)) return null
    return {
      type: event.type,
      intervalId: event.intervalId,
      sequence: event.sequence!,
      elapsedSeconds: event.elapsedSeconds!,
      startedAt: event.startedAt!,
      endedAt: event.endedAt!,
    }
  }
  return null
}

function lifecycleOrder(left: DesktopLifecycleEvent, right: DesktopLifecycleEvent) {
  if (left.sequence !== right.sequence) return left.sequence - right.sequence
  if (left.type === right.type) return 0
  return left.type === 'inactive-start' ? -1 : 1
}

export async function initializeDesktopBridge() {
  if (!isTauri()) return

  const commandCallbacks = new Set<(event: DesktopCommand) => void>()
  const lifecycleCallbacks = new Set<(event: DesktopLifecycleEvent) => void>()
  const queuedLifecycle: DesktopLifecycleEvent[] = []
  const queuedKeys = new Set<string>()
  let activeLifecycle: Extract<DesktopLifecycleEvent, { type: 'inactive-start' }> | null = null
  const dispatchLifecycle = (event: DesktopLifecycleEvent) => {
    if (event.type === 'inactive-start') activeLifecycle = event
    else if (activeLifecycle?.intervalId === event.intervalId) activeLifecycle = null
    if (lifecycleCallbacks.size) {
      lifecycleCallbacks.forEach(callback => callback(event))
      return
    }
    const key = `${event.type}:${event.intervalId}`
    if (!queuedKeys.has(key)) {
      queuedKeys.add(key)
      queuedLifecycle.push(event)
    }
  }

  const unlockSnapshotCallbacks = new Set<(snapshot: unknown) => void>()
  const unlockPresenceCallbacks = new Set<(presence: unknown) => void>()

  await listen<unknown>('repose-command', event => {
    const command = parseCommand(event.payload)
    if (command) commandCallbacks.forEach(callback => callback(command))
  })
  await listen<unknown>('repose-lifecycle', event => {
    const lifecycle = parseLifecycle(event.payload)
    if (lifecycle) dispatchLifecycle(lifecycle)
  })
  await listen<unknown>('repose-unlock-snapshot', event => {
    unlockSnapshotCallbacks.forEach(callback => callback(event.payload))
  })
  await listen<unknown>('repose-unlock-presence', event => {
    unlockPresenceCallbacks.forEach(callback => callback(event.payload))
  })

  // Phone Key bridge. Method names map to unlock.rs commands (§6.2); the panel
  // re-normalizes every returned snapshot as untrusted input.
  const unlock: UnlockDesktopBridge = {
    getSnapshot: () => invoke<unknown>('unlock_get_snapshot'),
    preflight: () => invoke<unknown>('unlock_preflight'),
    install: value => invoke<unknown>('unlock_install', { value }),
    repair: value => invoke<unknown>('unlock_repair', { value }),
    uninstall: () => invoke<unknown>('unlock_uninstall'),
    setEnabled: value => invoke<unknown>('unlock_set_enabled', { value }),
    setPresenceRunning: value => invoke<unknown>('unlock_presence_set', { value }),
    revokeDevice: value => invoke<unknown>('unlock_revoke_device', { value }),
    beginPairing: () => invoke<unknown>('unlock_pair_begin'),
    pollPairing: () => invoke<unknown>('unlock_pair_poll'),
    confirmPairing: () => invoke<unknown>('unlock_pair_confirm'),
    awaitPhonePairing: () => invoke<unknown>('unlock_pair_await_phone'),
    async cancelPairing() { await invoke('unlock_pair_cancel') },
    calibrateStart: value => invoke<unknown>('unlock_calibrate_start', { value }),
    calibrateSample: () => invoke<unknown>('unlock_calibrate_sample'),
    calibrateFinish: () => invoke<unknown>('unlock_calibrate_finish'),
    startDrill: value => invoke<unknown>('unlock_drill_start', { value }),
    async openBluetoothSettings() { await invoke('unlock_open_bluetooth_settings') },
    async openLockScreenSettings() { await invoke('unlock_open_lock_screen_settings') },
    onSnapshot(callback) { unlockSnapshotCallbacks.add(callback); return () => unlockSnapshotCallbacks.delete(callback) },
    onPresence(callback) { unlockPresenceCallbacks.add(callback); return () => unlockPresenceCallbacks.delete(callback) },
  }

  // A command from the phone lands wherever the user is looking, so this is
  // global rather than a listener on the shortcuts page. The phone only ever
  // knows it SENT something -- whether a key was pressed is the Mac's to say.
  void listen<{ action: string | null; app?: string; ok: boolean; detail?: string }>(
    'console-command',
    ({ payload }) => {
      consoleCommandCallbacks.forEach(cb => cb(payload))
    },
  )

  const consoleBridge: ConsoleDesktopBridge = {
    status: () => invoke<unknown>('console_status'),
    requestTrust: () => invoke<boolean>('console_request_trust'),
    run: value => invoke<void>('console_run', { value }),
    onCommand(callback) {
      consoleCommandCallbacks.add(callback)
      return () => consoleCommandCallbacks.delete(callback)
    },
    pickApp: () => invoke<unknown>('console_pick_app'),
    save: value => invoke<unknown>('console_save', { value }),
  }

  window.repose = {
    isDesktop: true,
    console: consoleBridge,
    onCommand(callback) {
      commandCallbacks.add(callback)
      return () => commandCallbacks.delete(callback)
    },
    onLifecycle(callback) {
      lifecycleCallbacks.add(callback)
      let replayedActive = false
      if (queuedLifecycle.length) {
        const replay = queuedLifecycle.splice(0).sort(lifecycleOrder)
        queuedKeys.clear()
        replay.forEach(event => {
          if (event.type === 'inactive-start' && event.intervalId === activeLifecycle?.intervalId) replayedActive = true
          callback(event)
        })
      }
      if (activeLifecycle && !replayedActive) callback(activeLifecycle)
      return () => lifecycleCallbacks.delete(callback)
    },
    acknowledgeLifecycle(intervalId) { return invoke<boolean>('acknowledge_lifecycle_interval', { intervalId }) },
    setStatus(status: Status) { void invoke('set_status', { value: status }) },
    setPreferences(preferences: Preferences) { void invoke('set_preferences', { value: preferences }) },
    notify(notification) { void invoke('notify_user', { value: notification }) },
    showBreak() { /* set_status creates the native cover after the phase changes. */ },
    postponeBreak() { return invoke<boolean>('postpone_break') },
    openSecuritySettings() { void invoke('open_security_settings') },
    unlock,
  }

  try {
    const snapshot = await invoke<LifecycleSnapshot>('get_lifecycle_snapshot')
    snapshot.pendingIntervals.map(parseLifecycle).forEach(event => { if (event) dispatchLifecycle(event) })
    const active = parseLifecycle(snapshot.activeInterval)
    if (active) dispatchLifecycle(active)
  } catch {
    // Live lifecycle events remain authoritative if startup replay is unavailable.
  }
}
