import assert from 'node:assert/strict'
import test from 'node:test'
import {
  normalizeUnlockSnapshot,
  deriveUnlockView,
  deriveGlobalBanner,
  healthClass,
  normalizePreflight,
  beginRequest,
  finishRequest,
  failRequest,
  canIssue,
  reduceRevocationConfirmation,
  remediationLabel,
  INITIAL_REQUEST_STATE,
  INITIAL_REVOCATION,
  type UnlockSnapshot,
  type UnlockState,
} from './unlock.ts'

function snapshot(overrides: Partial<UnlockSnapshot> = {}): UnlockSnapshot {
  return {
    readAt: '2026-09-09T14:03:22Z',
    state: 'ready',
    presence: 'near',
    variant: 'A',
    components: [
      { id: 'rule', health: 'ok', detail: '已就位' },
      { id: 'component', health: 'ok', detail: '已装入' },
      { id: 'daemon', health: 'ok', detail: '运行中' },
      { id: 'transport', health: 'ok', detail: '蓝牙已开' },
    ],
    componentInvocation: { kind: 'observed', at: '2026-09-09T14:22:00Z' },
    device: { id: 'dev1', name: 'realme RMX3888', platform: 'Android', pairedAt: '2026-09-09', lastSeenMs: 12000 },
    stats: { unlocksToday: 4, lastUnlockAt: '2026-09-09T14:22:00Z' },
    lastFailure: null,
    macosBuild: '23G93',
    componentVersion: '0.1.0',
    ...overrides,
  }
}

// ---- normalizeUnlockSnapshot: untrusted input ----------------------------

test('normalize collapses non-objects to the fallback safe state', () => {
  for (const bad of [null, undefined, 42, 'ready', []]) {
    const s = normalizeUnlockSnapshot(bad)
    assert.equal(s.state, 'needs-repair', `bad input ${JSON.stringify(bad)} should collapse`)
    assert.equal(s.componentInvocation.kind, 'never-observed')
  }
})

test('normalize rejects an unknown state and collapses', () => {
  const s = normalizeUnlockSnapshot({ state: 'totally-new-state', readAt: 'x' })
  assert.equal(s.state, 'needs-repair')
})

test('normalize honours a custom fallback (unsupported for !window.repose)', () => {
  const s = normalizeUnlockSnapshot(null, 'not-installed')
  assert.equal(s.state, 'not-installed')
})

test('normalize requires readAt; missing it collapses', () => {
  const s = normalizeUnlockSnapshot({ state: 'ready' })
  assert.equal(s.state, 'needs-repair')
})

test('normalize round-trips a valid snapshot', () => {
  const input = snapshot()
  const s = normalizeUnlockSnapshot(input)
  assert.equal(s.state, 'ready')
  assert.equal(s.presence, 'near')
  assert.equal(s.variant, 'A')
  assert.equal(s.components.length, 4)
  assert.equal(s.device?.name, 'realme RMX3888')
  assert.equal(s.stats.unlocksToday, 4)
})

test('normalize drops malformed components but keeps valid ones', () => {
  const s = normalizeUnlockSnapshot(snapshot({
    components: [
      { id: 'rule', health: 'ok', detail: 'ok' },
      { id: 'bogus', health: 'ok', detail: 'x' } as never,
      { id: 'daemon', health: 'nonsense', detail: 'x' } as never,
      { id: 'transport', health: 'broken', detail: 'off' },
    ],
  }))
  assert.deepEqual(s.components.map(c => c.id), ['rule', 'transport'])
})

test('normalize never fakes observed: observed without a timestamp becomes never-observed', () => {
  const s = normalizeUnlockSnapshot(snapshot({ componentInvocation: { kind: 'observed' } as never }))
  assert.equal(s.componentInvocation.kind, 'never-observed')
})

test('normalize coerces a negative/float unlocksToday to a safe integer', () => {
  const s = normalizeUnlockSnapshot(snapshot({ stats: { unlocksToday: -3.7 as never, lastUnlockAt: null } }))
  assert.equal(s.stats.unlocksToday, 0)
})

// ---- deriveUnlockView ----------------------------------------------------

test('ready + near + observed shows the green ready dot', () => {
  const v = deriveUnlockView(snapshot({ presence: 'near', componentInvocation: { kind: 'observed', at: 'x' } }))
  assert.equal(v.status.tone, 'ready')
  assert.equal(v.showReadyDot, true)
  assert.equal(v.primaryAction, null)
})

