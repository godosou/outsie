package ai.repose.mobile.unlock.companion

import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PresenceEventRouterTest {
    @Test
    fun `unknown and mismatched events are ignored`() {
        val accepted = mutableListOf<PresenceTransition>()
        val router = PresenceEventRouter({ 41 }, accepted::add)

        assertFalse(router.route(99, PresenceEventKind.BLE_APPEARED.platformValue))
        assertFalse(router.route(41, 99))
        assertFalse(router.route(DevicePresenceNoAssociation, PresenceEventKind.BLE_APPEARED.platformValue))

        assertTrue(accepted.isEmpty())
    }

    @Test
    fun `duplicate events enqueue exactly one native transition`() {
        val accepted = mutableListOf<PresenceTransition>()
        val router = PresenceEventRouter({ 41 }, accepted::add)

        assertTrue(router.route(41, PresenceEventKind.BLE_APPEARED.platformValue))
        assertFalse(router.route(41, PresenceEventKind.BLE_APPEARED.platformValue))

        assertEquals(
            listOf(PresenceTransition(41, PresenceEventKind.BLE_APPEARED)),
            accepted,
        )
    }

    @Test
    fun `different platform signals and real state changes are preserved`() {
        val accepted = mutableListOf<PresenceTransition>()
        val router = PresenceEventRouter({ 41 }, accepted::add)

        assertTrue(router.route(41, PresenceEventKind.BLE_APPEARED.platformValue))
        assertTrue(router.route(41, PresenceEventKind.BT_CONNECTED.platformValue))
        assertTrue(router.route(41, PresenceEventKind.BLE_DISAPPEARED.platformValue))
        assertTrue(router.route(41, PresenceEventKind.BLE_APPEARED.platformValue))

        assertEquals(4, accepted.size)
    }

    @Test
    fun `concurrent duplicate callbacks still enqueue once`() {
        val accepted = Collections.synchronizedList(mutableListOf<PresenceTransition>())
        val router = PresenceEventRouter({ 41 }, accepted::add)
        val start = CountDownLatch(1)
        val done = CountDownLatch(32)
        val executor = Executors.newFixedThreadPool(8)

        repeat(32) {
            executor.execute {
                start.await()
                router.route(41, PresenceEventKind.BLE_APPEARED.platformValue)
                done.countDown()
            }
        }
        start.countDown()
        done.await()
        executor.shutdownNow()

        assertEquals(1, accepted.size)
    }

    @Test
    fun `concurrent distinct callbacks enqueue in their accepted order`() {
        val accepted = Collections.synchronizedList(mutableListOf<PresenceTransition>())
        val firstEnteredEnqueue = CountDownLatch(1)
        val releaseFirstEnqueue = CountDownLatch(1)
        val secondEnteredEnqueue = CountDownLatch(1)
        val router = PresenceEventRouter(
            activeAssociationId = { 41 },
            enqueue = { transition ->
                if (transition.event == PresenceEventKind.BLE_APPEARED) {
                    firstEnteredEnqueue.countDown()
                    releaseFirstEnqueue.await()
                } else {
                    secondEnteredEnqueue.countDown()
                }
                accepted += transition
            },
        )
        val executor = Executors.newFixedThreadPool(2)

        val appeared = executor.submit<Boolean> {
            router.route(41, PresenceEventKind.BLE_APPEARED.platformValue)
        }
        assertTrue(firstEnteredEnqueue.await(1, TimeUnit.SECONDS))
        val disappeared = executor.submit<Boolean> {
            router.route(41, PresenceEventKind.BLE_DISAPPEARED.platformValue)
        }
        // The broken implementation reaches the second enqueue while the first is paused.
        secondEnteredEnqueue.await(200, TimeUnit.MILLISECONDS)
        releaseFirstEnqueue.countDown()

        assertTrue(appeared.get(1, TimeUnit.SECONDS))
        assertTrue(disappeared.get(1, TimeUnit.SECONDS))
        executor.shutdownNow()
        assertEquals(
            listOf(
                PresenceTransition(41, PresenceEventKind.BLE_APPEARED),
                PresenceTransition(41, PresenceEventKind.BLE_DISAPPEARED),
            ),
            accepted,
        )
    }

    @Test
    fun `failed enqueue does not poison duplicate retry`() {
        var fail = true
        val accepted = mutableListOf<PresenceTransition>()
        val router = PresenceEventRouter(
            activeAssociationId = { 41 },
            enqueue = { transition ->
                if (fail) {
                    fail = false
                    error("queue unavailable")
                }
                accepted += transition
            },
        )

        assertThrows(IllegalStateException::class.java) {
            router.route(41, PresenceEventKind.BLE_APPEARED.platformValue)
        }
        assertTrue(router.route(41, PresenceEventKind.BLE_APPEARED.platformValue))
        assertEquals(1, accepted.size)
    }

    @Test
    fun `matching association removal clears state and rejects later stale events`() {
        var activeAssociationId: Int? = 41
        var persistedAssociationId: Int? = 41
        var lastPresence: PresenceTransition? = PresenceTransition(
            41,
            PresenceEventKind.BLE_APPEARED,
        )
        val router = PresenceEventRouter(
            activeAssociationId = { activeAssociationId },
            enqueue = { lastPresence = it },
            removeAssociation = { associationId: Int ->
                assertEquals(activeAssociationId, associationId)
                persistedAssociationId = null
                activeAssociationId = null
                lastPresence = null
            },
        )

        assertTrue(router.route(41, 6))
        assertNull(activeAssociationId)
        assertNull(persistedAssociationId)
        assertNull(lastPresence)
        assertFalse(router.route(41, PresenceEventKind.BLE_APPEARED.platformValue))
    }

    @Test
    fun `mismatched association removal cannot clear the active association`() {
        var activeAssociationId: Int? = 41
        var removals = 0
        val router = PresenceEventRouter(
            activeAssociationId = { activeAssociationId },
            enqueue = {},
            removeAssociation = {
                removals += 1
                activeAssociationId = null
            },
        )

        assertFalse(router.route(99, 6))
        assertEquals(41, activeAssociationId)
        assertEquals(0, removals)
    }
}
