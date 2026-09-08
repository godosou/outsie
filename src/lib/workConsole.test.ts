import assert from 'node:assert/strict'
import test from 'node:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { act, create, type ReactTestInstance, type ReactTestRenderer } from 'react-test-renderer'
import { WorkConsolePanel } from '../components/WorkConsolePanel.tsx'
import { createConsoleBridge, isConsoleKey, formatConsoleSequence, formatConsoleStep, moveConsoleStep, recordConsoleKey, validateConsoleConfig, type ConsoleConfig, type ConsoleStatus, type WorkConsoleBridge } from './workConsole.ts'

const config: ConsoleConfig = { revision: 2, apps: [{ id: 'tmux', name: 'tmux', bundleId: 'com.apple.Terminal', actions: [{ id: 'split', name: '分屏', icon: '◫', kind: 'sequence', steps: [{ key: 'b', modifiers: ['ctrl'], delayMs: 0 }, { key: '5', modifiers: ['shift'], delayMs: 120 }] }] }] }
const keyEvent = (overrides: Partial<KeyboardEvent> = {}) => ({ key: 'k', code: 'KeyK', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, repeat: false, isComposing: false, ...overrides })

test('recording preserves printable key and modifiers for tmux percent split and Option keys', () => {
  assert.deepEqual(recordConsoleKey(keyEvent({ key: '%', code: 'Digit5', shiftKey: true }), 121.4), { key: '%', modifiers: ['shift'], delayMs: 121 })
  assert.deepEqual(recordConsoleKey(keyEvent({ key: '˚', code: 'KeyK', altKey: true }), 0), { key: 'k', modifiers: ['alt'], delayMs: 0 })
  assert.equal(formatConsoleStep({ key: 'k', modifiers: ['shift', 'meta'], delayMs: 0 }), '⌘⇧K')
})

test('recording excludes escape, composing, repeat and modifier-only events and bounds waits', () => {
  for (const event of [keyEvent({ key: 'Escape', code: 'Escape' }), keyEvent({ repeat: true }), keyEvent({ isComposing: true }), keyEvent({ key: 'Control', code: 'ControlLeft', ctrlKey: true })]) assert.equal(recordConsoleKey(event, 10), null)
  assert.equal(recordConsoleKey(keyEvent(), 9000)?.delayMs, 5000)
  assert.equal(recordConsoleKey(keyEvent(), -40)?.delayMs, 0)
})

test('configuration rejects invalid actions before save and keeps ordered sequence immutable', () => {
  assert.equal(validateConsoleConfig(config), null)
  const clone = structuredClone(config)
  clone.apps[0].actions[0].kind = 'hotkey'
  assert.match(validateConsoleConfig(clone)!, /快捷键需要 1 步/)
  clone.apps[0].actions[0].kind = 'sequence'
  clone.apps[0].actions[0].steps[0].delayMs = 5001
  assert.match(validateConsoleConfig(clone)!, /0–5 秒/)
  const steps = config.apps[0].actions[0].steps
  assert.deepEqual(moveConsoleStep(steps, 0, 1), [steps[1], steps[0]])
  assert.equal(steps[0].key, 'b')
})

test('typed bridge sends identifiers, revision and structured config without execution fallback', async () => {
  const calls: unknown[] = []
  const bridge = createConsoleBridge(async <T>(command: string, args?: Record<string, unknown>): Promise<T> => { calls.push({ command, args }); return {} as T })
  await bridge.save(config); await bridge.run('tmux', 'split'); await bridge.reset('tmux', 2)
  assert.deepEqual(calls, [{ command: 'console_save', args: { config } }, { command: 'console_run', args: { appId: 'tmux', actionId: 'split' } }, { command: 'console_reset', args: { appId: 'tmux', revision: 2 } }])
  const failing = createConsoleBridge(async () => { throw new Error('permission denied') })
  await assert.rejects(failing.run('tmux', 'split'), /permission denied/)
})

test('browser UI explicitly reports native controls unavailable', () => {
  const html = renderToStaticMarkup(createElement(WorkConsolePanel))
  assert.match(html, /网页版不连接手机/)
  assert.doesNotMatch(html, /开启并生成二维码/)
})

const status: ConsoleStatus = { config, enabled: false, connected: false, running: false, activeAppId: null, lastError: null, accessibility: true, blocked: false }
const textOf = (node: ReactTestInstance): string => node.children.map(child => typeof child === 'string' ? child : textOf(child)).join('')
function button(root: ReactTestInstance, text: string) { return root.findAllByType('button').find(node => textOf(node) === text)! }

