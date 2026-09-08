package ai.repose.mobile.unlock

import ai.repose.mobile.unlock.generated.FlutterError
import ai.repose.mobile.unlock.generated.NativeCalibrationPhase
import ai.repose.mobile.unlock.generated.NativeCalibrationSnapshot
import ai.repose.mobile.unlock.generated.NativeCalibrationStep
import ai.repose.mobile.unlock.generated.NativeCompanionCapability
import ai.repose.mobile.unlock.generated.NativeDiagnostics
import ai.repose.mobile.unlock.generated.NativePairingSession
import ai.repose.mobile.unlock.generated.NativeUnlockSnapshot
import ai.repose.mobile.unlock.generated.ReposeUnlockHostApi

internal interface LifecycleReposeUnlockHostApi : ReposeUnlockHostApi {
    fun close()
}

internal enum class NativeRuntimeAvailability(
    val capability: NativeCompanionCapability,
    val diagnostic: String,
) {
    UNSUPPORTED_API(
        NativeCompanionCapability.UNSUPPORTED_PLATFORM,
        "Automatic presence requires Android API 36.",
    ),
    COMPANION_FEATURE_UNAVAILABLE(
        NativeCompanionCapability.BACKGROUND_EXECUTION_UNAVAILABLE,
        "The companion-device background feature is unavailable.",
    ),
    ASSOCIATION_NOT_CONFIGURED(
        NativeCompanionCapability.ASSOCIATION_NOT_CONFIGURED,
        "A companion association is not configured.",
    ),
    TRANSPORT_NOT_IMPLEMENTED(
        NativeCompanionCapability.BACKGROUND_EXECUTION_UNAVAILABLE,
        "The companion association exists, but the encrypted transport is not implemented.",
    ),
}

internal class FailClosedReposeUnlockHostApi(
    private val requestCompanionAssociation: (((Result<Unit>) -> Unit) -> Unit)? = null,
    private val availability: () -> NativeRuntimeAvailability,
) : LifecycleReposeUnlockHostApi {
    override fun getSnapshot(): NativeUnlockSnapshot {
        val current = availability()
        return NativeUnlockSnapshot(
            capability = current.capability,
            devices = emptyList(),
            calibration = NativeCalibrationSnapshot(NativeCalibrationPhase.UNAVAILABLE),
            pendingPairing = null,
        )
    }

    override fun getDiagnostics(): NativeDiagnostics = NativeDiagnostics(
        summary = availability().diagnostic,
    )

    override fun requestCompanionAssociation(callback: (Result<Unit>) -> Unit) {
        if (availability() != NativeRuntimeAvailability.ASSOCIATION_NOT_CONFIGURED) {
            callback(Result.failure(FlutterError(
                code = "associationUnavailable",
                message = "A companion association cannot be started in the current runtime state.",
                details = null,
            )))
            return
        }
        val request = requestCompanionAssociation
        if (request == null) {
            callback(Result.failure(FlutterError(
                code = "activityUnavailable",
                message = "Open Repose on your phone before starting system association.",
                details = null,
            )))
            return
        }
        try {
            request(callback)
        } catch (error: RuntimeException) {
            callback(Result.failure(error))
        }
    }

    override fun beginPairing(qrPayload: String): NativePairingSession = unavailable("Pairing")

    override fun confirmPairing(sessionId: String): Unit = unavailable("Pairing confirmation")

    override fun startCalibration(): Unit = unavailable("Calibration")

    override fun submitCalibrationStep(step: NativeCalibrationStep): Unit =
        unavailable("Calibration")

    override fun revokeDevice(deviceId: String): Unit = unavailable("Device revocation")

    override fun close() = Unit

    private fun unavailable(operation: String): Nothing = throw FlutterError(
        code = "capabilityUnavailable",
        message = "$operation is unavailable until the native encrypted transport is implemented.",
        details = null,
    )
}