test('ready + near but NEVER observed must not show a green dot', () => {
  const v = deriveUnlockView(snapshot({ presence: 'near', componentInvocation: { kind: 'never-observed' } }))
  assert.equal(v.showReadyDot, false, 'no evidence => no green dot')
})

test('ready + away is neutral, not a warning, and has no primary action', () => {
  const v = deriveUnlockView(snapshot({ presence: 'away' }))
  assert.equal(v.status.tone, 'neutral')
  assert.equal(v.showReadyDot, false)
  assert.equal(v.primaryAction, null)
  assert.match(v.status.sub ?? '', /正常/)
})

test('ready + transport-unavailable offers opening bluetooth', () => {
  const v = deriveUnlockView(snapshot({ presence: 'transport-unavailable' }))
  assert.equal(v.primaryAction?.command, 'open-bluetooth-settings')
})

test('every non-ready/terminal state has exactly one primary action with a verb', () => {
  const states: UnlockState[] = [
    'not-installed', 'half-installed', 'awaiting-password-drill', 'awaiting-pairing',
    'awaiting-calibration', 'awaiting-verification', 'needs-repair', 'paused',
  ]
  for (const state of states) {
    const v = deriveUnlockView(snapshot({ state }))
    assert.ok(v.primaryAction, `${state} must have a primary action`)
    assert.ok(v.primaryAction!.verb.length > 0, `${state} verb non-empty`)
  }
})

test('needs-repair prioritises the rule layer over the component layer', () => {
  const v = deriveUnlockView(snapshot({
    state: 'needs-repair',
    components: [
      { id: 'rule', health: 'broken', detail: 'tampered' },
      { id: 'component', health: 'broken', detail: 'gone' },
    ],
  }))
  assert.equal(v.primaryAction?.command, 'repair-rule')
})

// ---- deriveGlobalBanner: deliberately rare -------------------------------

test('no banner for the everyday states (ready/away/paused)', () => {
  assert.equal(deriveGlobalBanner(snapshot({ state: 'ready', presence: 'near' })), null)
  assert.equal(deriveGlobalBanner(snapshot({ state: 'ready', presence: 'away' })), null)
  assert.equal(deriveGlobalBanner(snapshot({ state: 'paused' })), null)
  assert.equal(deriveGlobalBanner(snapshot({ state: 'not-installed' })), null)
})

test('needs-repair raises an attention banner', () => {
  const b = deriveGlobalBanner(snapshot({ state: 'needs-repair', components: [{ id: 'rule', health: 'broken', detail: 'x' }] }))
  assert.ok(b)
  assert.equal(b!.tone, 'attention')
})

// The banner for the fail-open state must describe the Mac as it is at this
// instant. The app is looking at a rule that still points at a missing
// component, so nothing has repaired anything yet -- an earlier version claimed
// the daemon had already fixed it and that everything was safe, which told the
// user to relax during the only state where the machine opens for anybody.
test('a vanished component raises a danger banner that says the Mac is open NOW', () => {
  const b = deriveGlobalBanner(snapshot({
    state: 'needs-repair',
    components: [
      { id: 'rule', health: 'broken', detail: 'dangling' },
      { id: 'component', health: 'broken', detail: 'gone' },
      { id: 'daemon', health: 'ok', detail: 'running' },
    ],
  }))
  assert.ok(b)
  assert.equal(b!.tone, 'danger')
  assert.match(b!.title, /不用密码/)
  assert.doesNotMatch(b!.body, /一切安全/)
  // It may say the daemon should act; it may not say it already has.
  assert.doesNotMatch(b!.body, /已把规则改回/)
  assert.match(b!.body, /还没有改回来/)
})

test('with the guard daemon stopped, the banner says nobody is coming to fix it', () => {
  const b = deriveGlobalBanner(snapshot({
    state: 'needs-repair',
    components: [
      { id: 'rule', health: 'broken', detail: 'dangling' },
      { id: 'component', health: 'broken', detail: 'gone' },
      { id: 'daemon', health: 'degraded', detail: 'not running' },
    ],
  }))
  assert.ok(b)
  assert.equal(b!.tone, 'danger')
  assert.match(b!.body, /没有在运行/)
})

test('awaiting-verification raises the "not tried yet" banner', () => {
  const b = deriveGlobalBanner(snapshot({ state: 'awaiting-verification' }))
  assert.ok(b)
  assert.match(b!.title, /还没试过/)
})

// ---- request reducer -----------------------------------------------------

