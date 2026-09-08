import { useEffect, useMemo, useRef, useState } from 'react'
import { Bluetooth, CheckCircle2, CircleAlert, LockKeyhole, RefreshCw, ShieldAlert, Smartphone, Wrench } from 'lucide-react'
import QRCodeImport from 'react-qr-code'
import type { UnlockDesktopBridge } from '../tauriBridge'
import {
  CLOSED_UNLOCK_SNAPSHOT,
  beginUnlockRequest,
  deriveUnlockView,
  failUnlockRequest,
  finishUnlockRequest,
  normalizeUnlockDiagnostics,
  normalizeUnlockSnapshot,
  reduceRevocationConfirmation,
  unlockErrorCode,
  unlockErrorMessage,
  type ComponentHealth,
  type UnlockAction,
  type UnlockDiagnostics,
  type RevocationConfirmationEvent,
  type UnlockUiState,
  type UnlockViewModel,
} from '../lib/unlock'

interface UnlockSettingsPanelProps {
  bridge?: UnlockDesktopBridge
  pairingPollScheduler?: PairingPollScheduler
}

interface PairingPollScheduler {
  setInterval: (callback: () => void, milliseconds: number) => number
  clearInterval: (timerId: number) => void
}

const browserPairingPollScheduler: PairingPollScheduler = {
  setInterval: (callback, milliseconds) => window.setInterval(callback, milliseconds),
  clearInterval: timerId => window.clearInterval(timerId),
}

const QRCode = ((QRCodeImport as unknown as { QRCode?: typeof QRCodeImport }).QRCode ?? QRCodeImport)

interface UnlockSettingsViewProps {
  model: UnlockViewModel
  busy: boolean
  diagnostics: UnlockDiagnostics | null
  errorMessage: string | null
  diagnosticsEnabled?: boolean
  pendingRevocationDeviceId: string | null
  onBeginPairing: () => void
  onConfirmPairing: () => void
  onBeginCalibration: () => void
  onRequestRevokeDevice: (deviceId: string) => void
  onCancelRevokeDevice: () => void
  onConfirmRevokeDevice: () => void
  onOpenDiagnostics: () => void
  pairingSecondsRemaining: number | null
}

const healthCopy: Record<ComponentHealth, string> = {
  notChecked: '未检查',
  missing: '未安装',
  ready: '正常',
  degraded: '需要处理',
  unavailable: '不可用',
}

