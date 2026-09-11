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
  /**
   * Carried, not interpreted.
   *
   * The config distinguishes "hotkey" (one keystroke) from "sequence" (several).
   * Nothing here reads it -- run_action just performs the steps -- but dropping
   * it from this type meant every action the panel round-tripped came back
   * without it, and a save then flattened the whole file to one value. The
   * user's data is not ours to simplify because we happen not to use a field.
   */
  kind: string | null
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

export type PickedApp = { name: string; bundleId: string; path: string; icon: string | null }

export type ConsoleDesktopBridge = {
  status: () => Promise<unknown>
  requestTrust: () => Promise<boolean>
  run: (value: { appId: string; actionId: string }) => Promise<void>
  pickApp: () => Promise<unknown>
  save: (value: { config: unknown }) => Promise<unknown>
}

/**
 * A keystroke as the browser reports it, turned into the shape the Mac presses.
 *
 * Recorded rather than typed. Asking someone to write "b" and then tick three
 * checkboxes is asking them to transcribe something they could simply do, and
 * transcription is where ⌃ becomes ⌘.
 *
 * Shift is deliberately NOT recorded for a printable character. The browser
 * already reports the shifted character -- ⇧5 arrives as "%" -- and
 * work_console.m derives the shift it needs from the character itself. Storing
 * both would press shift twice and show the pill as ⇧%. That is also exactly
 * how the config already on disk spells it: {"key": "%", "modifiers": []}.
 */
export function stepFromKeyboardEvent(e: {
  key: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
}): ConsoleStep | null {
  // A bare modifier is not a keystroke; the recorder keeps waiting.
  if (['Meta', 'Control', 'Alt', 'Shift', 'CapsLock'].includes(e.key)) return null

  const named: Record<string, string> = {
    Enter: 'return', Escape: 'escape', Tab: 'tab', ' ': 'space', Backspace: 'delete',
    ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right',
    Home: 'home', End: 'end', PageUp: 'pageup', PageDown: 'pagedown',
  }
  const modifiers: string[] = []
  if (e.metaKey) modifiers.push('cmd')
  if (e.ctrlKey) modifiers.push('ctrl')
  if (e.altKey) modifiers.push('alt')

  const name = named[e.key]
  if (name) {
    // Named keys have no shifted spelling, so shift has to be carried.
    if (e.shiftKey) modifiers.push('shift')
    return { key: name, modifiers, delayMs: 0 }
  }
  if (e.key.length !== 1) return null
  return { key: e.key, modifiers, delayMs: 0 }
}

/**
 * Stable enough for a config file, and readable when someone opens one.
 *
 * Letters and digits of any script, not just a-z: stripping non-ASCII turned
 * every Chinese action name into the same id, so 左右分屏 and 上下分屏 became
 * "x" and "x-2" — and find_action looks actions up by id, so a file anyone
 * hand-edits would be a file where the names and the ids say different things.
 */
export function slug(name: string, taken: string[]): string {
  const base = name
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/^-|-$/g, '') || 'x'
  let id = base
  let n = 2
  while (taken.includes(id)) id = `${base}-${n++}`
  return id
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
          return [{ id: aid, name: aname, icon: str(c.icon), kind: str(c.kind), steps: steps(c.steps) }]
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
