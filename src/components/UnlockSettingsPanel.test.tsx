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
    />,
  )

  assert.match(html, /data-testid="unlock-install"[^>]*disabled/)
  assert.doesNotMatch(html, /data-testid="unlock-install"[^>]*onClick/)
  assert.match(html, /系统密码/)
  assert.match(html, /中继/)
  assert.match(html, /手机被盗/)
  assert.match(html, /蓝牙关闭/)
  assert.match(html, /强制停止/)
})

test('the Repose settings page mounts the isolated unlock panel through the narrow bridge', () => {
  const appSource = readFileSync(new URL('../App.tsx', import.meta.url), 'utf8')
  assert.match(appSource, /import \{ UnlockSettingsPanel \} from '\.\/components\/UnlockSettingsPanel'/)
  assert.match(appSource, /<UnlockSettingsPanel bridge=\{window\.repose\?\.unlock\} \/>/)
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
    />,
  )

  assert.match(html, /确认撤销 realme GT5 Pro/)
  assert.match(html, /data-testid="unlock-revoke-cancel"/)
  assert.match(html, /data-testid="unlock-revoke-confirm"/)
})

test('mounted panel invokes one exact revocation while the first confirmation is pending', async () => {
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
    renderer = create(<UnlockSettingsPanel bridge={bridge} />)
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
