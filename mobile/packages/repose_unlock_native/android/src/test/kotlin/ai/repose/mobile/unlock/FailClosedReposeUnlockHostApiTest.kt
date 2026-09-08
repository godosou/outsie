package ai.repose.mobile.unlock

import ai.repose.mobile.unlock.generated.FlutterError
import ai.repose.mobile.unlock.generated.NativeCalibrationStep
import ai.repose.mobile.unlock.generated.NativeCompanionCapability
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
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
                NativeCompanionCapability.ASSOCIATION_NOT_CONFIGURED,
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

    @Test
    fun `companion association is delegated only while association is not configured`() {
        var delegated = false
        var callbackResult: Result<Unit>? = null
        val api = FailClosedReposeUnlockHostApi(
            availability = { NativeRuntimeAvailability.ASSOCIATION_NOT_CONFIGURED },
            requestCompanionAssociation = { callback ->
                delegated = true
                callback(Result.success(Unit))
            },
        )

        api.requestCompanionAssociation { callbackResult = it }

        assertTrue(delegated)
        assertTrue(callbackResult?.isSuccess == true)
    }

    @Test
    fun `companion association remains fail closed without an activity delegate`() {
        var callbackResult: Result<Unit>? = null
        val api = FailClosedReposeUnlockHostApi {
            NativeRuntimeAvailability.ASSOCIATION_NOT_CONFIGURED
        }

        api.requestCompanionAssociation { callbackResult = it }

        assertTrue(callbackResult?.isFailure == true)
        assertEquals(
            "activityUnavailable",
            (callbackResult?.exceptionOrNull() as FlutterError).code,
        )
    }

    @Test
    fun `companion association is not launched for unsupported or configured runtime`() {
        listOf(
            NativeRuntimeAvailability.UNSUPPORTED_API,
            NativeRuntimeAvailability.COMPANION_FEATURE_UNAVAILABLE,
            NativeRuntimeAvailability.TRANSPORT_NOT_IMPLEMENTED,
        ).forEach { availability ->
            var delegated = false
            var callbackResult: Result<Unit>? = null
            val api = FailClosedReposeUnlockHostApi(
                availability = { availability },
                requestCompanionAssociation = {
                    delegated = true
                },
            )

            api.requestCompanionAssociation { callbackResult = it }

            assertTrue(callbackResult?.isFailure == true)
            assertTrue(!delegated)
            assertNull(callbackResult?.getOrNull())
        }
    }
}
