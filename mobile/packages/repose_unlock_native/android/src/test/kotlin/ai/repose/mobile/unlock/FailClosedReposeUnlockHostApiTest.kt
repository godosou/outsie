package ai.repose.mobile.unlock

import ai.repose.mobile.unlock.generated.FlutterError
import ai.repose.mobile.unlock.generated.NativeCalibrationStep
import ai.repose.mobile.unlock.generated.NativeCompanionCapability
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FailClosedReposeUnlockHostApiTest {
    @Test
    fun `snapshot and diagnostics distinguish every unavailable runtime state`() {
        val expectations = listOf(
            Triple(
                NativeRuntimeAvailability.UNSUPPORTED_API,
                NativeCompanionCapability.UNSUPPORTED_PLATFORM,
                "Android API 36",
            ),
            Triple(
                NativeRuntimeAvailability.COMPANION_FEATURE_UNAVAILABLE,
                NativeCompanionCapability.BACKGROUND_EXECUTION_UNAVAILABLE,
                "companion-device",
            ),
            Triple(
                NativeRuntimeAvailability.ASSOCIATION_NOT_CONFIGURED,
                NativeCompanionCapability.BACKGROUND_EXECUTION_UNAVAILABLE,
                "not configured",
            ),
            Triple(
                NativeRuntimeAvailability.TRANSPORT_NOT_IMPLEMENTED,
                NativeCompanionCapability.BACKGROUND_EXECUTION_UNAVAILABLE,
                "not implemented",
            ),
        )

        expectations.forEach { (availability, expectedCapability, diagnosticFragment) ->
            val api = FailClosedReposeUnlockHostApi { availability }
            val snapshot = api.getSnapshot()

            assertEquals(expectedCapability, snapshot.capability)
            assertTrue(snapshot.devices.isEmpty())
            assertEquals(null, snapshot.pendingPairing)
            assertTrue(
                api.getDiagnostics().summary.contains(diagnosticFragment, ignoreCase = true),
            )
        }
    }

    @Test
    fun `all unfinished domain mutations fail explicitly`() {
        val api = FailClosedReposeUnlockHostApi {
            NativeRuntimeAvailability.ASSOCIATION_NOT_CONFIGURED
        }
        val operations = listOf<() -> Unit>(
            { api.beginPairing("qr") },
            { api.confirmPairing("session") },
            { api.startCalibration() },
            { api.submitCalibrationStep(NativeCalibrationStep.NEAR) },
            { api.revokeDevice("device") },
        )

        operations.forEach { operation ->
            val error = runCatching(operation).exceptionOrNull()
            assertTrue(error is FlutterError)
            assertEquals("capabilityUnavailable", (error as FlutterError).code)
        }
    }
}
