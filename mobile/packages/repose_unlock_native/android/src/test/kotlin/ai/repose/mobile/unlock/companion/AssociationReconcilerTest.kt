package ai.repose.mobile.unlock.companion

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AssociationReconcilerTest {
    @Test
    fun `startup reads my associations and observes the one persisted id`() {
        val store = RecordingStore(associationId = 23)
        val catalog = RecordingCatalog(listOf(11, 23, 42))
        val observer = RecordingObserver()
        val manager = CompanionAssociationManager(store, catalog, observer)

        val result = manager.reconcileAtStartup()

        assertEquals(AssociationReconciliation.Observing(23), result)
        assertEquals(1, catalog.readCount)
        assertEquals(listOf(23), observer.associationIds)
    }

    @Test
    fun `missing persisted association fails closed`() {
        val store = RecordingStore(associationId = 23)
        val observer = RecordingObserver()
        val manager = CompanionAssociationManager(
            store,
            RecordingCatalog(listOf(11, 42)),
            observer,
        )

        assertEquals(AssociationReconciliation.Missing, manager.reconcileAtStartup())
        assertTrue(observer.associationIds.isEmpty())
        assertEquals(null, store.associationId)
        assertEquals(1, store.clearCount)
    }

    @Test
    fun `duplicate association ids fail closed without observing`() {
        val observer = RecordingObserver()
        val manager = CompanionAssociationManager(
            RecordingStore(associationId = 23),
            RecordingCatalog(listOf(23, 23)),
            observer,
        )

        assertEquals(AssociationReconciliation.Ambiguous, manager.reconcileAtStartup())
        assertTrue(observer.associationIds.isEmpty())
    }

    @Test
    fun `startup without a persisted association never guesses`() {
        val catalog = RecordingCatalog(listOf(23))
        val observer = RecordingObserver()
        val manager = CompanionAssociationManager(RecordingStore(null), catalog, observer)

        assertEquals(AssociationReconciliation.NotConfigured, manager.reconcileAtStartup())
        assertEquals(1, catalog.readCount)
        assertTrue(observer.associationIds.isEmpty())
    }

    @Test
    fun `native configuration persists only an exact platform association before observing`() {
        val operations = mutableListOf<String>()
        val store = RecordingStore(null, operations)
        val observer = RecordingObserver(operations)
        val manager = CompanionAssociationManager(
            store,
            RecordingCatalog(listOf(23)),
            observer,
        )

        assertEquals(
            AssociationReconciliation.Observing(23),
            manager.configureAssociation(23),
        )
        assertEquals(23, store.associationId)
        assertEquals(listOf("save:23", "observe:23"), operations)
    }

    @Test
    fun `native configuration never persists an absent association`() {
        val store = RecordingStore(null)
        val observer = RecordingObserver()
        val manager = CompanionAssociationManager(
            store,
            RecordingCatalog(listOf(11, 42)),
            observer,
        )

        assertEquals(AssociationReconciliation.Missing, manager.configureAssociation(23))
        assertEquals(null, store.associationId)
        assertEquals(0, store.saveCount)
        assertTrue(observer.associationIds.isEmpty())
    }

    private class RecordingStore(
        var associationId: Int?,
        private val operations: MutableList<String>? = null,
    ) : AssociationIdStore {
        var saveCount = 0
        var clearCount = 0

        override fun loadAssociationId(): Int? = associationId

        override fun saveAssociationId(associationId: Int): Boolean {
            saveCount += 1
            this.associationId = associationId
            operations?.add("save:$associationId")
            return true
        }

        override fun clearAssociationId(): Boolean {
            clearCount += 1
            associationId = null
            return true
        }
    }

    private class RecordingCatalog(private val ids: List<Int>) : AssociationCatalog {
        var readCount = 0

        override fun getMyAssociationIds(): List<Int> {
            readCount += 1
            return ids
        }
    }

    private class RecordingObserver(
        private val operations: MutableList<String>? = null,
    ) : AssociationPresenceObserver {
        val associationIds = mutableListOf<Int>()

        override fun startObservingAssociation(associationId: Int) {
            associationIds += associationId
            operations?.add("observe:$associationId")
        }
    }
}
