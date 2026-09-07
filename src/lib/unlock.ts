export type AuthorizationGateState = 'notRun' | 'failed' | 'passed'
export type ComponentHealth = 'notChecked' | 'missing' | 'ready' | 'degraded' | 'unavailable'
export type UnlockCapability = 'ready' | 'gateClosed' | 'serviceUnavailable' | 'unsupported'
export type CompanionPlatform = 'android' | 'ios'
export type CalibrationPhase = 'idle' | 'collectingNear' | 'collectingFar' | 'complete' | 'overlapRejected' | 'unavailable' | 'failed'
export type UnlockLimitation = 'task8GateClosed' | 'relayRisk' | 'stolenUnlockedPhone' | 'bluetoothOff' | 'androidForceStop' | 'passwordFallback'
export type UnlockAction = 'status' | 'pair' | 'confirm' | 'calibrate' | 'revoke' | 'diagnostics'
export type UnlockErrorCode = 'callerNotAllowed' | 'invalidRequest' | 'backendUnavailable' | 'busy' | 'pairingExpired' | 'deviceNotFound'

export interface AuthorizationGate {
  state: AuthorizationGateState
  evidenceRecord: string
  installAllowed: boolean
}

export interface ComponentHealthSnapshot {
  policy: ComponentHealth
  plugin: ComponentHealth
  service: ComponentHealth
  transport: ComponentHealth
}

export interface PairedDevice {
  id: string
  displayName: string
  platform: CompanionPlatform
}

export interface PairingSession {
  sessionId: string
  candidateName: string
  qrPayload: string
  expiresAtEpochMs: number
}

export interface CalibrationSnapshot {
  deviceId: string | null
  phase: CalibrationPhase
}

export interface UnlockSnapshot {
  schemaVersion: 1
  capability: UnlockCapability
  authorizationGate: AuthorizationGate
  components: ComponentHealthSnapshot
  devices: PairedDevice[]
  pendingPairing: PairingSession | null
  calibration: CalibrationSnapshot
  limitations: UnlockLimitation[]
}

export interface UnlockDiagnostics {
  schemaVersion: 1
  summary: 'gateClosed' | 'backendUnavailable'
  authorizationGate: AuthorizationGateState
  components: ComponentHealthSnapshot
  validationRecord: string
}

export interface CalibrationProgress {
  completedSteps: number
  totalSteps: 2
}

export interface UnlockViewModel {
  snapshot: UnlockSnapshot
  installEnabled: false
  authorizationMessage: string
  capabilityMessage: string
  canBeginPairing: boolean
  canConfirmPairing: boolean
  canBeginCalibration: boolean
  canRevokeDevice: boolean
  pairingIsExpired: boolean
  calibrationProgress: CalibrationProgress
}

export interface UnlockUiState {
  snapshot: UnlockSnapshot
  pendingRequestId: number | null
  pendingAction: UnlockAction | null
  errorCode: UnlockErrorCode | null
}

export type RevocationConfirmationEvent =
  | { type: 'request'; deviceId: string }
  | { type: 'cancel' }
  | { type: 'confirm' }

export interface RevocationConfirmationDecision {
  pendingDeviceId: string | null
  revokeDeviceId: string | null
}

const VALIDATION_RECORD = 'docs/validation/macos-authorization-results.md'
const IDENTIFIER = /^[A-Za-z0-9_-]{1,64}$/
const COMPONENT_HEALTH = new Set<ComponentHealth>(['notChecked', 'missing', 'ready', 'degraded', 'unavailable'])
const CAPABILITIES = new Set<UnlockCapability>(['ready', 'gateClosed', 'serviceUnavailable', 'unsupported'])
const GATE_STATES = new Set<AuthorizationGateState>(['notRun', 'failed', 'passed'])
const PLATFORMS = new Set<CompanionPlatform>(['android', 'ios'])
const CALIBRATION_PHASES = new Set<CalibrationPhase>(['idle', 'collectingNear', 'collectingFar', 'complete', 'overlapRejected', 'unavailable', 'failed'])
const LIMITATIONS = new Set<UnlockLimitation>(['task8GateClosed', 'relayRisk', 'stolenUnlockedPhone', 'bluetoothOff', 'androidForceStop', 'passwordFallback'])
const ERROR_CODES = new Set<UnlockErrorCode>(['callerNotAllowed', 'invalidRequest', 'backendUnavailable', 'busy', 'pairingExpired', 'deviceNotFound'])

