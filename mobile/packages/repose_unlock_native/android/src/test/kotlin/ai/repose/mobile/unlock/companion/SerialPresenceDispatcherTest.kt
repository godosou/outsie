package ai.repose.mobile.unlock.companion

import java.util.ArrayDeque
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SerialPresenceDispatcherTest {
    @Test
    fun `startup reconciliation is queued before an immediately arriving presence event`() {
        val tasks = ArrayDeque<() -> Unit>()
        val executionOrder = mutableListOf<String>()
        var activeAssociationId: Int? = null
        val transitions = mutableListOf<PresenceTransition>()
        val router = PresenceEventRouter({ activeAssociationId }, transitions::add)
        val dispatcher = SerialPresenceDispatcher(
            execute = tasks::addLast,
            reconcileAtStartup = {
                executionOrder += "reconcile"
                activeAssociationId = 41
            },
            route = { associationId: Int, event: Int ->
                executionOrder += "event"
                router.route(associationId, event)
            },
        )

        dispatcher.enqueue(41, PresenceEventKind.BLE_APPEARED.platformValue)
        assertTrue(transitions.isEmpty())
        while (tasks.isNotEmpty()) tasks.removeFirst().invoke()

        assertEquals(listOf("reconcile", "event"), executionOrder)
        assertEquals(
            listOf(PresenceTransition(41, PresenceEventKind.BLE_APPEARED)),
            transitions,
        )
    }
}
