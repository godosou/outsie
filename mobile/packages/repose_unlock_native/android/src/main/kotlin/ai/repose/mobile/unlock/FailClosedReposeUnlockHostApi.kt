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
        NativeCompanionCapability.BACKGROUND_EXECUTION_UNAVAILABLE,
        "A companion association is not configured.",
    ),
    TRANSPORT_NOT_IMPLEMENTED(
        NativeCompanionCapability.BACKGROUND_EXECUTION_UNAVAILABLE,
        "The companion association exists, but the encrypted transport is not implemented.",
    ),
}

internal class FailClosedReposeUnlockHostApi(
    private val availability: () -> NativeRuntimeAvailability,
) : ReposeUnlockHostApi {
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

    override fun beginPairing(qrPayload: String): NativePairingSession = unavailable("Pairing")

    override fun confirmPairing(sessionId: String): Unit = unavailable("Pairing confirmation")

    override fun startCalibration(): Unit = unavailable("Calibration")

    override fun submitCalibrationStep(step: NativeCalibrationStep): Unit =
        unavailable("Calibration")

    override fun revokeDevice(deviceId: String): Unit = unavailable("Device revocation")

    private fun unavailable(operation: String): Nothing = throw FlutterError(
        code = "capabilityUnavailable",
        message = "$operation is unavailable until the native encrypted transport is implemented.",
        details = null,
    )
}
