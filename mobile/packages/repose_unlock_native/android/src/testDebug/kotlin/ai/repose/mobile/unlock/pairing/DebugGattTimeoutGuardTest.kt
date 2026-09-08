package ai.repose.mobile.unlock.pairing

import java.nio.charset.StandardCharsets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DebugGattTimeoutGuardTest {
    @Test
    fun `armed deadline fires exactly once with the configured short timeout`() {
        val scheduler = RecordingScheduler()
        var failures = 0
        val guard = DebugGattTimeoutGuard(scheduler, 15_000) { failures += 1 }

        guard.arm()
        assertEquals(15_000, scheduler.tasks.single().delayMillis)

        scheduler.tasks.single().run()
        scheduler.tasks.single().run()

        assertEquals(1, failures)
    }

    @Test
    fun `cancellation invalidates an already dequeued callback`() {
        val scheduler = RecordingScheduler()
        var failures = 0
        val guard = DebugGattTimeoutGuard(scheduler, 15_000) { failures += 1 }
        guard.arm()
        val stale = scheduler.tasks.single()

        guard.cancel()
        stale.run()

        assertTrue(stale.cancelled)
        assertEquals(0, failures)
    }

    @Test
    fun `rearming fences the old generation and only the current deadline can fire`() {
        val scheduler = RecordingScheduler()
        var failures = 0
        val guard = DebugGattTimeoutGuard(scheduler, 15_000) { failures += 1 }
        guard.arm()
        val stale = scheduler.tasks.single()

        guard.arm()
        val current = scheduler.tasks.last()
        stale.run()
        current.run()

        assertTrue(stale.cancelled)
        assertEquals(1, failures)
    }

    @Test
    fun `synchronous scheduler firing cannot leave a live stale ticket`() {
        val ticket = RecordingTicket(15_000) {}
        var failures = 0
        val scheduler = DebugGattTimeoutScheduler { _, callback ->
            callback()
            ticket
        }
        val guard = DebugGattTimeoutGuard(scheduler, 15_000) { failures += 1 }

        guard.arm()

        assertEquals(1, failures)
        assertTrue(ticket.cancelled)
    }

    @Test
    fun `only exact accepted status cancels the handshake deadline`() {
        val scheduler = RecordingScheduler()
        var failures = 0
        val deadline = DebugGattHandshakeDeadline(
            scheduler = scheduler,
            delayMillis = 15_000,
            expectedAcceptedStatus = ascii("ACCEPTED|session"),
            onTimeout = { failures += 1 },
        )
        deadline.arm()

        assertTrue(deadline.permitStatus(ascii("WAITING|session")))
        assertTrue(!scheduler.tasks.single().cancelled)
        assertTrue(deadline.permitStatus(ascii("ACCEPTED|session")))
        assertTrue(scheduler.tasks.single().cancelled)
        scheduler.tasks.single().run()

        assertEquals(0, failures)
    }

    @Test
    fun `timeout wins atomically over a late accepted callback`() {
        val scheduler = RecordingScheduler()
        var failures = 0
        val deadline = DebugGattHandshakeDeadline(
            scheduler = scheduler,
            delayMillis = 15_000,
            expectedAcceptedStatus = ascii("ACCEPTED|session"),
            onTimeout = { failures += 1 },
        )
        deadline.arm()

        scheduler.tasks.single().run()

        assertEquals(1, failures)
        assertTrue(!deadline.permitStatus(ascii("ACCEPTED|session")))
    }

    @Test
    fun `closing the handshake cancels timeout and rejects queued status`() {
        val scheduler = RecordingScheduler()
        var failures = 0
        val deadline = DebugGattHandshakeDeadline(
            scheduler = scheduler,
            delayMillis = 15_000,
            expectedAcceptedStatus = ascii("ACCEPTED|session"),
            onTimeout = { failures += 1 },
        )
        deadline.arm()

        deadline.close()
        scheduler.tasks.single().run()

        assertEquals(0, failures)
        assertTrue(!deadline.permitStatus(ascii("WAITING|session")))
    }

    private fun ascii(value: String): ByteArray = value.toByteArray(StandardCharsets.US_ASCII)

    private class RecordingScheduler : DebugGattTimeoutScheduler {
        val tasks = mutableListOf<RecordingTicket>()

        override fun schedule(
            delayMillis: Long,
            callback: () -> Unit,
        ): DebugGattTimeoutTicket = RecordingTicket(delayMillis, callback).also(tasks::add)
    }

    private class RecordingTicket(
        val delayMillis: Long,
        private val callback: () -> Unit,
    ) : DebugGattTimeoutTicket {
        var cancelled = false
            private set

        fun run() = callback()

        override fun cancel() {
            cancelled = true
        }
    }
}
