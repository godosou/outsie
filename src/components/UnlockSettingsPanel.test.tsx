import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { act, create, type ReactTestRenderer } from 'react-test-renderer'
import { UnlockSettingsPanel, UnlockSettingsView } from './UnlockSettingsPanel.tsx'
import { CLOSED_UNLOCK_SNAPSHOT, deriveUnlockView, type UnlockSnapshot } from '../lib/unlock.ts'
import type { UnlockDesktopBridge } from '../tauriBridge.ts'

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true

const readySnapshot: UnlockSnapshot = {
  schemaVersion: 1,
  capability: 'ready',
  authorizationGate: { state: 'passed', evidenceRecord: 'reviewed-gate-record', installAllowed: true },
  components: { policy: 'ready', plugin: 'ready', service: 'ready', transport: 'ready' },
  devices: [{ id: 'phone_1', displayName: 'realme GT5 Pro', platform: 'android' }],
  pendingPairing: null,
  calibration: { deviceId: 'phone_1', phase: 'idle' },
  limitations: ['relayRisk', 'passwordFallback'],
}

const waitingPairingSnapshot: UnlockSnapshot = {
  ...readySnapshot,
  devices: [],
  pendingPairing: {
    sessionId: 'session_polling',
    candidateName: '等待手机连接',
    qrPayload: 'repose://pair/polling-test-payload',
    expiresAtEpochMs: 1_900_000_000_000,
  },
}

function installTimerWindow() {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'window')
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      setTimeout: () => 91,
      clearTimeout: () => {},
    },
  })
  return () => {
    if (previous) Object.defineProperty(globalThis, 'window', previous)
    else delete (globalThis as { window?: unknown }).window
  }
}

function createPollingScheduler() {
  let callback: (() => void) | null = null
  let delay: number | null = null
  const cleared: number[] = []
  return {
    scheduler: {
      setInterval(next: () => void, milliseconds: number) {
        callback = next
        delay = milliseconds
        return 73
      },
      clearInterval(timerId: number) { cleared.push(timerId) },
    },
    tick() {
      assert.ok(callback, 'polling callback should be registered')
      callback()
    },
    get delay() { return delay },
    get cleared() { return cleared },
  }
}

function bridgeWithStatus(unlockStatus: () => Promise<unknown>): UnlockDesktopBridge {
  const unavailable = async (): Promise<never> => { throw { code: 'backendUnavailable' } }
  return {
    unlockStatus,
    beginPairing: unavailable,
    confirmPairing: unavailable,
    beginCalibration: unavailable,
    revokeDevice: unavailable,
    openUnlockDiagnostics: unavailable,
  }
}

test('closed-gate settings render a disabled install control with no handler', () => {
  const html = renderToStaticMarkup(
    <UnlockSettingsView
      model={deriveUnlockView(CLOSED_UNLOCK_SNAPSHOT, 1_800_000_000_000)}
      busy={false}
      diagnostics={null}
      errorMessage={null}
      pendingRevocationDeviceId={null}
      onBeginPairing={() => { throw new Error('must stay disabled') }}
      onConfirmPairing={() => { throw new Error('must stay disabled') }}
      onBeginCalibration={() => { throw new Error('must stay disabled') }}
      onRequestRevokeDevice={() => { throw new Error('must stay disabled') }}
      onCancelRevokeDevice={() => {}}
      onConfirmRevokeDevice={() => { throw new Error('must stay disabled') }}
      onOpenDiagnostics={() => {}}
      pairingSecondsRemaining={null}
    />,
  )

  assert.match(html, /data-testid="unlock-install"[^>]*disabled/)
  assert.doesNotMatch(html, /data-testid="unlock-install"[^>]*onClick/)
  assert.match(html, /系统密码/)
  assert.match(html, /中继/)
  assert.match(html, /手机被盗/)
  assert.match(html, /蓝牙关闭/)
  assert.match(html, /强制停止/)
  assert.match(html, /BLE 调试/)
  assert.match(html, /系统授权未启用/)
  assert.doesNotMatch(html, /安全预览|安装尚不可用/)
})

