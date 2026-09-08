package ai.repose.mobile.unlock.pairing

import ai.repose.mobile.unlock.companion.legacyAssociationToken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DebugAssociationSelectorTest {
    @Test
    fun `API 35 injected sole association is selected without API 36 presence state`() {
        val selected = DebugAssociationSelector.select(
            candidates = listOf(
                DebugCompanionAssociation(
                    associationId = 1,
                    deviceAddress = "02:00:00:00:00:01",
                ),
            ),
            preferred = null,
        )

        assertEquals(1, selected?.associationId)
        assertEquals("02:00:00:00:00:01", selected?.deviceAddress)
    }

    @Test
    fun `stored association selects one exact candidate from multiple`() {
        val selected = DebugAssociationSelector.select(
            candidates = listOf(
                DebugCompanionAssociation(1, "02:00:00:00:00:01"),
                DebugCompanionAssociation(7, "02:00:00:00:00:07"),
            ),
            preferred = DebugCompanionAssociation(7, "02:00:00:00:00:07"),
        )

        assertEquals(7, selected?.associationId)
    }

    @Test
    fun `ambiguous or replaced association remains fail closed`() {
        val candidates = listOf(
            DebugCompanionAssociation(1, "02:00:00:00:00:01"),
            DebugCompanionAssociation(2, "02:00:00:00:00:02"),
        )

        assertNull(DebugAssociationSelector.select(candidates, preferred = null))
        assertNull(
            DebugAssociationSelector.select(
                candidates,
                preferred = DebugCompanionAssociation(9, "02:00:00:00:00:09"),
            ),
        )
    }

    @Test
    fun `legacy API 31 association address remains a valid foreground GATT reference`() {
        val selected = DebugAssociationSelector.select(
            candidates = listOf(
                DebugCompanionAssociation(
                    associationId = null,
                    deviceAddress = "AA:BB:CC:DD:EE:FF",
                ),
            ),
            preferred = null,
        )

        assertNull(selected?.associationId)
        assertEquals("AA:BB:CC:DD:EE:FF", selected?.deviceAddress)
        assertEquals(
            legacyAssociationToken("aa:bb:cc:dd:ee:ff"),
            legacyAssociationToken("AA:BB:CC:DD:EE:FF"),
        )
    }

    @Test
    fun `invalid legacy addresses and duplicate ids are not selected`() {
        assertNull(
            DebugAssociationSelector.select(
                listOf(DebugCompanionAssociation(null, "not-a-mac")),
                preferred = null,
            ),
        )
        assertNull(
            DebugAssociationSelector.select(
                listOf(
                    DebugCompanionAssociation(1, "02:00:00:00:00:01"),
                    DebugCompanionAssociation(1, "02:00:00:00:00:01"),
                ),
                preferred = null,
            ),
        )
    }
}
