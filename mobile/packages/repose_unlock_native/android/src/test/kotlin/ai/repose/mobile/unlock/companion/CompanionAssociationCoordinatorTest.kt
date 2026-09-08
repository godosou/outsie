package ai.repose.mobile.unlock.companion

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CompanionAssociationCoordinatorTest {
    @Test
    fun `missing nearby permissions are requested before discovery and denial fails closed`() {
        val driver = RecordingAssociationDriver(
            missingPermissions = mutableListOf("scan", "connect"),
        )
        val coordinator = CompanionAssociationCoordinator(driver)
        var outcome: AssociationSetupOutcome? = null

        coordinator.requestAssociation { outcome = it }

        assertEquals(listOf("scan", "connect"), driver.requestedPermissions)
        assertNull(driver.discovery)
        assertNull(outcome)

        assertTrue(coordinator.onBluetoothPermissionsResult(driver.permissionRequestCode))
        assertEquals(
            AssociationSetupOutcome.Failed(AssociationSetupFailure.PERMISSION_DENIED),
            outcome,
        )
        assertNull(driver.discovery)
    }

    @Test
    fun `permission grant discovers only the fixed Repose service`() {
        val driver = RecordingAssociationDriver(
            missingPermissions = mutableListOf("scan", "connect"),
        )
        val coordinator = CompanionAssociationCoordinator(driver)

        coordinator.requestAssociation { }
        driver.missingPermissions.clear()
        assertTrue(coordinator.onBluetoothPermissionsResult(driver.permissionRequestCode))

        assertEquals(ReposeBluetoothContract.SERVICE_UUID, driver.discoveredServiceUuid)
        assertTrue(driver.discovery != null)
    }

    @Test
    fun `created association completes only after runtime configuration succeeds`() {
        val driver = RecordingAssociationDriver()
        val coordinator = CompanionAssociationCoordinator(driver)
        var outcome: AssociationSetupOutcome? = null

        coordinator.requestAssociation { outcome = it }
        driver.discovery!!.onAssociationPending(FakePrompt)
        assertEquals(FakePrompt, driver.launchedPrompt)
        assertNull(outcome)

        assertTrue(
            coordinator.onAssociationActivityResult(
                driver.associationRequestCode,
                accepted = true,
                associationId = 41,
            ),
        )
        assertEquals(41, driver.configuredAssociationId)
        assertNull(outcome)

        driver.configurationCompletion!!(AssociationConfiguration.CONFIGURED)
        assertEquals(AssociationSetupOutcome.Associated, outcome)
    }

    @Test
    fun `accepted chooser without an association reference fails closed`() {
        val driver = RecordingAssociationDriver()
        val coordinator = CompanionAssociationCoordinator(driver)
        val outcomes = mutableListOf<AssociationSetupOutcome>()

        coordinator.requestAssociation(outcomes::add)
        driver.discovery!!.onAssociationPending(FakePrompt)

        assertTrue(
            coordinator.onAssociationActivityResult(
                driver.associationRequestCode,
                accepted = true,
                associationId = null,
            ),
        )
        assertEquals(
            listOf(
                AssociationSetupOutcome.Failed(
                    AssociationSetupFailure.CONFIGURATION_FAILED,
                ),
            ),
            outcomes,
        )
        driver.discovery!!.onAssociationCreated(41)
        assertNull(driver.configuredAssociationId)
    }

    @Test
    fun `cancelled chooser fails closed without configuring an association`() {
        val driver = RecordingAssociationDriver()
        val coordinator = CompanionAssociationCoordinator(driver)
        var outcome: AssociationSetupOutcome? = null

        coordinator.requestAssociation { outcome = it }
        driver.discovery!!.onAssociationPending(FakePrompt)

        assertTrue(
            coordinator.onAssociationActivityResult(
                driver.associationRequestCode,
                accepted = false,
                associationId = null,
            ),
        )
        assertEquals(
            AssociationSetupOutcome.Failed(AssociationSetupFailure.USER_CANCELLED),
            outcome,
        )
        assertNull(driver.configuredAssociationId)
    }

    @Test
    fun `duplicate platform prompt callback cannot launch two chooser activities`() {
        val driver = RecordingAssociationDriver()
        val coordinator = CompanionAssociationCoordinator(driver)

        coordinator.requestAssociation { }
        driver.discovery!!.onAssociationPending(FakePrompt)
        driver.discovery!!.onAssociationPending(FakePrompt)

        assertEquals(1, driver.launchedPromptCount)
    }

    @Test
    fun `activity detach cancels pending work and ignores stale callbacks`() {
        val driver = RecordingAssociationDriver()
        val coordinator = CompanionAssociationCoordinator(driver)
        val outcomes = mutableListOf<AssociationSetupOutcome>()

        coordinator.requestAssociation(outcomes::add)
        val staleDiscovery = driver.discovery!!
        coordinator.detach()
        staleDiscovery.onAssociationCreated(13)
        staleDiscovery.onFailure("late platform callback")

        assertEquals(
            listOf(AssociationSetupOutcome.Failed(AssociationSetupFailure.ACTIVITY_DETACHED)),
            outcomes,
        )
        assertNull(driver.configuredAssociationId)
        assertFalse(
            coordinator.onAssociationActivityResult(
                driver.associationRequestCode,
                accepted = true,
                associationId = 13,
            ),
        )
    }

    @Test
    fun `stale discovery callback cannot complete a later request`() {
        val driver = RecordingAssociationDriver()
        val coordinator = CompanionAssociationCoordinator(driver)
        val firstOutcomes = mutableListOf<AssociationSetupOutcome>()
        val secondOutcomes = mutableListOf<AssociationSetupOutcome>()

        coordinator.requestAssociation(firstOutcomes::add)
        val staleDiscovery = driver.discovery!!
        staleDiscovery.onFailure("first failed")
        assertEquals(
            listOf(AssociationSetupOutcome.Failed(AssociationSetupFailure.DISCOVERY_FAILED)),
            firstOutcomes,
        )

        coordinator.requestAssociation(secondOutcomes::add)
        val currentDiscovery = driver.discovery!!
        staleDiscovery.onAssociationCreated(7)
        assertNull(driver.configuredAssociationId)
        assertTrue(secondOutcomes.isEmpty())

        currentDiscovery.onAssociationCreated(8)
        driver.configurationCompletion!!(AssociationConfiguration.CONFIGURED)
        assertEquals(8, driver.configuredAssociationId)
        assertEquals(listOf(AssociationSetupOutcome.Associated), secondOutcomes)
    }

    @Test
    fun `concurrent request is rejected without replacing the active callback`() {
        val driver = RecordingAssociationDriver()
        val coordinator = CompanionAssociationCoordinator(driver)
        val firstOutcomes = mutableListOf<AssociationSetupOutcome>()
        val secondOutcomes = mutableListOf<AssociationSetupOutcome>()

        coordinator.requestAssociation(firstOutcomes::add)
        coordinator.requestAssociation(secondOutcomes::add)

        assertTrue(firstOutcomes.isEmpty())
        assertEquals(
            listOf(AssociationSetupOutcome.Failed(AssociationSetupFailure.BUSY)),
            secondOutcomes,
        )
    }

    private object FakePrompt : AssociationPrompt

    private class RecordingAssociationDriver(
        val missingPermissions: MutableList<String> = mutableListOf(),
    ) : CompanionAssociationDriver {
        var requestedPermissions: List<String>? = null
        var permissionRequestCode = -1
        var discoveredServiceUuid: String? = null
        var discovery: AssociationDiscoveryEvents? = null
        var launchedPrompt: AssociationPrompt? = null
        var launchedPromptCount = 0
        var associationRequestCode = -1
        var configuredAssociationId: Int? = null
        var configurationCompletion: ((AssociationConfiguration) -> Unit)? = null

        override fun missingBluetoothPermissions(): List<String> = missingPermissions.toList()

        override fun requestBluetoothPermissions(permissions: List<String>, requestCode: Int) {
            requestedPermissions = permissions
            permissionRequestCode = requestCode
        }

        override fun discoverReposeMac(
            serviceUuid: String,
            events: AssociationDiscoveryEvents,
        ) {
            discoveredServiceUuid = serviceUuid
            discovery = events
        }

        override fun launchAssociationPrompt(prompt: AssociationPrompt, requestCode: Int) {
            launchedPrompt = prompt
            launchedPromptCount += 1
            associationRequestCode = requestCode
        }

        override fun configureAssociation(
            associationId: Int,
            completion: (AssociationConfiguration) -> Unit,
        ) {
            configuredAssociationId = associationId
            configurationCompletion = completion
        }
    }
}