test('the Repose sidebar exposes a standalone phone-key page through the narrow bridge', () => {
  const appSource = readFileSync(new URL('../App.tsx', import.meta.url), 'utf8')
  assert.match(appSource, /import \{ UnlockSettingsPanel \} from '\.\/components\/UnlockSettingsPanel'/)
  assert.match(appSource, /type Page = [^\n]*'phoneKey'/)
  assert.match(appSource, /\{ id: 'phoneKey', label: '手机钥匙', icon: KeyRound \}/)
  assert.match(appSource, /phoneKey: \{ title: '手机钥匙与 BLE 调试。'/)
  assert.match(appSource, /page === 'phoneKey'[^\n]*<UnlockSettingsPanel bridge=\{window\.repose\?\.unlock\} \/>/)
  assert.equal(appSource.match(/<UnlockSettingsPanel bridge=\{window\.repose\?\.unlock\} \/>/g)?.length, 1)
})

test('pending pairing renders a scannable QR with countdown and no text-copy fallback', () => {
  const payload = 'repose://pair/visible-debug-payload'
  const pairingSnapshot: UnlockSnapshot = {
    ...readySnapshot,
    pendingPairing: {
      sessionId: 'session_1',
      candidateName: 'realme GT5 Pro',
      qrPayload: payload,
      expiresAtEpochMs: 1_900_000_000_000,
    },
  }
  const props = {
    model: deriveUnlockView(pairingSnapshot, 1_800_000_000_000),
    busy: false,
    diagnostics: null,
    errorMessage: null,
    pendingRevocationDeviceId: null,
    onBeginPairing: () => {},
    onConfirmPairing: () => {},
    onBeginCalibration: () => {},
    onRequestRevokeDevice: () => {},
    onCancelRevokeDevice: () => {},
    onConfirmRevokeDevice: () => {},
    onOpenDiagnostics: () => {},
    pairingSecondsRemaining: 100,
  }
  const html = renderToStaticMarkup(<UnlockSettingsView {...props} />)

  assert.match(html, /data-testid="pairing-qr-code"/)
  assert.match(html, /role="img"[^>]*aria-label="[^"]*扫描[^"]*"/)
  assert.match(html, /<svg[^>]*viewBox=/)
  assert.match(html, /01:40/)
  assert.match(html, /role="timer"[^>]*aria-label="二维码有效期还剩 01:40"/)
  assert.match(html, /连接状态每秒自动刷新/)
  assert.match(html, /打开 Repose 手机 App/)
  assert.doesNotMatch(html, /copy-pairing-payload|复制配对码|只读配对码|<textarea/)
  assert.doesNotMatch(html, new RegExp(payload.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')))

  const alternateHtml = renderToStaticMarkup(<UnlockSettingsView {...{
    ...props,
    model: deriveUnlockView({
      ...pairingSnapshot,
      pendingPairing: { ...pairingSnapshot.pendingPairing!, qrPayload: `${payload}-different` },
    }, 1_800_000_000_000),
  }} />)
  const qrMarkup = (value: string) => value.match(/<figure[^>]*data-testid="pairing-qr-code"[\s\S]*?<\/figure>/)?.[0]
  assert.notEqual(qrMarkup(html), qrMarkup(alternateHtml))
})

test('desktop pairing QR owns a full flex row with a legible status column', () => {
  const stylesheet = readFileSync(new URL('../styles.css', import.meta.url), 'utf8')
  const html = renderToStaticMarkup(<UnlockSettingsView
    model={deriveUnlockView(waitingPairingSnapshot, 1_800_000_000_000)}
    busy={false}
    diagnostics={null}
    errorMessage={null}
    pendingRevocationDeviceId={null}
    onBeginPairing={() => {}}
    onConfirmPairing={() => {}}
    onBeginCalibration={() => {}}
    onRequestRevokeDevice={() => {}}
    onCancelRevokeDevice={() => {}}
    onConfirmRevokeDevice={() => {}}
    onOpenDiagnostics={() => {}}
    pairingSecondsRemaining={100}
  />)

  assert.match(html, /<article class="unlock-session">[\s\S]*?<div class="unlock-session-heading"[\s\S]*?<div class="unlock-qr-layout"[\s\S]*?<div class="unlock-qr-status">/)
  assert.match(stylesheet, /\.unlock-session>\.unlock-session-heading\{flex:1 0 100%\}/)
  assert.match(stylesheet, /\.unlock-session>\.unlock-qr-layout\{[^}]*grid-template-columns:minmax\(220px,286px\) minmax\(180px,1fr\)[^}]*flex:1 0 100%/)
  assert.match(stylesheet, /\.unlock-qr-status\{min-width:180px/)
  assert.match(stylesheet, /\.unlock-session>\.unlock-qr-layout\{grid-template-columns:1fr;gap:14px;padding:14px\}/)
  assert.match(stylesheet, /\.unlock-qr-status\{min-width:0;text-align:center/)
  assert.doesNotMatch(stylesheet, /\.unlock-session>div\{flex:1\}/)
})

test('expired pairing hides the stale QR and announces automatic refresh', () => {
  const expiredSnapshot: UnlockSnapshot = {
    ...waitingPairingSnapshot,
    pendingPairing: {
      ...waitingPairingSnapshot.pendingPairing!,
      expiresAtEpochMs: 1_700_000_000_000,
    },
  }
  const html = renderToStaticMarkup(<UnlockSettingsView {...{
    model: deriveUnlockView(expiredSnapshot, 1_800_000_000_000),
    busy: false,
    diagnostics: null,
    errorMessage: null,
    pendingRevocationDeviceId: null,
    onBeginPairing: () => {},
    onConfirmPairing: () => {},
    onBeginCalibration: () => {},
    onRequestRevokeDevice: () => {},
    onCancelRevokeDevice: () => {},
    onConfirmRevokeDevice: () => {},
    onOpenDiagnostics: () => {},
    pairingSecondsRemaining: 0,
  }} />)

  assert.match(html, /二维码已过期，正在自动刷新配对状态/)
  assert.doesNotMatch(html, /data-testid="pairing-qr-code"/)
})

test('revocation confirmation names the device and exposes separate cancel and confirm controls', () => {
  const html = renderToStaticMarkup(
    <UnlockSettingsView
      model={deriveUnlockView(readySnapshot, 1_800_000_000_000)}
      busy={false}
      diagnostics={null}
      errorMessage={null}
      pendingRevocationDeviceId="phone_1"
      onBeginPairing={() => {}}
      onConfirmPairing={() => {}}
      onBeginCalibration={() => {}}
      onRequestRevokeDevice={() => {}}
      onCancelRevokeDevice={() => {}}
      onConfirmRevokeDevice={() => {}}
      onOpenDiagnostics={() => {}}
      pairingSecondsRemaining={null}
    />,
  )

  assert.match(html, /确认撤销 realme GT5 Pro/)
  assert.match(html, /data-testid="unlock-revoke-cancel"/)
  assert.match(html, /data-testid="unlock-revoke-confirm"/)
})

test('mounted panel invokes one exact revocation while the first confirmation is pending', async () => {
  const polling = createPollingScheduler()
  const revokeCalls: string[] = []
  let resolveRevocation: (snapshot: unknown) => void = () => {}
  const pendingRevocation = new Promise<unknown>(resolve => { resolveRevocation = resolve })
  const unavailable = async (): Promise<never> => { throw { code: 'backendUnavailable' } }
  const bridge: UnlockDesktopBridge = {
    unlockStatus: async () => readySnapshot,
    beginPairing: unavailable,
    confirmPairing: unavailable,
    beginCalibration: unavailable,
    revokeDevice: deviceId => {
      revokeCalls.push(deviceId)
      return pendingRevocation
    },
    openUnlockDiagnostics: unavailable,
  }
  let renderer: ReactTestRenderer | null = null

  await act(async () => {
    renderer = create(<UnlockSettingsPanel bridge={bridge} pairingPollScheduler={polling.scheduler} />)
    await Promise.resolve()
  })
  const mounted = renderer as unknown as ReactTestRenderer

  await act(async () => {
    mounted.root.findByProps({ className: 'text-button unlock-revoke' }).props.onClick()
  })
  await act(async () => {
    mounted.root.findByProps({ 'data-testid': 'unlock-revoke-cancel' }).props.onClick()
  })
  assert.deepEqual(revokeCalls, [])

  await act(async () => {
    mounted.root.findByProps({ className: 'text-button unlock-revoke' }).props.onClick()
  })
  const confirm = mounted.root.findByProps({ 'data-testid': 'unlock-revoke-confirm' })
  await act(async () => {
    confirm.props.onClick()
    confirm.props.onClick()
    await Promise.resolve()
  })
  assert.deepEqual(revokeCalls, ['phone_1'])

  await act(async () => {
    resolveRevocation(readySnapshot)
    await pendingRevocation
  })
  await act(async () => { mounted.unmount() })
})

for (const initialStatus of ['unavailable', 'failed'] as const) {
  test(`idle panel recovers from ${initialStatus} status without first creating a pairing session`, async () => {
    const polling = createPollingScheduler()
    let statusCalls = 0
    const bridge = bridgeWithStatus(async () => {
      statusCalls += 1
      if (statusCalls > 1) return readySnapshot
      if (initialStatus === 'failed') throw { code: 'backendUnavailable' }
      return {
        ...readySnapshot,
        capability: 'serviceUnavailable',
        components: { ...readySnapshot.components, transport: 'unavailable' },
      }
    })
    let renderer: ReactTestRenderer | null = null
    await act(async () => {
      renderer = create(<UnlockSettingsPanel bridge={bridge} pairingPollScheduler={polling.scheduler} />)
    })
    const mounted = renderer as unknown as ReactTestRenderer
    const beginPairing = () => mounted.root.findAllByType('button').find(button => button.props.children === '开始配对')!
    assert.equal(beginPairing().props.disabled, true)

    await act(async () => { polling.tick() })

    assert.equal(statusCalls, 2)
    assert.equal(beginPairing().props.disabled, false)
    assert.equal(mounted.root.findAllByProps({ role: 'alert' }).length, 0)
    await act(async () => { mounted.unmount() })
  })
}

test('background status refresh keeps controls responsive and cannot overwrite a newer pairing action', async t => {
  t.after(installTimerWindow())
  const polling = createPollingScheduler()
  let resolveStatus: (snapshot: unknown) => void = () => {}
  const pendingStatus = new Promise<unknown>(resolve => { resolveStatus = resolve })
  let statusCalls = 0
  let pairingCalls = 0
  const bridge = {
    ...bridgeWithStatus(() => ++statusCalls === 1 ? Promise.resolve(readySnapshot) : pendingStatus),
    beginPairing: async () => {
      pairingCalls += 1
      return waitingPairingSnapshot
    },
  }
  let renderer: ReactTestRenderer | null = null
  await act(async () => {
    renderer = create(<UnlockSettingsPanel bridge={bridge} pairingPollScheduler={polling.scheduler} />)
  })
  const mounted = renderer as unknown as ReactTestRenderer
  act(() => { polling.tick() })
  const beginPairing = mounted.root.findAllByType('button').find(button => button.props.children === '开始配对')!
  assert.equal(beginPairing.props.disabled, false, 'background refresh must not toggle busy')

  await act(async () => { beginPairing.props.onClick() })
  assert.equal(pairingCalls, 1, 'a foreground action takes priority over a slow status refresh')
  assert.equal(mounted.root.findAllByProps({ 'data-testid': 'pairing-qr-code' }).length, 1)
  await act(async () => {
    resolveStatus(CLOSED_UNLOCK_SNAPSHOT)
    await pendingStatus
  })
  assert.equal(mounted.root.findAllByProps({ 'data-testid': 'pairing-qr-code' }).length, 1, 'stale status must not erase a newer pairing session')
  await act(async () => { mounted.unmount() })
})

test('background status refresh pauses while a revocation confirmation is open', async () => {
  const polling = createPollingScheduler()
  let statusCalls = 0
  const bridge = bridgeWithStatus(async () => {
    statusCalls += 1
    return readySnapshot
  })
  let renderer: ReactTestRenderer | null = null
  await act(async () => {
    renderer = create(<UnlockSettingsPanel bridge={bridge} pairingPollScheduler={polling.scheduler} />)
  })
  const mounted = renderer as unknown as ReactTestRenderer
  await act(async () => {
    mounted.root.findByProps({ className: 'text-button unlock-revoke' }).props.onClick()
  })
  await act(async () => { polling.tick() })
  assert.equal(statusCalls, 1)
  assert.equal(mounted.root.findAllByProps({ role: 'alertdialog' }).length, 1)
  await act(async () => {
    mounted.root.findByProps({ 'data-testid': 'unlock-revoke-cancel' }).props.onClick()
  })
  await act(async () => { polling.tick() })
  assert.equal(statusCalls, 2)
  await act(async () => { mounted.unmount() })
})

test('idle polling never promotes a closed release gate to ready', async () => {
  const polling = createPollingScheduler()
  const bridge = bridgeWithStatus(async () => CLOSED_UNLOCK_SNAPSHOT)
  let renderer: ReactTestRenderer | null = null
  await act(async () => {
    renderer = create(<UnlockSettingsPanel bridge={bridge} pairingPollScheduler={polling.scheduler} />)
  })
  const mounted = renderer as unknown as ReactTestRenderer
  await act(async () => { polling.tick() })
  const beginPairing = mounted.root.findAllByType('button').find(button => button.props.children === '开始配对')!
  assert.equal(beginPairing.props.disabled, true)
  assert.equal(mounted.root.findByProps({ 'data-testid': 'unlock-install' }).props.disabled, true)
  assert.match(JSON.stringify(mounted.toJSON()), /安全验证门禁仍关闭/)
  await act(async () => { mounted.unmount() })
})

test('pending pairing polls status and enables confirmation only after the phone connects', async t => {
  const restoreWindow = installTimerWindow()
  t.after(restoreWindow)
  const polling = createPollingScheduler()
  const connectedSnapshot: UnlockSnapshot = {
    ...waitingPairingSnapshot,
    pendingPairing: {
      ...waitingPairingSnapshot.pendingPairing!,
      candidateName: 'realme GT5 Pro',
    },
  }
  let statusCalls = 0
  const bridge = bridgeWithStatus(async () => {
    statusCalls += 1
    return statusCalls === 1 ? waitingPairingSnapshot : connectedSnapshot
  })
  let renderer: ReactTestRenderer | null = null

  await act(async () => {
    renderer = create(<UnlockSettingsPanel bridge={bridge} pairingPollScheduler={polling.scheduler} />)
    await Promise.resolve()
  })
  const mounted = renderer as unknown as ReactTestRenderer
  assert.equal(polling.delay, 1000)
  const waitingConfirm = mounted.root.findByProps({ 'data-testid': 'confirm-pairing' })
  assert.equal(waitingConfirm.props.disabled, true)
  assert.deepEqual(waitingConfirm.props.children, '连接中…')

  await act(async () => {
    polling.tick()
    await Promise.resolve()
  })

  assert.equal(statusCalls, 2)
  assert.match(JSON.stringify(mounted.toJSON()), /等待确认：realme GT5 Pro/)
  const connectedConfirm = mounted.root.findByProps({ 'data-testid': 'confirm-pairing' })
  assert.equal(connectedConfirm.props.disabled, false)
  assert.deepEqual(connectedConfirm.props.children, '确认这台手机')
  await act(async () => { mounted.unmount() })
})

test('pairing status polling never overlaps and is inert after unmount', async t => {
  const restoreWindow = installTimerWindow()
  t.after(restoreWindow)
  const polling = createPollingScheduler()
  let resolvePollingStatus: (snapshot: unknown) => void = () => {}
  const pendingStatus = new Promise<unknown>(resolve => { resolvePollingStatus = resolve })
  let statusCalls = 0
  const bridge = bridgeWithStatus(() => {
    statusCalls += 1
    return statusCalls === 1 ? Promise.resolve(waitingPairingSnapshot) : pendingStatus
  })
  let renderer: ReactTestRenderer | null = null

  await act(async () => {
    renderer = create(<UnlockSettingsPanel bridge={bridge} pairingPollScheduler={polling.scheduler} />)
    await Promise.resolve()
  })
  const mounted = renderer as unknown as ReactTestRenderer

  act(() => {
    polling.tick()
    polling.tick()
    polling.tick()
  })
  assert.equal(statusCalls, 2)

  act(() => { mounted.unmount() })
  // The initial idle timer is replaced by the active-pairing timer; both are cleared.
  assert.deepEqual(polling.cleared, [73, 73])
  resolvePollingStatus(waitingPairingSnapshot)
  await pendingStatus
  await act(async () => { await Promise.resolve() })

  polling.tick()
  assert.equal(statusCalls, 2)
})