export const CLOSED_UNLOCK_SNAPSHOT: UnlockSnapshot = {
  schemaVersion: 1,
  capability: 'gateClosed',
  authorizationGate: {
    state: 'notRun',
    evidenceRecord: VALIDATION_RECORD,
    installAllowed: false,
  },
  components: {
    policy: 'notChecked',
    plugin: 'notChecked',
    service: 'notChecked',
    transport: 'notChecked',
  },
  devices: [],
  pendingPairing: null,
  calibration: { deviceId: null, phase: 'unavailable' },
  limitations: ['task8GateClosed', 'relayRisk', 'stolenUnlockedPhone', 'bluetoothOff', 'androidForceStop', 'passwordFallback'],
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const isBoundedText = (value: unknown, maximum: number): value is string =>
  typeof value === 'string' && value.length > 0 && value.length <= maximum

const isIdentifier = (value: unknown): value is string =>
  typeof value === 'string' && IDENTIFIER.test(value)

function componentSnapshot(value: unknown): ComponentHealthSnapshot | null {
  if (!isRecord(value)) return null
  const fields = ['policy', 'plugin', 'service', 'transport'] as const
  if (!fields.every(field => COMPONENT_HEALTH.has(value[field] as ComponentHealth))) return null
  return Object.fromEntries(fields.map(field => [field, value[field]])) as unknown as ComponentHealthSnapshot
}

function pairedDevice(value: unknown): PairedDevice | null {
  if (!isRecord(value) || !isIdentifier(value.id) || !isBoundedText(value.displayName, 80) || !PLATFORMS.has(value.platform as CompanionPlatform)) return null
  return { id: value.id, displayName: value.displayName, platform: value.platform as CompanionPlatform }
}

function pairingSession(value: unknown): PairingSession | null {
  if (!isRecord(value)
    || !isIdentifier(value.sessionId)
    || !isBoundedText(value.candidateName, 80)
    || !isBoundedText(value.qrPayload, 4096)
    || typeof value.expiresAtEpochMs !== 'number'
    || !Number.isFinite(value.expiresAtEpochMs)
    || value.expiresAtEpochMs < 0) return null
  return {
    sessionId: value.sessionId,
    candidateName: value.candidateName,
    qrPayload: value.qrPayload,
    expiresAtEpochMs: value.expiresAtEpochMs,
  }
}

function calibrationSnapshot(value: unknown): CalibrationSnapshot | null {
  if (!isRecord(value) || !CALIBRATION_PHASES.has(value.phase as CalibrationPhase)) return null
  if (value.deviceId !== null && !isIdentifier(value.deviceId)) return null
  return { deviceId: value.deviceId as string | null, phase: value.phase as CalibrationPhase }
}

/** Treat native IPC as untrusted input and collapse malformed or future schemas to a closed state. */
export function normalizeUnlockSnapshot(value: unknown): UnlockSnapshot {
  if (!isRecord(value)
    || value.schemaVersion !== 1
    || !CAPABILITIES.has(value.capability as UnlockCapability)
    || !isRecord(value.authorizationGate)
    || !GATE_STATES.has(value.authorizationGate.state as AuthorizationGateState)
    || !isBoundedText(value.authorizationGate.evidenceRecord, 240)
    || typeof value.authorizationGate.installAllowed !== 'boolean'
    || !Array.isArray(value.devices)
    || value.devices.length > 16
    || !Array.isArray(value.limitations)
    || value.limitations.length > LIMITATIONS.size
    || !value.limitations.every(limitation => LIMITATIONS.has(limitation as UnlockLimitation))) return CLOSED_UNLOCK_SNAPSHOT

  const components = componentSnapshot(value.components)
  const calibration = calibrationSnapshot(value.calibration)
  if (!components || !calibration) return CLOSED_UNLOCK_SNAPSHOT

  const devices = value.devices.map(pairedDevice)
  if (devices.some(device => device === null)) return CLOSED_UNLOCK_SNAPSHOT

  const pendingPairing = value.pendingPairing === null ? null : pairingSession(value.pendingPairing)
  if (value.pendingPairing !== null && !pendingPairing) return CLOSED_UNLOCK_SNAPSHOT

  const limitations = value.limitations as UnlockLimitation[]

  const gateState = value.authorizationGate.state as AuthorizationGateState
  return {
    schemaVersion: 1,
    capability: value.capability as UnlockCapability,
    authorizationGate: {
      state: gateState,
      evidenceRecord: value.authorizationGate.evidenceRecord,
      installAllowed: gateState === 'passed' && value.authorizationGate.installAllowed,
    },
    components,
    devices: devices as PairedDevice[],
    pendingPairing,
    calibration,
    limitations,
  }
}

export function pairingExpired(session: PairingSession, now: number): boolean {
  return !Number.isFinite(session.expiresAtEpochMs) || !Number.isFinite(now) || now >= session.expiresAtEpochMs
}

export function calibrationProgress(phase: CalibrationPhase): CalibrationProgress {
  if (phase === 'collectingFar') return { completedSteps: 1, totalSteps: 2 }
  if (phase === 'complete') return { completedSteps: 2, totalSteps: 2 }
  return { completedSteps: 0, totalSteps: 2 }
}

function authorizationMessage(state: AuthorizationGateState): string {
  if (state === 'failed') return 'macOS 授权实体机验证未通过，自动解锁安装保持关闭。'
  if (state === 'passed') return '授权证据已记录，但此版本仍未开放系统安装。'
  return 'macOS 授权实体机验证尚未执行，自动解锁安装保持关闭。'
}

function capabilityMessage(capability: UnlockCapability): string {
  if (capability === 'ready') return '手机钥匙控制面已就绪。'
  if (capability === 'unsupported') return '当前平台不支持手机钥匙。'
  if (capability === 'serviceUnavailable') return '解锁后端尚未接通，请继续使用系统密码。'
  return '安全验证门禁仍关闭，请继续使用系统密码。'
}

export function deriveUnlockView(snapshot: UnlockSnapshot, now: number): UnlockViewModel {
  const pendingExpired = snapshot.pendingPairing ? pairingExpired(snapshot.pendingPairing, now) : false
  const operational = snapshot.capability === 'ready'
    && snapshot.components.service === 'ready'
    && snapshot.components.transport === 'ready'
  return {
    snapshot,
    installEnabled: false,
    authorizationMessage: authorizationMessage(snapshot.authorizationGate.state),
    capabilityMessage: capabilityMessage(snapshot.capability),
    canBeginPairing: operational && snapshot.pendingPairing === null,
    canConfirmPairing: operational && snapshot.pendingPairing !== null && !pendingExpired,
    canBeginCalibration: operational
      && snapshot.devices.length > 0
      && snapshot.pendingPairing === null
      && !['collectingNear', 'collectingFar'].includes(snapshot.calibration.phase),
    canRevokeDevice: snapshot.devices.length > 0,
    pairingIsExpired: pendingExpired,
    calibrationProgress: calibrationProgress(snapshot.calibration.phase),
  }
}

export function beginUnlockRequest(state: UnlockUiState, requestId: number, action: UnlockAction): UnlockUiState {
  if (state.pendingRequestId !== null) return state
  return { ...state, pendingRequestId: requestId, pendingAction: action, errorCode: null }
}

export function finishUnlockRequest(state: UnlockUiState, requestId: number, value: unknown): UnlockUiState {
  if (state.pendingRequestId !== requestId) return state
  return {
    snapshot: normalizeUnlockSnapshot(value),
    pendingRequestId: null,
    pendingAction: null,
    errorCode: null,
  }
}

export function failUnlockRequest(state: UnlockUiState, requestId: number, errorCode: UnlockErrorCode): UnlockUiState {
  if (state.pendingRequestId !== requestId) return state
  return {
    ...state,
    snapshot: state.pendingAction === 'status' ? CLOSED_UNLOCK_SNAPSHOT : state.snapshot,
    pendingRequestId: null,
    pendingAction: null,
    errorCode,
  }
}

export function unlockErrorCode(value: unknown): UnlockErrorCode {
  if (isRecord(value) && ERROR_CODES.has(value.code as UnlockErrorCode)) return value.code as UnlockErrorCode
  return 'backendUnavailable'
}

export function unlockErrorMessage(code: UnlockErrorCode | null): string | null {
  if (code === null) return null
  if (code === 'callerNotAllowed') return '此窗口不能管理手机钥匙。'
  if (code === 'invalidRequest') return '请求无效，请刷新状态后重试。'
  if (code === 'busy') return '已有手机钥匙操作正在进行。'
  if (code === 'pairingExpired') return '配对二维码已过期，请重新开始。'
  if (code === 'deviceNotFound') return '找不到该配对设备，请刷新状态。'
  return '手机钥匙后端尚未接通，请继续使用系统密码。'
}

export function reduceRevocationConfirmation(
  pendingDeviceId: string | null,
  event: RevocationConfirmationEvent,
  snapshot: UnlockSnapshot,
  busy: boolean,
): RevocationConfirmationDecision {
  if (event.type === 'cancel') return { pendingDeviceId: null, revokeDeviceId: null }
  if (busy) return { pendingDeviceId, revokeDeviceId: null }

  const deviceId = event.type === 'request' ? event.deviceId : pendingDeviceId
  if (!deviceId || !snapshot.devices.some(device => device.id === deviceId)) {
    return { pendingDeviceId: null, revokeDeviceId: null }
  }
  if (event.type === 'request') return { pendingDeviceId: deviceId, revokeDeviceId: null }
  return { pendingDeviceId: null, revokeDeviceId: deviceId }
}

export function normalizeUnlockDiagnostics(value: unknown): UnlockDiagnostics | null {
  if (!isRecord(value)
    || value.schemaVersion !== 1
    || !['gateClosed', 'backendUnavailable'].includes(value.summary as string)
    || !GATE_STATES.has(value.authorizationGate as AuthorizationGateState)
    || !isBoundedText(value.validationRecord, 240)) return null
  const components = componentSnapshot(value.components)
  if (!components) return null
  return {
    schemaVersion: 1,
    summary: value.summary as UnlockDiagnostics['summary'],
    authorizationGate: value.authorizationGate as AuthorizationGateState,
    components,
    validationRecord: value.validationRecord,
  }
}
