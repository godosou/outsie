import assert from 'node:assert/strict'
import test from 'node:test'
import {
  panelMode, setupStep, showsSwitch, deriveUnlockView,
  UNSUPPORTED_SNAPSHOT, type UnlockState,
} from './unlock'

const ALL: UnlockState[] = [
  'not-installed', 'installing', 'half-installed', 'awaiting-password-drill',
  'awaiting-pairing', 'awaiting-calibration', 'awaiting-verification',
  'ready', 'needs-repair', 'paused', 'uninstalling',
]

test('every state belongs to exactly one mode', () => {
  for (const s of ALL) {
    const m = panelMode(s)
    assert.ok(['setup', 'daily', 'repair'].includes(m), `${s} -> ${m}`)
  }
  assert.equal(panelMode('ready'), 'daily')
  assert.equal(panelMode('needs-repair'), 'repair')
  assert.equal(panelMode('awaiting-pairing'), 'setup')
})

test('only setup is numbered, and the numbers only go forwards', () => {
  // Numbers are structure here, not decoration: setup is the one part of this
  // feature that genuinely has an order.
  for (const s of ALL) {
    const step = setupStep(s)
    if (panelMode(s) === 'setup') {
      assert.ok(step !== null && step >= 1 && step <= 3, `${s} is setup but has no step`)
    } else {
      assert.equal(step, null, `${s} is not setup but claims step ${step}`)
    }
  }
  assert.equal(setupStep('not-installed'), 1)
  assert.equal(setupStep('awaiting-pairing'), 2)
  assert.equal(setupStep('awaiting-verification'), 3)
})

test('nothing that is not installed gets a switch', () => {
  assert.equal(showsSwitch('not-installed'), false)
  assert.equal(showsSwitch('installing'), false)
  assert.equal(showsSwitch('ready'), true)
  assert.equal(showsSwitch('awaiting-pairing'), true)
})

test('every state offers at most one primary action, and it is a verb', () => {
  // The rule the prototype demonstrates: one state, one action. A state with
  // two calls to action makes the reader choose before they understand.
  for (const s of ALL) {
    const v = deriveUnlockView({ ...UNSUPPORTED_SNAPSHOT, state: s })
    if (v.primaryAction) {
      assert.ok(v.primaryAction.verb.length > 0, `${s} has an unlabelled action`)
    }
  }
})
