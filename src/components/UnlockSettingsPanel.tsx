import { useEffect, useMemo, useRef, useState } from 'react'
import { Bluetooth, CheckCircle2, CircleAlert, KeyRound, LockKeyhole, ShieldAlert, Smartphone, Wrench } from 'lucide-react'
import type { UnlockDesktopBridge } from '../tauriBridge'
import {
  CLOSED_UNLOCK_SNAPSHOT,
  beginUnlockRequest,
  deriveUnlockView,
  failUnlockRequest,
  finishUnlockRequest,
  normalizeUnlockDiagnostics,
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
}

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
}

const healthCopy: Record<ComponentHealth, string> = {
  notChecked: '未检查',
  missing: '未安装',
  ready: '正常',
  degraded: '需要处理',
  unavailable: '不可用',
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
        <h2 id="unlock-settings-title">手机靠近解锁</h2>
        <p>通过已配对手机和校准后的蓝牙距离恢复当前 Mac 会话。</p>
      </div>
      <span className="subtle-badge"><KeyRound size={13} />安全预览</span>
    </div>

    <div className="unlock-gate" role="status" aria-live="polite">
      <ShieldAlert size={18} />
      <div><strong>{model.capabilityMessage}</strong><p>{model.authorizationMessage}</p></div>
    </div>
    {errorMessage && <div className="unlock-error" role="alert"><CircleAlert size={16} />{errorMessage}</div>}

    <div className="unlock-install-row">
      <div><h3>macOS 授权组件</h3><p>Task 8 实体机授权验证仍为 NOT RUN / GATE CLOSED。</p></div>
      <button type="button" className="button outline" data-testid="unlock-install" disabled>安装尚不可用</button>
    </div>

    <div className="unlock-health" aria-label="手机钥匙组件状态">
      {health.map(([label, state]) => <div key={label}><span>{label}</span><strong data-health={state}>{healthCopy[state]}</strong></div>)}
    </div>

    <div className="unlock-actions-grid">
      <article className="unlock-action-card">
        <Smartphone size={20} />
        <div><h3>已配对手机</h3><p>{snapshot.devices.length ? `${snapshot.devices.length} 台设备` : '尚无可用设备；后端接通前不会创建临时假配对。'}</p></div>
        <button type="button" className="button outline" disabled={busy || !model.canBeginPairing} onClick={onBeginPairing}>开始配对</button>
      </article>

      {pairing && <article className="unlock-session" aria-live="polite">
        <div><strong>{model.pairingIsExpired ? '配对会话已过期' : `等待确认：${pairing.candidateName}`}</strong><p>会话由原生后端生成并强制一次性使用；页面倒计时不承担安全校验。</p></div>
        <button type="button" className="button outline" disabled={busy || !model.canConfirmPairing} onClick={onConfirmPairing}>确认这台手机</button>
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

    <div className="unlock-safety-copy">
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

export function UnlockSettingsPanel({ bridge }: UnlockSettingsPanelProps) {
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
  const busy = state.pendingRequestId !== null
  const model = useMemo(() => deriveUnlockView(state.snapshot, now), [state.snapshot, now])

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
      setState(previous => finishUnlockRequest(previous, requestId, snapshot))
    } catch (error) {
      setState(previous => failUnlockRequest(previous, requestId, unlockErrorCode(error)))
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
  />
}