async function mountTest(run: (renderer: ReactTestRenderer, context: { tick(): void; changeStatus(next: ConsoleStatus): void; deferStatus(): (next: ConsoleStatus) => void; blurWindow(): void; saved: ConsoleConfig[]; calls: string[] }) => Promise<void>) {
  const oldWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')
  const oldDocument = Object.getOwnPropertyDescriptor(globalThis, 'document')
  let tick = () => {}
  let current = structuredClone(status)
  let pendingStatus: Promise<ConsoleStatus> | null = null
  const windowListeners = new Map<string, () => void>()
  const saved: ConsoleConfig[] = []
  const calls: string[] = []
  Object.defineProperty(globalThis, 'window', { configurable: true, value: { addEventListener: (name: string, callback: () => void) => { windowListeners.set(name, callback) }, removeEventListener: (name: string) => { windowListeners.delete(name) }, setInterval: (callback: () => void) => { tick = callback; return 1 }, clearInterval: () => {} } })
  Object.defineProperty(globalThis, 'document', { configurable: true, value: { addEventListener: () => {}, removeEventListener: () => {} } })
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true
  const bridge: WorkConsoleBridge = {
    status: async () => { const next = pendingStatus; pendingStatus = null; return next ?? current },
    save: async draft => { saved.push(draft); current = { ...current, config: { ...draft, revision: current.config.revision + 1 } }; return current },
    reset: async () => current, start: async () => ({ qrPayload: 'repose://console/v1/test', status: { ...current, enabled: true } }),
    stop: async () => current, run: async () => { calls.push('run'); return current }, cancel: async () => current, accessibility: async () => current,
  }
  let renderer!: ReactTestRenderer
  try {
    await act(async () => { renderer = create(createElement(WorkConsolePanel, { bridge })) })
    await run(renderer, { tick: () => tick(), changeStatus: next => { current = next }, deferStatus: () => { let resolve!: (value: ConsoleStatus) => void; pendingStatus = new Promise<ConsoleStatus>(done => { resolve = done }); return resolve }, blurWindow: () => windowListeners.get('blur')?.(), saved, calls })
  } finally {
    if (renderer) await act(async () => renderer.unmount())
    if (oldWindow) Object.defineProperty(globalThis, 'window', oldWindow)
    else delete (globalThis as { window?: unknown }).window
    if (oldDocument) Object.defineProperty(globalThis, 'document', oldDocument)
    else delete (globalThis as { document?: unknown }).document
  }
}

test('editing stays local until saved, disables trial, and cancel restores server state', async () => {
  await mountTest(async (renderer, context) => {
    const root = renderer.root
    const nameInput = () => root.findAllByType('input').find(node => node.props.value === '分屏')!
    await act(async () => nameInput().props.onChange({ target: { value: '新的分屏' } }))
    assert.equal(context.saved.length, 0)
    assert.equal(button(root, '在 Mac 试运行').props.disabled, true)
    await act(async () => button(root, '取消编辑').props.onClick())
    assert.ok(nameInput())
    await act(async () => nameInput().props.onChange({ target: { value: '保存的分屏' } }))
    await act(async () => button(root, '保存配置').props.onClick())
    assert.equal(context.saved[0].apps[0].actions[0].name, '保存的分屏')
    assert.equal(context.saved[0].revision, 2)
    assert.equal(button(root, '在 Mac 试运行').props.disabled, false)
  })
})

test('phone revision changes preserve draft and block overwriting a newer layout', async () => {
  await mountTest(async (renderer, context) => {
    const root = renderer.root
    await act(async () => root.findAllByType('input').find(node => node.props.value === '分屏')!.props.onChange({ target: { value: '本地草稿' } }))
    context.changeStatus({ ...status, config: { ...config, revision: 3 } })
    await act(async () => context.tick())
    assert.ok(root.findAllByType('input').find(node => node.props.value === '本地草稿'))
    assert.equal(button(root, '保存配置').props.disabled, true)
    assert.match(textOf(root), /此草稿基于旧版本/)
    await act(async () => button(root, '取消编辑').props.onClick())
    assert.match(textOf(root), /版本 3/)
  })
})

