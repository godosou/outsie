// 快捷控制 — what this Mac can be asked to press.
//
// Everything here treats the reply from the desktop as untrusted input, the
// same way the phone-key panel does: a malformed status must render as "nothing
// configured", never as a page that refuses to draw or, worse, as buttons that
// claim to do something.

export type ConsoleStep = {
  key: string
  modifiers: string[]
  delayMs: number
}

export type ConsoleAction = {
  id: string
  name: string
  icon: string | null
  steps: ConsoleStep[]
}

export type ConsoleApp = {
  id: string
  name: string
  bundleId: string
  appPath: string | null
  actions: ConsoleAction[]
}

export type ConsoleStatus = {
  /** Whether macOS will let this app press keys in another one. */
  trusted: boolean
  apps: ConsoleApp[]
}

export type ConsoleDesktopBridge = {
  status: () => Promise<unknown>
  requestTrust: () => Promise<boolean>
  run: (value: { appId: string; actionId: string }) => Promise<void>
}

export const EMPTY_CONSOLE: ConsoleStatus = { trusted: false, apps: [] }

function str(v: unknown): string | null {
  return typeof v === 'string' && v.trim() !== '' ? v : null
}

function steps(v: unknown): ConsoleStep[] {
  if (!Array.isArray(v)) return []
  return v.flatMap(raw => {
    if (!raw || typeof raw !== 'object') return []
    const o = raw as Record<string, unknown>
    const key = typeof o.key === 'string' ? o.key : ''
    const modifiers = Array.isArray(o.modifiers)
      ? o.modifiers.filter((m): m is string => typeof m === 'string')
      : []
    const delayMs = typeof o.delayMs === 'number' && Number.isFinite(o.delayMs) ? o.delayMs : 0
    return [{ key, modifiers, delayMs }]
  })
}

export function normalizeConsoleStatus(raw: unknown): ConsoleStatus {
  const o = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>
  const cfg = (o.config && typeof o.config === 'object' ? o.config : {}) as Record<string, unknown>
  const apps = Array.isArray(cfg.apps) ? cfg.apps : []
  return {
    trusted: o.trusted === true,
    apps: apps.flatMap(raw => {
      if (!raw || typeof raw !== 'object') return []
      const a = raw as Record<string, unknown>
      const id = str(a.id)
      const name = str(a.name)
      const bundleId = str(a.bundleId)
      // An app with no id cannot be acted on -- console_run finds it by id --
      // so listing it would be a row whose buttons are all dead.
      if (!id || !name || !bundleId) return []
      const actions = Array.isArray(a.actions) ? a.actions : []
      return [{
        id,
        name,
        bundleId,
        appPath: str(a.appPath),
        actions: actions.flatMap(raw => {
          if (!raw || typeof raw !== 'object') return []
          const c = raw as Record<string, unknown>
          const aid = str(c.id)
          const aname = str(c.name)
          if (!aid || !aname) return []
          return [{ id: aid, name: aname, icon: str(c.icon), steps: steps(c.steps) }]
        }),
      }]
    }),
  }
}

/** Mirrors console.rs `step_label`, so the two screens spell one shortcut one way. */
export function stepLabel(step: ConsoleStep): string {
  const sym: Record<string, string> = {
    cmd: '⌘', command: '⌘', meta: '⌘',
    ctrl: '⌃', control: '⌃',
    alt: '⌥', option: '⌥', opt: '⌥',
    shift: '⇧',
  }
  return step.modifiers.map(m => sym[m.toLowerCase()] ?? m).join('') + step.key
}

export type ActionHealth = 'ok' | 'empty' | 'missing-key'

/** Mirrors console.rs `action_health`. A button certain to fail is not drawn. */
export function actionHealth(action: ConsoleAction): ActionHealth {
  if (action.steps.length === 0) return 'empty'
  if (action.steps.some(s => s.key.trim() === '')) return 'missing-key'
  return 'ok'
}
