package ai.repose.mobile.unlock.responder

import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class EphemeralSlotLeaseTest {
    @Test
    fun `bounded slots fail closed when full and reuse a released slot`() {
        val directory = Files.createTempDirectory("repose-ephemeral-slot-test").toFile()
        val leases = mutableListOf<EphemeralSlotLease>()
        try {
            repeat(4) {
                leases += EphemeralSlotLease.acquire(directory, 4, "slot-")
            }
            assertEquals(listOf(0, 1, 2, 3), leases.map { it.slot })
            assertThrows(ResponderUnavailableException::class.java) {
                EphemeralSlotLease.acquire(directory, 4, "slot-")
            }

            leases.removeAt(1).close()
            val replacement = EphemeralSlotLease.acquire(directory, 4, "slot-")
            leases += replacement

            assertEquals(1, replacement.slot)
        } finally {
            leases.forEach(EphemeralSlotLease::close)
            directory.deleteRecursively()
        }
    }

    @Test
    fun `acquisition sweeps every unlocked slot while retaining one lease`() {
        val directory = Files.createTempDirectory("repose-ephemeral-sweep-test").toFile()
        val swept = mutableListOf<Int>()
        try {
            EphemeralSlotLease.acquireAndSweep(directory, 4, "slot-") { slot ->
                swept += slot
            }.use { retained ->
                assertEquals(0, retained.slot)
                assertEquals(listOf(0, 1, 2, 3), swept)
                assertEquals(1, EphemeralSlotLease.acquire(directory, 4, "slot-").use { it.slot })
            }
        } finally {
            directory.deleteRecursively()
        }
    }
}
