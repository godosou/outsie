import { isConsoleKey, MODIFIERS, validateConsoleConfig, type ConsoleApp, type ConsoleConfig, type ConsoleModifier } from './workConsole'

export const MAX_IMPORT_BYTES = 192 * 1024
const fail = (message: string): never => { throw new Error(message) }
function object(value: unknown, allowed: string[], label: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return fail(`${label}格式不正确。`)
  const result = value as Record<string, unknown>
  const unknown = Object.keys(result).find(key => !allowed.includes(key))
  if (unknown) fail(`${label}包含不支持的字段「${unknown}」。`)
  return result
}
function text(value: unknown, max: number, label: string): string {
  if (typeof value !== 'string' || !value.trim() || [...value].length > max || /[\x00-\x1f\x7f]/.test(value)) return fail(`${label}需要 1–${max} 个字符。`)
  return value.trim()
}
function array(value: unknown, max: number, label: string): unknown[] {
  if (!Array.isArray(value) || !value.length || value.length > max) return fail(`${label}需要 1–${max} 项。`)
  return value
}
/** Portable templates deliberately contain no local executable paths or trusted IDs. */
export function parseConsoleImport(source: string, newId = () => crypto.randomUUID()): ConsoleApp[] {
  if (new TextEncoder().encode(source).length > MAX_IMPORT_BYTES) fail('配置文件不能超过 192 KiB。')
  let raw: unknown
  try { raw = JSON.parse(source) } catch { return fail('无法读取 JSON，请让 AI 输出纯 JSON 配置文件。') }
  const file = object(raw, ['schemaVersion', 'apps'], '配置文件')
  if (file.schemaVersion !== 1) fail('不支持此配置版本，请使用 schemaVersion: 1。')
  return array(file.apps, 16, 'App 列表').map((value, index) => {
    const app = object(value, ['name', 'actions'], `App ${index + 1}`)
    return {
      id: newId(), name: text(app.name, 64, 'App 名称'), bundleId: '',
      actions: array(app.actions, 96, '操作列表').map(value => {
        const action = object(value, ['name', 'icon', 'kind', 'steps'], '操作')
        if (action.kind !== 'hotkey' && action.kind !== 'sequence') fail('操作类型只能是 hotkey 或 sequence。')
        const steps = array(action.steps, 20, '步骤').map(value => {
          const step = object(value, ['key', 'modifiers', 'delayMs'], '步骤')
          const key = text(step.key, 16, '按键')
          if (!isConsoleKey(key)) fail(`不支持按键「${key}」。`)
          if (!Array.isArray(step.modifiers) || step.modifiers.length > 4 || step.modifiers.some(m => !MODIFIERS.includes(m)) || new Set(step.modifiers).size !== step.modifiers.length) fail('修饰键只能使用 meta、ctrl、alt、shift，且不能重复。')
          if (typeof step.delayMs !== 'number' || !Number.isInteger(step.delayMs) || step.delayMs < 0 || step.delayMs > 5000) fail('每步 delayMs 须为 0–5000 的整数。')
          return { key, modifiers: step.modifiers as ConsoleModifier[], delayMs: step.delayMs as number }
        })
        if (action.kind === 'hotkey' && steps.length !== 1) fail('连续按键请使用 sequence 类型。')
        return { id: newId(), name: text(action.name, 64, '操作名称'), icon: action.icon === undefined ? '⌘' : text(action.icon, 16, '图标'), kind: action.kind as 'hotkey' | 'sequence', steps }
      }),
    }
  })
}

export function appendConsoleImport(config: ConsoleConfig, apps: ConsoleApp[]): ConsoleConfig {
  if (config.apps.length + apps.length > 16) fail('导入后超过 16 个 App，请先删除不需要的 App 配置。')
  const next = { ...config, apps: [...config.apps, ...apps] }
  // Imported targets remain unbound until the user chooses the installed application.
  const validation = validateConsoleConfig({ ...next, apps: next.apps.map(app => ({ ...app, bundleId: app.bundleId || 'local.unbound' })) })
  if (validation) fail(validation)
  if (new TextEncoder().encode(JSON.stringify(next)).length > MAX_IMPORT_BYTES) fail('合并后配置超过 192 KiB，请减少操作或步骤。')
  return next
}

export const CONSOLE_AI_PROMPT = `请为 macOS 上的「填写 App 名称和版本」生成 Repose 手机快捷操作配置。
我的需求：填写想用的快捷键或按键序列，例如先执行一个快捷键，等待 2 秒，再执行另一个快捷键。
请先核实该 App 的官方快捷键，不要猜测；不同界面焦点或功能开关等前提写进按钮 name。只输出可保存为 .json 文件的纯 JSON，不要 Markdown 代码围栏。
格式严格如下，不得添加字段，不要生成 Bundle ID、App 路径、脚本、命令或内部 ID。导入后我会在 Mac 上交互选择实际 App。
{"schemaVersion":1,"apps":[{"name":"App 名称","actions":[{"name":"查找后关闭示例","icon":"⌕","kind":"sequence","steps":[{"key":"f","modifiers":["meta"],"delayMs":0},{"key":"Escape","modifiers":[],"delayMs":2000}]}]}]}
约束：1–16 个 App；每个 1–96 个操作；name 最多 64 字符；icon 最多 16 字符。kind 为 hotkey（恰好 1 步）或 sequence（1–20 步）。modifiers 为 meta(Command)、ctrl、alt(Option)、shift 的不重复数组。delayMs 是本步骤按下前的等待毫秒数，整数 0–5000。key 为单个 ASCII 可见字符，或 Enter、Tab、Space、Escape、Backspace、Delete、ArrowLeft、ArrowRight、ArrowUp、ArrowDown、Home、End、PageUp、PageDown、F1–F20。不支持鼠标、长按和仅修饰键的操作。
上面的序列只是格式示例，请按我的真实需求生成。没有官方默认快捷键的功能不要加入配置；生成文件前先说明缺项，确认需求后再输出纯 JSON。`
