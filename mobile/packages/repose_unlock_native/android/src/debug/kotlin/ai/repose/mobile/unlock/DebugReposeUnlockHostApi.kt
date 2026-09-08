package ai.repose.mobile.unlock

import ai.repose.mobile.unlock.generated.FlutterError
import ai.repose.mobile.unlock.generated.NativeCalibrationPhase
import ai.repose.mobile.unlock.generated.NativeCalibrationSnapshot
import ai.repose.mobile.unlock.generated.NativeCalibrationStep
import ai.repose.mobile.unlock.generated.NativeCompanionCapability
import ai.repose.mobile.unlock.generated.NativeCompanionPlatform
import ai.repose.mobile.unlock.generated.NativeDiagnostics
import ai.repose.mobile.unlock.generated.NativePairedDevice
import ai.repose.mobile.unlock.generated.NativePairingSession
import ai.repose.mobile.unlock.generated.NativeUnlockSnapshot
import ai.repose.mobile.unlock.pairing.DebugPairingCoordinator
import ai.repose.mobile.unlock.pairing.DebugPairingException
import ai.repose.mobile.unlock.pairing.DebugPairingFailure

internal class DebugReposeUnlockHostApi(
    private val coordinator: DebugPairingCoordinator,
    requestCompanionAssociation: ((Result<Unit>) -> Unit) -> Unit,
    private val availability: () -> NativeRuntimeAvailability,
    private val bluetoothReady: () -> Boolean,
) : LifecycleReposeUnlockHostApi {
    private val associationDelegate = FailClosedReposeUnlockHostApi(
        requestCompanionAssociation = requestCompanionAssociation,
        availability = availability,
    )

    override fun getSnapshot(): NativeUnlockSnapshot {
        val capability = capability()
        if (capability != NativeCompanionCapability.READY) coordinator.disconnect()
        val debug = coordinator.snapshot()
        return NativeUnlockSnapshot(
            capability = capability,
            devices = debug.devices.map { device ->
                NativePairedDevice(
                    id = device.id,
                    displayName = device.displayName,
                    platform = NativeCompanionPlatform.ANDROID,
                )
            },
            calibration = NativeCalibrationSnapshot(NativeCalibrationPhase.UNAVAILABLE),
            pendingPairing = debug.pending?.let { session ->
                NativePairingSession(
                    sessionId = session.sessionId,
                    deviceName = session.deviceName,
                    expiresAtEpochMillis = session.expiresAtEpochMillis,
                )
            },
        )
    }

    override fun getDiagnostics(): NativeDiagnostics {
        val snapshot = getSnapshot()
        val state = when {
            snapshot.capability != NativeCompanionCapability.READY ->
                availability().diagnostic
            snapshot.pendingPairing != null -> "Debug BLE pairing is waiting for Mac acceptance."
            else -> "Debug BLE pairing transport is ready."
        }
        return NativeDiagnostics("$state Paired devices: ${snapshot.devices.size}.")
    }

    override fun requestCompanionAssociation(callback: (Result<Unit>) -> Unit) {
        associationDelegate.requestCompanionAssociation(callback)
    }

    override fun beginPairing(qrPayload: String): NativePairingSession {
        requireReady("Pairing")
        val session = pairingOperation { coordinator.begin(qrPayload) }
        return NativePairingSession(
            sessionId = session.sessionId,
            deviceName = session.deviceName,
            expiresAtEpochMillis = session.expiresAtEpochMillis,
        )
    }

    override fun confirmPairing(sessionId: String) {
        requireReady("Pairing confirmation")
        pairingOperation { coordinator.confirm(sessionId) }
    }

    override fun revokeDevice(deviceId: String) {
        requireReady("Device revocation")
        pairingOperation { coordinator.revoke(deviceId) }
    }

    override fun startCalibration(): Unit = calibrationUnavailable()

    override fun submitCalibrationStep(step: NativeCalibrationStep): Unit =
        calibrationUnavailable()

    override fun close() {
        coordinator.disconnect()
    }

    private fun capability(): NativeCompanionCapability = when (val current = availability()) {
        NativeRuntimeAvailability.TRANSPORT_NOT_IMPLEMENTED -> if (bluetoothReady()) {
            NativeCompanionCapability.READY
        } else {
            NativeCompanionCapability.BLUETOOTH_UNAVAILABLE
        }
        else -> current.capability
    }

    private fun requireReady(operation: String) {
        if (capability() != NativeCompanionCapability.READY) {
            coordinator.disconnect()
            throw FlutterError(
                code = "capabilityUnavailable",
                message = "$operation requires an active association and Bluetooth.",
                details = null,
            )
        }
    }

    private fun calibrationUnavailable(): Nothing = throw FlutterError(
        code = "capabilityUnavailable",
        message = "Calibration is not part of the debug pairing transport.",
        details = null,
    )

    private fun <T> pairingOperation(operation: () -> T): T = try {
        operation()
    } catch (error: DebugPairingException) {
        throw error.asFlutterError()
    }
}

private fun DebugPairingException.asFlutterError(): FlutterError {
    val (code, message) = when (reason) {
        DebugPairingFailure.INVALID_QR ->
            "invalidPairingCode" to "Scan a valid Repose pairing QR code."
        DebugPairingFailure.QR_EXPIRED ->
            "qrExpired" to "This pairing code has expired. Scan a new code on the Mac."
        DebugPairingFailure.QR_ALREADY_USED ->
            "qrAlreadyUsed" to "This pairing code has already been used."
        DebugPairingFailure.ASSOCIATION_UNAVAILABLE ->
            "capabilityUnavailable" to "Connect this phone to the Repose Mac first."
        DebugPairingFailure.BUSY ->
            "pairingBusy" to "Another pairing transaction is already active."
        DebugPairingFailure.NOT_ACCEPTED ->
            "pairingNotAccepted" to "Wait for the Repose Mac to accept this phone."
        DebugPairingFailure.SESSION_MISMATCH ->
            "pairingSessionMismatch" to "This pairing transaction is no longer active."
        DebugPairingFailure.DEVICE_NOT_FOUND ->
            "deviceNotFound" to "The paired device was not found."
        DebugPairingFailure.TRANSPORT_UNAVAILABLE ->
            "capabilityUnavailable" to "The Bluetooth pairing connection could not be opened."
    }
    return FlutterError(code = code, message = message, details = null)
}
