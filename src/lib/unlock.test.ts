import assert from 'node:assert/strict'
import test from 'node:test'
import {
  CLOSED_UNLOCK_SNAPSHOT,
  beginUnlockRequest,
  calibrationProgress,
  deriveUnlockView,
  failUnlockRequest,
  finishUnlockRequest,
  normalizeUnlockSnapshot,
  pairingExpired,
  reduceRevocationConfirmation,
  unlockErrorCode,
  type UnlockSnapshot,
  type UnlockUiState,
} from './unlock.ts'

const NOW = 1_800_000_000_000

const readySnapshot: UnlockSnapshot = {
  schemaVersion: 1,
  capability: 'ready',
  authorizationGate: {
    state: 'passed',
    evidenceRecord: 'reviewed-gate-record',
    installAllowed: true,
  },
  components: {
    policy: 'ready',
    plugin: 'ready',
    service: 'ready',
    transport: 'ready',
  },
  devices: [{ id: 'phone_1', displayName: 'realme GT5 Pro', platform: 'android' }],
  pendingPairing: null,
  calibration: { deviceId: 'phone_1', phase: 'idle' },
  limitations: ['relayRisk', 'passwordFallback'],
}

test('unknown native snapshots normalize to a closed state', () => {
  for (const raw of [null, [], {}, { schemaVersion: 2 }, { schemaVersion: 1, capability: 'future-state' }]) {
    assert.deepEqual(normalizeUnlockSnapshot(raw), CLOSED_UNLOCK_SNAPSHOT)
  }
})

test('unknown or oversized native limitations close the snapshot instead of weakening it', () => {
  assert.deepEqual(normalizeUnlockSnapshot({
    ...readySnapshot,
    limitations: ['relayRisk', 'futureSafetyConstraint'],
  }), CLOSED_UNLOCK_SNAPSHOT)
  assert.deepEqual(normalizeUnlockSnapshot({
    ...readySnapshot,
    limitations: Array.from({ length: 7 }, () => 'relayRisk'),
  }), CLOSED_UNLOCK_SNAPSHOT)
})

test('closed authorization evidence cannot enable installation even when native input claims it can', () => {
  const snapshot = normalizeUnlockSnapshot({
    ...readySnapshot,
    authorizationGate: {
      state: 'notRun',
      evidenceRecord: 'docs/validation/macos-authorization-results.md',
      installAllowed: true,
    },
  })

  const view = deriveUnlockView(snapshot, NOW)
  assert.equal(view.installEnabled, false)
  assert.match(view.authorizationMessage, /尚未执行/)
})

test('failed and unsupported states have explicit fail-closed messages', () => {
  const failed = deriveUnlockView({
    ...readySnapshot,
    authorizationGate: { state: 'failed', evidenceRecord: 'failed-record', installAllowed: false },
  }, NOW)
  const unsupported = deriveUnlockView({
    ...readySnapshot,
    capability: 'unsupported',
  }, NOW)

  assert.match(failed.authorizationMessage, /未通过/)
  assert.match(unsupported.capabilityMessage, /不支持/)
  assert.equal(failed.installEnabled, false)
  assert.equal(unsupported.canBeginPairing, false)
})

test('pairing expires exactly at the published wall-clock deadline', () => {
  const session = {
    sessionId: 'pair_1',
    candidateName: 'realme GT5 Pro',
    qrPayload: 'repose://pair/redacted-test-payload',
    expiresAtEpochMs: NOW + 120_000,
  }

  assert.equal(pairingExpired(session, NOW + 119_999), false)
  assert.equal(pairingExpired(session, NOW + 120_000), true)
  assert.equal(pairingExpired({ ...session, expiresAtEpochMs: Number.NaN }, NOW), true)
})

test('calibration progress preserves near then far order and rejects overlap as incomplete', () => {
  assert.deepEqual(calibrationProgress('collectingNear'), { completedSteps: 0, totalSteps: 2 })
  assert.deepEqual(calibrationProgress('collectingFar'), { completedSteps: 1, totalSteps: 2 })
  assert.deepEqual(calibrationProgress('complete'), { completedSteps: 2, totalSteps: 2 })
  assert.deepEqual(calibrationProgress('overlapRejected'), { completedSteps: 0, totalSteps: 2 })
})