test('request reducer guards a single in-flight call by sequence', () => {
  let s = INITIAL_REQUEST_STATE
  assert.equal(canIssue(s), true)
  s = beginRequest(s, 'install')
  assert.equal(s.pending, 'install')
  assert.equal(canIssue(s), false)
  const seq = s.sequence
  // a stale finish (older sequence) is ignored
  const stale = finishRequest(s, seq - 1)
  assert.equal(stale.pending, 'install')
  // the matching finish clears pending
  s = finishRequest(s, seq)
  assert.equal(s.pending, null)
  assert.equal(canIssue(s), true)
})

test('failRequest records the error only for the matching sequence', () => {
  let s = beginRequest(INITIAL_REQUEST_STATE, 'uninstall')
  const err = { code: 'backup-missing' as const, detail: 'no backup' }
  const stale = failRequest(s, s.sequence - 1, err)
  assert.equal(stale.error, null)
  s = failRequest(s, s.sequence, err)
  assert.equal(s.error?.code, 'backup-missing')
  assert.equal(s.pending, null)
})

// ---- revocation reducer --------------------------------------------------

test('revocation arm/cancel/confirm', () => {
  let s = INITIAL_REVOCATION
  s = reduceRevocationConfirmation(s, { type: 'arm', deviceId: 'dev1' })
  assert.equal(s.armedDeviceId, 'dev1')
  s = reduceRevocationConfirmation(s, { type: 'cancel' })
  assert.equal(s.armedDeviceId, null)
  s = reduceRevocationConfirmation({ armedDeviceId: 'dev2' }, { type: 'confirm' })
  assert.equal(s.armedDeviceId, null)
})

// ---- remediation labels: closed set, one per variant ---------------------

test('every remediation primitive has a label except leave-it-alone', () => {
  assert.equal(remediationLabel({ kind: 'reinstall-component' }), '重新安装组件')
  assert.equal(remediationLabel({ kind: 'repair-rule' }), '修复规则')
  assert.equal(remediationLabel({ kind: 're-pair' }), '重新配对')
  assert.equal(remediationLabel({ kind: 're-calibrate' }), '重做校准')
  assert.equal(remediationLabel({ kind: 'fix-on-phone', hint: 'unseen' }), '在手机上打开')
  assert.equal(remediationLabel({ kind: 'revoke-device' }), '撤销这台设备')
  assert.equal(remediationLabel({ kind: 'uninstall-and-restore' }), '移除手机钥匙并还原系统设置')
  assert.equal(remediationLabel({ kind: 'leave-it-alone' }), null)
})

// ---- health colours are actually painted --------------------------------
//
// healthClass returned 'bad' for a broken component from the first version, and
// phone-key.css had no rule for it. So the row meaning "this Mac may open with
// no password right now" was drawn in the same colour as "已就位" -- the class
// was emitted and nothing painted it, which no type checker can catch.

test('every class healthClass can emit has a rule in phone-key.css', async () => {
  const { readFileSync } = await import('node:fs')
  const { fileURLToPath } = await import('node:url')
  const css = readFileSync(
    fileURLToPath(new URL('../phone-key.css', import.meta.url)), 'utf8')
  for (const health of ['ok', 'degraded', 'broken', 'unknown']) {
    const cls = healthClass(health)
    if (!cls) continue // 'unknown' deliberately inherits the default colour
    assert.ok(
      css.includes(`dd.${cls}`),
      `healthClass('${health}') returns '${cls}' but phone-key.css never styles dd.${cls}`,
    )
  }
})

test('broken and ok do not share a class', () => {
  assert.notEqual(healthClass('broken'), healthClass('ok'))
  assert.notEqual(healthClass('degraded'), healthClass('ok'))
})

// ---- preflight -----------------------------------------------------------

test('preflight normalizes to "cannot install" for anything odd', () => {
  for (const bad of [null, undefined, 42, 'ok', []]) {
    assert.equal(normalizePreflight(bad).canInstall, false)
  }
})

test('a third-party mechanism survives normalization so the sheet can show it', () => {
  const p = normalizePreflight({
    variant: 'A', canInstall: true, ruleNow: '[]',
    foreign: ['com.openai.sky.CUAService.AuthorizationPlugin.remote'],
  })
  assert.equal(p.canInstall, true)
  assert.deepEqual(p.foreign, ['com.openai.sky.CUAService.AuthorizationPlugin.remote'])
})

test('a foreign list that is not strings, or absurdly long, cannot wedge the sheet', () => {
  assert.deepEqual(normalizePreflight({ foreign: [1, 2, 3] }).foreign, [])
  assert.equal(normalizePreflight({ foreign: Array(500).fill('x') }).foreign.length, 12)
})
