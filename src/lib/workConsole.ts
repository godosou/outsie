export type ConsoleModifier = 'meta' | 'ctrl' | 'alt' | 'shift'
export type ConsoleStep = { key: string; modifiers: ConsoleModifier[]; delayMs: number }
export type ConsoleAction = { id: string; name: string; icon: string; kind: 'hotkey' | 'sequence'; steps: ConsoleStep[] }
export type ConsoleApp = { id: string; name: string; bundleId: string; actions: ConsoleAction[] }
export type ConsoleConfig = { revision: number; apps: ConsoleApp[] }
export type ConsoleStatus = { config: ConsoleConfig; enabled: boolean; connected: boolean; running: boolean; activeAppId: string | null; lastError: string | null; accessibility: boolean; blocked: boolean; transport?: 'bluetooth'; pairedDevices?: { id: string; name: string }[]; bluetoothReady?: boolean }
export interface WorkConsoleBridge {
  status(): Promise<ConsoleStatus>
  save(config: ConsoleConfig): Promise<ConsoleStatus>
  reset(appId: string, revision: number): Promise<ConsoleStatus>
  start(): Promise<ConsoleStatus>
  stop(): Promise<ConsoleStatus>
  run(appId: string, actionId: string): Promise<ConsoleStatus>
  cancel(): Promise<ConsoleStatus>
  accessibility(): Promise<ConsoleStatus>
}

export const MODIFIERS: ConsoleModifier[] = ['meta', 'ctrl', 'alt', 'shift']
export const MODIFIER_LABELS: Record<ConsoleModifier, string> = { meta: '⌘', ctrl: '⌃', alt: '⌥', shift: '⇧' }
const specialKeys = new Set(['Enter', 'Tab', 'Backspace', 'Delete', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown', 'Space', 'Escape', ...Array.from({ length: 20 }, (_, i) => `F${i + 1}`)])
export function isConsoleKey(key: string): boolean {
  return specialKeys.has(key) || /^[\x21-\x7e]$/.test(key)
}
const codeKeys: Record<string, string> = { Space: 'Space', Backquote: '`', Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']', Backslash: '\\', Semicolon: ';', Quote: "'", Comma: ',', Period: '.', Slash: '/' }
type RecordingKey = Pick<KeyboardEvent, 'key' | 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'repeat' | 'isComposing'>

/** Preserve printable keys; Option-generated non-ASCII characters fall back to their base key. */
export function recordConsoleKey(event: RecordingKey, delayMs: number): ConsoleStep | null {
  if (event.repeat || event.isComposing || ['Escape', 'Dead', 'Process', 'Unidentified'].includes(event.key)) return null
  const key = /^[\x21-\x7e]$/.test(event.key) ? event.key.toLowerCase()
    : /^Key[A-Z]$/.test(event.code) ? event.code.slice(3).toLowerCase()
    : /^Digit[0-9]$/.test(event.code) ? event.code.slice(5)
      : codeKeys[event.code] ?? (event.key === ' ' ? 'Space' : event.key.length === 1 ? event.key.toLowerCase() : event.key)
  if (!isConsoleKey(key)) return null
  const modifiers = MODIFIERS.filter(modifier => ({ meta: event.metaKey, ctrl: event.ctrlKey, alt: event.altKey, shift: event.shiftKey })[modifier])
  return { key, modifiers, delayMs: Math.min(5000, Math.max(0, Math.round(delayMs))) }
}

export function formatConsoleStep(step: ConsoleStep): string {
  return `${MODIFIERS.filter(modifier => step.modifiers.includes(modifier)).map(modifier => MODIFIER_LABELS[modifier]).join('')}${step.key.length === 1 ? step.key.toUpperCase() : step.key}`
}

export function formatConsoleSequence(steps: ConsoleStep[]): string {
  return steps.flatMap(step => [
    ...(!Number.isFinite(step.delayMs) ? ['等待时间待设置'] : step.delayMs > 0 ? [`等待${step.delayMs / 1000}秒`] : []),
    formatConsoleStep(step),
  ]).join(' → ')
}

export function validateConsoleConfig(config: ConsoleConfig): string | null {
  if (config.apps.length < 1 || config.apps.length > 16) return '请保留 1–16 个 App。'
  if (new Set(config.apps.map(app => app.id)).size !== config.apps.length) return 'App ID 重复，请重新加载配置。'
  for (const app of config.apps) {
    if (!app.name.trim() || Array.from(app.name).length > 64) return 'App 名称需要 1–64 个字符。'
    if (!/^[A-Za-z0-9][A-Za-z0-9.-]+$/.test(app.bundleId)) return `${app.name} 需要有效的 App Bundle ID。`
    if (app.actions.length > 12) return '每个 App 最多 12 个操作。'
    if (new Set(app.actions.map(action => action.id)).size !== app.actions.length) return '操作 ID 重复，请重新加载配置。'
    for (const action of app.actions) {
      if (!action.name.trim() || Array.from(action.name).length > 64) return '操作名称需要 1–64 个字符。'
      if (!action.icon.trim() || Array.from(action.icon).length > 16) return '请填写简短的按钮图标。'
      if (action.steps.length < 1 || action.steps.length > 20 || (action.kind === 'hotkey' && action.steps.length !== 1)) return `${action.name}：快捷键需要 1 步，键盘序列需要 1–20 步。`
      for (const step of action.steps) {
        if (!isConsoleKey(step.key)) return `${action.name}：按键 ${step.key || '（空）'} 不受支持。`
        if (!Number.isInteger(step.delayMs) || step.delayMs < 0 || step.delayMs > 5000) return `${action.name}：等待时间须为 0–5 秒。`
        if (new Set(step.modifiers).size !== step.modifiers.length || step.modifiers.some(modifier => !MODIFIERS.includes(modifier))) return `${action.name}：修饰键无效。`
      }
    }
  }
  return null
}

export function moveConsoleStep(steps: ConsoleStep[], index: number, offset: -1 | 1): ConsoleStep[] {
  const destination = index + offset
  if (destination < 0 || destination >= steps.length) return steps
  const result = [...steps]
  ;[result[index], result[destination]] = [result[destination], result[index]]
  return result
}

export function createConsoleBridge(invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>): WorkConsoleBridge {
  return {
    status: () => invoke('console_status'),
    save: config => invoke('console_save', { config }),
    reset: (appId, revision) => invoke('console_reset', { appId, revision }),
    start: () => invoke('console_start'),
    stop: () => invoke('console_stop'),
    run: (appId, actionId) => invoke('console_run', { appId, actionId }),
    cancel: () => invoke('console_cancel'),
    accessibility: () => invoke('console_accessibility'),
  }
}
