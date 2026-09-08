package ai.repose.mobile.unlock

import ai.repose.mobile.unlock.companion.AssociationSetupFailure
import ai.repose.mobile.unlock.companion.AssociationSetupOutcome
import ai.repose.mobile.unlock.generated.FlutterError
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AssociationSetupResultTest {
    @Test
    fun `successful system association completes the Flutter request`() {
        assertTrue(associationSetupResult(AssociationSetupOutcome.Associated).isSuccess)
    }

    @Test
    fun `every fail-closed association outcome has a stable safe error code`() {
        val expectedCodes = mapOf(
            AssociationSetupFailure.BUSY to "associationBusy",
            AssociationSetupFailure.PERMISSION_DENIED to "bluetoothPermissionDenied",
            AssociationSetupFailure.USER_CANCELLED to "associationCancelled",
            AssociationSetupFailure.ACTIVITY_DETACHED to "activityUnavailable",
            AssociationSetupFailure.DISCOVERY_FAILED to "associationDiscoveryFailed",
            AssociationSetupFailure.CONFIGURATION_FAILED to "associationConfigurationFailed",
        )

        expectedCodes.forEach { (failure, expectedCode) ->
            val result = associationSetupResult(AssociationSetupOutcome.Failed(failure))
            assertTrue(result.isFailure)
            assertEquals(expectedCode, (result.exceptionOrNull() as FlutterError).code)
        }
    }
}