test('a failed revocation keeps the authoritative paired device visible', () => {
  const initial: UnlockUiState = {
    snapshot: readySnapshot,
    pendingRequestId: null,
    pendingAction: null,
    errorCode: null,
  }
  const pending = beginUnlockRequest(initial, 7, 'revoke')
  const failed = failUnlockRequest(pending, 7, 'backendUnavailable')

  assert.deepEqual(failed.snapshot.devices, readySnapshot.devices)
  assert.equal(failed.errorCode, 'backendUnavailable')
})

test('a failed status refresh discards a stale ready snapshot', () => {
  const initial: UnlockUiState = {
    snapshot: readySnapshot,
    pendingRequestId: null,
    pendingAction: null,
    errorCode: null,
  }
  const pending = beginUnlockRequest(initial, 8, 'status')
  const failed = failUnlockRequest(pending, 8, 'backendUnavailable')

  assert.deepEqual(failed.snapshot, CLOSED_UNLOCK_SNAPSHOT)
  assert.equal(deriveUnlockView(failed.snapshot, NOW).canBeginPairing, false)
  assert.equal(deriveUnlockView(failed.snapshot, NOW).canBeginCalibration, false)
  assert.equal(deriveUnlockView(failed.snapshot, NOW).canRevokeDevice, false)
})

test('a stale command result cannot replace a newer authoritative request', () => {
  const initial: UnlockUiState = {
    snapshot: CLOSED_UNLOCK_SNAPSHOT,
    pendingRequestId: null,
    pendingAction: null,
    errorCode: null,
  }
  const first = beginUnlockRequest(initial, 1, 'status')
  const second = beginUnlockRequest({ ...first, pendingRequestId: null, pendingAction: null }, 2, 'status')
  const stale = finishUnlockRequest(second, 1, readySnapshot)

  assert.equal(stale, second)
  assert.deepEqual(stale.snapshot, CLOSED_UNLOCK_SNAPSHOT)
})

test('duplicate actions remain bound to the first in-flight request', () => {
  const initial: UnlockUiState = {
    snapshot: readySnapshot,
    pendingRequestId: null,
    pendingAction: null,
    errorCode: null,
  }
  const first = beginUnlockRequest(initial, 10, 'calibrate')
  assert.equal(beginUnlockRequest(first, 11, 'calibrate'), first)
})

test('only fixed native error codes cross the UI boundary', () => {
  assert.equal(unlockErrorCode({ code: 'deviceNotFound', message: '/private/secret' }), 'deviceNotFound')
  assert.equal(unlockErrorCode({ code: 'futureError', message: 'raw native error' }), 'backendUnavailable')
  assert.equal(unlockErrorCode('raw native error'), 'backendUnavailable')
})

test('revocation requires an explicit confirmation bound to a still-paired device', () => {
  const requested = reduceRevocationConfirmation(null, { type: 'request', deviceId: 'phone_1' }, readySnapshot, false)
  assert.deepEqual(requested, { pendingDeviceId: 'phone_1', revokeDeviceId: null })

  const cancelled = reduceRevocationConfirmation(requested.pendingDeviceId, { type: 'cancel' }, readySnapshot, false)
  assert.deepEqual(cancelled, { pendingDeviceId: null, revokeDeviceId: null })

  const confirmed = reduceRevocationConfirmation(requested.pendingDeviceId, { type: 'confirm' }, readySnapshot, false)
  assert.deepEqual(confirmed, { pendingDeviceId: null, revokeDeviceId: 'phone_1' })

  const stale = reduceRevocationConfirmation(requested.pendingDeviceId, { type: 'confirm' }, {
    ...readySnapshot,
    devices: [],
  }, false)
  assert.deepEqual(stale, { pendingDeviceId: null, revokeDeviceId: null })

  const busy = reduceRevocationConfirmation(requested.pendingDeviceId, { type: 'confirm' }, readySnapshot, true)
  assert.deepEqual(busy, { pendingDeviceId: 'phone_1', revokeDeviceId: null })
})
