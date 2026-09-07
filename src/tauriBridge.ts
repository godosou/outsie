import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

type Command = 'toggle-pause' | 'start-short-break' | 'start-long-break' | 'postpone-break' | 'strict-break-finished' | 'idle-lock-failed'
type Status = { running: boolean; phase: string; remaining: number; breakId: string | null; canPostpone: boolean; postponeSeconds: number }
type Preferences = { strictBreaks: boolean; idleLockEnabled: boolean; idleLockSeconds: 30 }

export type UnlockInvoker = (command: string, args?: Record<string, unknown>) => Promise<unknown>

export interface UnlockDesktopBridge {
  unlockStatus: () => Promise<unknown>
  beginPairing: () => Promise<unknown>
  confirmPairing: (sessionId: string) => Promise<unknown>
  beginCalibration: (deviceId: string) => Promise<unknown>
  revokeDevice: (deviceId: string) => Promise<unknown>
  openUnlockDiagnostics: () => Promise<unknown>
}

/** The renderer never receives a generic command or raw byte transport surface. */
export function createUnlockBridge(call: UnlockInvoker = (command, args) => invoke(command, args)): UnlockDesktopBridge {
  return {
    unlockStatus: () => call('unlock_status'),
    beginPairing: () => call('begin_pairing'),
    confirmPairing: sessionId => call('confirm_pairing', { value: { sessionId } }),
    beginCalibration: deviceId => call('begin_calibration', { value: { deviceId } }),
    revokeDevice: deviceId => call('revoke_device', { value: { deviceId } }),
    openUnlockDiagnostics: () => call('open_unlock_diagnostics'),
  }
}

export async function initializeDesktopBridge() {
  if (!isTauri()) return

  const callbacks = new Set<(command: Command) => void>()
  await listen<Command>('repose-command', event => callbacks.forEach(callback => callback(event.payload)))

  window.repose = {
    isDesktop: true,
    onCommand(callback) {
      callbacks.add(callback)
      return () => callbacks.delete(callback)
    },
    setStatus(status: Status) { void invoke('set_status', { value: status }) },
    setPreferences(preferences: Preferences) { void invoke('set_preferences', { value: preferences }) },
    notify(notification) { void invoke('notify_user', { value: notification }) },
    showBreak() { /* set_status creates the native cover after the phase changes. */ },
    postponeBreak() { return invoke<boolean>('postpone_break') },
    openSecuritySettings() { void invoke('open_security_settings') },
    unlock: createUnlockBridge(),
  }
}
