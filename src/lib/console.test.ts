import { test } from 'node:test'
import assert from 'node:assert/strict'
import { normalizeConsoleStatus, stepLabel, actionHealth } from './console.ts'

test('the desktop\'s own config shape survives normalization', () => {
  const status = normalizeConsoleStatus({
    trusted: true,
    config: {
      revision: 3,
      apps: [{
        id: 'tmux', name: 'tmux', bundleId: 'com.apple.Terminal',
        appPath: '/System/Applications/Utilities/Terminal.app',
        actions: [{
          id: 'split-horizontal', name: '左右分屏', icon: '◫', kind: 'sequence',
          steps: [
            { key: 'b', modifiers: ['ctrl'], delayMs: 0 },
            { key: '%', modifiers: [], delayMs: 100 },
          ],
        }],
      }],
    },
  })
  assert.equal(status.trusted, true)
  assert.equal(status.apps.length, 1)
  assert.equal(status.apps[0].actions[0].steps.length, 2)
  assert.equal(stepLabel(status.apps[0].actions[0].steps[0]), '⌃b')
})

test('nothing at all reads as nothing configured, and as not trusted', () => {
  // The safe direction in both fields: claiming trust we do not have would draw
  // enabled buttons that silently do nothing.
  for (const raw of [null, undefined, 'nope', 42, {}, { config: 'x' }]) {
    const s = normalizeConsoleStatus(raw)
    assert.equal(s.trusted, false)
    assert.deepEqual(s.apps, [])
  }
})

test('an app with no id is dropped rather than listed with dead buttons', () => {
  // console_run finds the app by id; a row without one is a row whose every
  // button fails.
  const s = normalizeConsoleStatus({
    config: { apps: [{ name: '没有 id', bundleId: 'com.x' }, { id: 'a', name: 'A', bundleId: 'com.a' }] },
  })
  assert.deepEqual(s.apps.map(a => a.id), ['a'])
})

test('a malformed step does not take the whole action down with it', () => {
  const s = normalizeConsoleStatus({
    config: { apps: [{ id: 'a', name: 'A', bundleId: 'com.a', actions: [
      { id: 'x', name: 'X', steps: ['not an object', { key: 'k', modifiers: ['cmd'] }] },
    ] }] },
  })
  assert.equal(s.apps[0].actions[0].steps.length, 1)
  assert.equal(stepLabel(s.apps[0].actions[0].steps[0]), '⌘k')
})

test('health matches what console.rs decides, so the two never disagree', () => {
  assert.equal(actionHealth({ id: 'a', name: 'a', icon: null, steps: [] }), 'empty')
  assert.equal(
    actionHealth({ id: 'a', name: 'a', icon: null, steps: [{ key: ' ', modifiers: [], delayMs: 0 }] }),
    'missing-key',
  )
  assert.equal(
    actionHealth({ id: 'a', name: 'a', icon: null, steps: [{ key: 'b', modifiers: ['ctrl'], delayMs: 0 }] }),
    'ok',
  )
})

test('an unknown modifier is shown as itself rather than dropped', () => {
  // Silently omitting it would show a shortcut that is not the one that will be
  // pressed — the reader would then blame the app for pressing the wrong keys.
  assert.equal(stepLabel({ key: 'k', modifiers: ['hyper'], delayMs: 0 }), 'hyperk')
})