function formatPairingCountdown(value: number | null): string {
  const totalSeconds = Number.isFinite(value) ? Math.max(0, Math.ceil(value ?? 0)) : 0
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`
}

export function UnlockSettingsView({
  model,
  busy,
  diagnostics,
  errorMessage,
  diagnosticsEnabled = true,
  pendingRevocationDeviceId,
  onBeginPairing,
  onConfirmPairing,
  onBeginCalibration,
  onRequestRevokeDevice,
  onCancelRevokeDevice,
  onConfirmRevokeDevice,
  onOpenDiagnostics,
  pairingSecondsRemaining,
}: UnlockSettingsViewProps) {
  const { snapshot } = model
  const pairing = snapshot.pendingPairing
  const health = [
    ['授权策略', snapshot.components.policy],
    ['授权插件', snapshot.components.plugin],
    ['Unlock Service', snapshot.components.service],
    ['手机传输', snapshot.components.transport],
  ] as const

  return <section className="panel preferences-panel unlock-panel" aria-labelledby="unlock-settings-title">
    <div className="section-heading">
      <div>
        <h2 id="unlock-settings-title">蓝牙配对与距离校准</h2>
        <p>生成一次性配对二维码，联调手机连接，并采集靠近与远离的蓝牙信号。</p>
      </div>
      <span className="subtle-badge"><Bluetooth size={13} />BLE 调试</span>
    </div>

    <div className="unlock-gate" role="status" aria-live="polite">
      <ShieldAlert size={18} />
      <div><strong>{model.capabilityMessage}</strong><p>{model.authorizationMessage}</p></div>
    </div>
    {errorMessage && <div className="unlock-error" role="alert"><CircleAlert size={16} />{errorMessage}</div>}

    <div className="unlock-install-row">
      <div><h3>BLE 配对调试</h3><p>可先联调配对二维码与蓝牙传输；macOS 自动解锁的系统授权尚未启用。</p></div>
      <button type="button" className="button outline" data-testid="unlock-install" disabled>系统授权未启用</button>
    </div>

    <div className="unlock-health" aria-label="手机钥匙组件状态">
      {health.map(([label, state]) => <div key={label}><span>{label}</span><strong data-health={state}>{healthCopy[state]}</strong></div>)}
    </div>

    <div className="unlock-actions-grid">
      <article className="unlock-action-card">
        <Smartphone size={20} />
        <div><h3>已配对手机</h3><p>{snapshot.devices.length ? `${snapshot.devices.length} 台设备` : '尚无可用设备。点击开始配对后，用手机 App 扫描 Mac 生成的二维码。'}</p></div>
        <button type="button" className="button outline" disabled={busy || !model.canBeginPairing} onClick={onBeginPairing}>开始配对</button>
      </article>

      {pairing && <article className="unlock-session">
        <div className="unlock-session-heading" aria-live="polite"><strong>{model.pairingIsExpired ? '配对会话已过期' : model.pairingPeerConnected ? `等待确认：${pairing.candidateName}` : '等待手机连接'}</strong><p>使用 Repose 手机 App 扫描下方二维码，连接后这里会自动更新设备状态。</p></div>
        {!model.pairingIsExpired && <div className="unlock-qr-layout">
          <figure className="unlock-qr-figure" data-testid="pairing-qr-code" role="img" aria-label="手机钥匙配对二维码，请使用 Repose 手机 App 扫描">
            <div className="unlock-qr-canvas" aria-hidden="true">
              <QRCode value={pairing.qrPayload} size={256} viewBox="0 0 256 256" level="M" bgColor="#FFFFFF" fgColor="#1F2D24" aria-hidden="true" focusable="false" />
            </div>
            <figcaption>打开 Repose 手机 App，选择“扫描 Mac 二维码”，将镜头对准此处。</figcaption>
          </figure>
          <div className="unlock-qr-status">
            <span>二维码有效期</span>
            <strong role="timer" aria-label={`二维码有效期还剩 ${formatPairingCountdown(pairingSecondsRemaining)}`}>{formatPairingCountdown(pairingSecondsRemaining)}</strong>
            <p><RefreshCw size={14} aria-hidden="true" />连接状态每秒自动刷新，无需手动操作。</p>
            <small>请保持此页面和手机蓝牙开启，直到设备名称出现在上方。</small>
          </div>
        </div>}
        {model.pairingIsExpired && <div className="unlock-qr-expired" role="status"><RefreshCw size={17} aria-hidden="true" /><div><strong>二维码已过期，正在自动刷新配对状态…</strong><p>状态同步完成后，请重新开始配对以生成新的二维码。</p></div></div>}
        <button type="button" className="button outline" data-testid="confirm-pairing" disabled={busy || !model.canConfirmPairing} onClick={onConfirmPairing}>{model.pairingIsExpired ? '等待刷新…' : model.pairingPeerConnected ? '确认这台手机' : '连接中…'}</button>
      </article>}

      {snapshot.devices.map(device => <article className="unlock-device" key={device.id}>
        <div><strong>{device.displayName}</strong><p>{device.platform === 'android' ? 'Android' : 'iPhone'} · 标识已隐藏</p></div>
        {pendingRevocationDeviceId === device.id
          ? <div className="unlock-revoke-confirmation" role="alertdialog" aria-label={`确认撤销 ${device.displayName}`}>
            <strong>确认撤销 {device.displayName}？</strong>
            <p>撤销后若要再次使用，需要重新配对并校准距离。</p>
            <div>
              <button type="button" className="text-button" data-testid="unlock-revoke-cancel" onClick={onCancelRevokeDevice}>取消</button>
              <button type="button" className="button outline" data-testid="unlock-revoke-confirm" disabled={busy || !model.canRevokeDevice} onClick={onConfirmRevokeDevice}>确认撤销</button>
            </div>
          </div>
          : <button type="button" className="text-button unlock-revoke" disabled={busy || !model.canRevokeDevice} onClick={() => onRequestRevokeDevice(device.id)}>撤销设备</button>}
      </article>)}

      <article className="unlock-action-card">
        <Bluetooth size={20} />
        <div><h3>距离校准</h3><p>进度 {model.calibrationProgress.completedSteps}/{model.calibrationProgress.totalSteps}；依次采集靠近和远离分布。</p></div>
        <button type="button" className="button outline" disabled={busy || !model.canBeginCalibration} onClick={onBeginCalibration}>开始校准</button>
      </article>
    </div>

    <div className="unlock-safety-notes">
      <h3><LockKeyhole size={16} />已知限制与恢复方式</h3>
      <ul>
        <li>蓝牙 RSSI 不是密码学距离证明，无法完全抵御无线中继。</li>
        <li>手机被盗且保持解锁时存在残余风险，应尽快在已解锁 Mac 上撤销设备。</li>
        <li>蓝牙关闭或 Android 应用被强制停止后，后台响应会失效。</li>
        <li>自动解锁不可用、超时或出错时，始终继续使用 macOS 系统密码。</li>
      </ul>
    </div>

    <div className="unlock-diagnostics-row">
      <button type="button" className="text-button" disabled={busy || !diagnosticsEnabled} onClick={onOpenDiagnostics}><Wrench size={14} />打开解锁诊断</button>
      {diagnostics && <span><CheckCircle2 size={14} />诊断状态：门禁关闭；未读取系统授权数据库</span>}
    </div>
  </section>
}

export function UnlockSettingsPanel({ bridge, pairingPollScheduler = browserPairingPollScheduler }: UnlockSettingsPanelProps) {
  const [state, setState] = useState<UnlockUiState>({
    snapshot: CLOSED_UNLOCK_SNAPSHOT,
    pendingRequestId: null,
    pendingAction: null,
    errorCode: null,
  })
  const [diagnostics, setDiagnostics] = useState<UnlockDiagnostics | null>(null)
  const [pendingRevocationDeviceId, setPendingRevocationDeviceId] = useState<string | null>(null)
  const [now, setNow] = useState(Date.now())
  const requestSequence = useRef(0)
  const inFlightRequest = useRef<number | null>(null)
  const statusPollInFlight = useRef(false)
  const mounted = useRef(true)
  const busy = state.pendingRequestId !== null
  const model = useMemo(() => deriveUnlockView(state.snapshot, now), [state.snapshot, now])
  const pairingSessionId = state.snapshot.pendingPairing?.sessionId ?? null

  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])

  useEffect(() => {
    const pairing = state.snapshot.pendingPairing
    if (!pairing || model.pairingIsExpired) return
    const delay = Math.max(1, Math.min(1000, pairing.expiresAtEpochMs - Date.now()))
    const timerId = window.setTimeout(() => setNow(Date.now()), delay)
    return () => window.clearTimeout(timerId)
  }, [state.snapshot.pendingPairing, model.pairingIsExpired, now])

  const runSnapshot = async (action: UnlockAction, operation: () => Promise<unknown>) => {
    if (inFlightRequest.current !== null) return
    const requestId = ++requestSequence.current
    inFlightRequest.current = requestId
    setState(previous => beginUnlockRequest(previous, requestId, action))
    try {
      const snapshot = await operation()
      if (mounted.current) setState(previous => finishUnlockRequest(previous, requestId, snapshot))
    } catch (error) {
      if (mounted.current) setState(previous => failUnlockRequest(previous, requestId, unlockErrorCode(error)))
    } finally {
      if (inFlightRequest.current === requestId) inFlightRequest.current = null
    }
  }

  useEffect(() => {
    if (!bridge) return
    const requestId = ++requestSequence.current
    let active = true
    inFlightRequest.current = requestId
    setState(previous => ({ ...previous, pendingRequestId: requestId, pendingAction: 'status', errorCode: null }))
    void bridge.unlockStatus().then(
      snapshot => { if (active) setState(previous => finishUnlockRequest(previous, requestId, snapshot)) },
      error => { if (active) setState(previous => failUnlockRequest(previous, requestId, unlockErrorCode(error))) },
    ).finally(() => {
      if (active && inFlightRequest.current === requestId) inFlightRequest.current = null
    })
    return () => {
      active = false
      if (inFlightRequest.current === requestId) inFlightRequest.current = null
    }
  }, [bridge])

  useEffect(() => {
    if (!bridge) return
    let active = true
    const timerId = pairingPollScheduler.setInterval(() => {
      if (!active || !mounted.current || pendingRevocationDeviceId || inFlightRequest.current !== null || statusPollInFlight.current) return
      const requestId = ++requestSequence.current
      statusPollInFlight.current = true
      // Refresh silently; a user action may supersede this read without waiting for it.
      void bridge.unlockStatus().then(
        snapshot => {
          if (active && mounted.current && requestSequence.current === requestId) {
            setState(previous => ({ ...previous, snapshot: normalizeUnlockSnapshot(snapshot), errorCode: null }))
          }
        },
        error => {
          if (active && mounted.current && requestSequence.current === requestId) {
            setState(previous => ({ ...previous, snapshot: CLOSED_UNLOCK_SNAPSHOT, errorCode: unlockErrorCode(error) }))
          }
        },
      ).finally(() => { statusPollInFlight.current = false })
    }, pairingSessionId ? 1000 : 3000)
    return () => {
      active = false
      pairingPollScheduler.clearInterval(timerId)
    }
  }, [bridge, pairingSessionId, pairingPollScheduler, pendingRevocationDeviceId])

  const openDiagnostics = async () => {
    if (!bridge || inFlightRequest.current !== null) return
    const requestId = ++requestSequence.current
    inFlightRequest.current = requestId
    setState(previous => beginUnlockRequest(previous, requestId, 'diagnostics'))
    try {
      const value = normalizeUnlockDiagnostics(await bridge.openUnlockDiagnostics())
      if (!value) throw new Error('invalid diagnostics')
      setDiagnostics(value)
      setState(previous => finishUnlockRequest(previous, requestId, previous.snapshot))
    } catch (error) {
      setState(previous => failUnlockRequest(previous, requestId, unlockErrorCode(error)))
    } finally {
      if (inFlightRequest.current === requestId) inFlightRequest.current = null
    }
  }

  const handleRevocation = (event: RevocationConfirmationEvent) => {
    const decision = reduceRevocationConfirmation(pendingRevocationDeviceId, event, state.snapshot, busy)
    setPendingRevocationDeviceId(decision.pendingDeviceId)
    if (decision.revokeDeviceId && bridge) {
      const deviceId = decision.revokeDeviceId
      void runSnapshot('revoke', () => bridge.revokeDevice(deviceId))
    }
  }

  return <UnlockSettingsView
    model={model}
    busy={busy}
    diagnostics={diagnostics}
    diagnosticsEnabled={Boolean(bridge)}
    errorMessage={unlockErrorMessage(state.errorCode)}
    pendingRevocationDeviceId={pendingRevocationDeviceId}
    onBeginPairing={() => { if (bridge && model.canBeginPairing) void runSnapshot('pair', bridge.beginPairing) }}
    onConfirmPairing={() => { if (bridge && model.canConfirmPairing && state.snapshot.pendingPairing) void runSnapshot('confirm', () => bridge.confirmPairing(state.snapshot.pendingPairing!.sessionId)) }}
    onBeginCalibration={() => { if (bridge && model.canBeginCalibration && state.snapshot.devices[0]) void runSnapshot('calibrate', () => bridge.beginCalibration(state.snapshot.devices[0].id)) }}
    onRequestRevokeDevice={deviceId => { handleRevocation({ type: 'request', deviceId }) }}
    onCancelRevokeDevice={() => { handleRevocation({ type: 'cancel' }) }}
    onConfirmRevokeDevice={() => { handleRevocation({ type: 'confirm' }) }}
    onOpenDiagnostics={() => { void openDiagnostics() }}
    pairingSecondsRemaining={state.snapshot.pendingPairing
      ? Math.max(0, Math.ceil((state.snapshot.pendingPairing.expiresAtEpochMs - now) / 1000))
      : null}
  />
}