test('recording replaces sequence in focused capture and Escape ends without becoming a step', async () => {
  await mountTest(async (renderer, context) => {
    const root = renderer.root
    await act(async () => button(root, '开始录制').props.onClick())
    const capture = () => root.findByProps({ 'aria-label': '键盘录制区域' })
    const press = (event: ReturnType<typeof keyEvent>) => capture().props.onKeyDown({ key: event.key, nativeEvent: event, preventDefault() {}, stopPropagation() {} })
    await act(async () => press(keyEvent({ ctrlKey: true })))
    await act(async () => press(keyEvent({ key: 'Enter', code: 'Enter' })))
    await act(async () => press(keyEvent({ key: 'Escape', code: 'Escape' })))
    assert.ok(button(root, '开始录制'))
    await act(async () => button(root, '保存配置').props.onClick())
    assert.deepEqual(context.saved[0].apps[0].actions[0].steps.map(step => step.key), ['k', 'Enter'])
  })
})


test('backend tmux punctuation presets and Unicode icon limits remain valid', () => {
  for (let code = 33; code <= 126; code += 1) assert.equal(isConsoleKey(String.fromCharCode(code)), true)
  const presets = structuredClone(config)
  presets.apps[0].actions[0].steps = [{ key: 'b', modifiers: ['ctrl'], delayMs: 0 }, { key: '%', modifiers: [], delayMs: 100 }, { key: '"', modifiers: [], delayMs: 100 }]
  presets.apps[0].actions[0].icon = '📱'.repeat(16)
  assert.equal(validateConsoleConfig(presets), null)
  presets.apps[0].actions[0].icon += '📱'
  assert.match(validateConsoleConfig(presets)!, /图标/)
  assert.equal(recordConsoleKey(keyEvent({ key: 'Dead', code: 'Quote' }), 0), null)
})

test('a poll started before a completed command cannot restore obsolete runtime state', async () => {
  await mountTest(async (renderer, context) => {
    const root = renderer.root
    const resolve = context.deferStatus()
    await act(async () => context.tick())
    await act(async () => root.findByProps({ placeholder: '例如 192.168.1.20' }).props.onChange({ target: { value: '192.168.1.20' } }))
    await act(async () => button(root, '开启并生成二维码').props.onClick())
    assert.ok(button(root, '关闭手机控制'))
    await act(async () => resolve(status))
    assert.ok(button(root, '关闭手机控制'))
  })
})

test('window focus loss ends recording and server execution errors appear on next status poll', async () => {
  await mountTest(async (renderer, context) => {
    const root = renderer.root
    await act(async () => button(root, '开始录制').props.onClick())
    await act(async () => context.blurWindow())
    assert.ok(button(root, '开始录制'))
    context.changeStatus({ ...status, lastError: '目标 App 已切换，剩余步骤已停止' })
    await act(async () => context.tick())
    assert.match(textOf(root), /目标 App 已切换，剩余步骤已停止/)
  })
})


test('sequence wait edits use seconds while saving milliseconds and preserve every key', async () => {
  await mountTest(async (renderer, context) => {
    const root = renderer.root
    const wait = root.findByProps({ 'aria-label': '步骤 2 等待秒数' })
    assert.equal(wait.props.max, 5)
    assert.equal(wait.props.step, 0.1)
    assert.equal(wait.props.value, 0.12)
    await act(async () => wait.props.onChange({ target: { value: '2' } }))
    assert.equal(textOf(root.findByProps({ 'aria-label': '执行顺序' })), '⌃B → 等待2秒 → ⇧5')
    await act(async () => button(root, '添加步骤').props.onClick())
    await act(async () => root.findByProps({ 'aria-label': '步骤 3 等待秒数' }).props.onChange({ target: { value: '1.5' } }))
    await act(async () => button(root, '保存配置').props.onClick())
    assert.deepEqual(context.saved[0].apps[0].actions[0].steps, [
      { key: 'b', modifiers: ['ctrl'], delayMs: 0 },
      { key: '5', modifiers: ['shift'], delayMs: 2000 },
      { key: 'Enter', modifiers: [], delayMs: 1500 },
    ])
    assert.equal(formatConsoleSequence(context.saved[0].apps[0].actions[0].steps), '⌃B → 等待2秒 → ⇧5 → 等待1.5秒 → Enter')
  })
})

test('empty or invalid seconds cannot silently save as zero delay', async () => {
  await mountTest(async (renderer) => {
    const root = renderer.root
    for (const value of ['', 'invalid', '6']) {
      await act(async () => root.findByProps({ 'aria-label': '步骤 2 等待秒数' }).props.onChange({ target: { value } }))
      assert.equal(button(root, '保存配置').props.disabled, true)
      assert.match(textOf(root), /等待时间须为 0–5 秒/)
    }
    await act(async () => root.findByProps({ 'aria-label': '步骤 2 等待秒数' }).props.onChange({ target: { value: '0' } }))
    assert.equal(button(root, '保存配置').props.disabled, false)
  })
})
