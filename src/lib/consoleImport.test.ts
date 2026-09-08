import test from 'node:test'
import assert from 'node:assert/strict'
import presets from './codexPresets.json'
import { parseConsoleImport, appendConsoleImport, MAX_IMPORT_BYTES } from './consoleImport'
import { validateConsoleConfig, type ConsoleAction } from './workConsole'
const template = () => ({ schemaVersion: 1, apps: [{ name: 'Safari', actions: [{ name: '查找后关闭', icon: '⌕', kind: 'sequence', steps: [{ key: 'f', modifiers: ['meta'], delayMs: 0 }, { key: 'Escape', modifiers: [], delayMs: 2000 }] }] }] })
const parse = (value: unknown) => parseConsoleImport(JSON.stringify(value))
test('AI import preserves keyboard order and waits, generates IDs and requires interactive App selection', () => {
  const a = parse(template()), b = parse(template())
  assert.notEqual(a[0].id, b[0].id)
  assert.notEqual(a[0].actions[0].id, b[0].actions[0].id)
  assert.equal(a[0].bundleId, '')
  assert.equal(a[0].appPath, undefined)
  assert.deepEqual(a[0].actions[0].steps, template().apps[0].actions[0].steps)
  assert.match(validateConsoleConfig({ revision: 0, apps: a })!, /选择本机 App/)
  a[0].bundleId = 'com.apple.Safari'
  assert.equal(validateConsoleConfig({ revision: 0, apps: a }), null)
})
test('import rejects malformed, executable, unsupported or out of bounds input atomically', () => {
  const mutate = (fn: (file: any) => void) => { const file = template(); fn(file); assert.throws(() => parse(file)) }
  assert.throws(() => parseConsoleImport('```json\n{}\n```'))
  mutate(f => f.schemaVersion = 2)
  mutate(f => f.apps[0].appPath = '/Applications/Other.app')
  mutate(f => f.apps[0].bundleId = 'com.apple.Safari')
  mutate(f => f.apps[0].actions[0].command = 'open something')
  mutate(f => f.apps[0].actions[0].kind = 'script')
  mutate(f => f.apps[0].actions[0].steps[0].key = 'Fn')
  mutate(f => f.apps[0].actions[0].steps[0].modifiers = ['meta', 'meta'])
  mutate(f => f.apps[0].actions[0].steps[0].delayMs = 5001)
  mutate(f => f.apps[0].actions[0].steps[0].delayMs = 1.5)
  mutate(f => f.apps[0].actions[0].steps[0].delayMs = '2000')
  mutate(f => f.apps[0].actions[0].kind = 'hotkey')
  mutate(f => f.apps.push(null))
  mutate(f => f.apps[0].actions[0].steps = [])
  assert.throws(() => parseConsoleImport(' '.repeat(MAX_IMPORT_BYTES + 1)))
})
test('import appends without changing saved profiles, rejects combined capacity overflow', () => {
  const apps = parse(template()); apps[0].bundleId = 'com.apple.Safari'
  const config = { revision: 12, apps }, snapshot = JSON.stringify(config)
  const next = appendConsoleImport(config, parse(template()))
  assert.equal(JSON.stringify(config), snapshot)
  assert.equal(next.apps.length, 2)
  assert.equal(next.revision, 12)
  assert.equal(next.apps[0], config.apps[0])
  assert.throws(() => appendConsoleImport(config, Array.from({ length: 16 }, () => parse(template())[0])))
})
test('all 77 Codex presets validate, including terminal, voice, review and numeric task navigation', () => {
  const actions = presets as ConsoleAction[]
  assert.equal(actions.length, 77)
  assert.equal(validateConsoleConfig({ revision: 0, apps: [{ id: 'codex', name: 'Codex', bundleId: 'com.openai.codex', actions }] }), null)
  const step = (id: string) => actions.find(action => action.id === id)!.steps[0]
  assert.deepEqual(step('terminal'), { key: '`', modifiers: ['ctrl'], delayMs: 0 })
  assert.deepEqual(step('dictation').modifiers, ['ctrl', 'shift'])
  assert.equal(step('review-panel').key, 'b')
  assert.equal(step('task-9').key, '9')
  assert.equal(step('recent-6').key, '6')
})
