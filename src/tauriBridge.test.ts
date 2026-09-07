import assert from 'node:assert/strict'
import test from 'node:test'
import { createUnlockBridge } from './tauriBridge.ts'

test('unlock bridge exposes exactly the six bounded domain commands', async () => {
  const calls: Array<{ command: string; args?: unknown }> = []
  const bridge = createUnlockBridge(async (command, args) => {
    calls.push({ command, args })
    return { command }
  })

  assert.deepEqual(Object.keys(bridge).sort(), [
    'beginCalibration',
    'beginPairing',
    'confirmPairing',
    'openUnlockDiagnostics',
    'revokeDevice',
    'unlockStatus',
  ])

  await bridge.unlockStatus()
  await bridge.beginPairing()
  await bridge.confirmPairing('pair_1')
  await bridge.beginCalibration('phone_1')
  await bridge.revokeDevice('phone_1')
  await bridge.openUnlockDiagnostics()

  assert.deepEqual(calls, [
    { command: 'unlock_status', args: undefined },
    { command: 'begin_pairing', args: undefined },
    { command: 'confirm_pairing', args: { value: { sessionId: 'pair_1' } } },
    { command: 'begin_calibration', args: { value: { deviceId: 'phone_1' } } },
    { command: 'revoke_device', args: { value: { deviceId: 'phone_1' } } },
    { command: 'open_unlock_diagnostics', args: undefined },
  ])
})
