package ai.repose.mobile.unlock.bluetooth

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class GattFragmentCodecTest {
    @Test
    fun `reassembles the v1 challenge at every possible split`() {
        val frame = BluetoothTestFixtures.challenge()

        for (split in 1 until frame.size) {
            val fragments = GattFragmentCodec.encodeWithPayloadSizes(
                frame,
                listOf(split, frame.size - split),
            )
            var reassembly: GattReassembly = GattReassembly.Empty
            fragments.forEach { fragment ->
                reassembly = GattFragmentCodec.accept(reassembly, fragment)
            }

            assertTrue("split $split did not complete", reassembly is GattReassembly.Complete)
            assertArrayEquals(frame, GattFragmentCodec.finish(reassembly))
        }
    }

    @Test
    fun `reassembles the maximum length v1 response at every possible split`() {
        val frame = BluetoothTestFixtures.response()
        assertEquals(GattFragmentCodec.maxFrameLength, frame.size)

        for (split in 1 until frame.size) {
            val fragments = GattFragmentCodec.encodeWithPayloadSizes(
                frame,
                listOf(split, frame.size - split),
            )
            val complete = fragments.fold<ByteArray, GattReassembly>(
                GattReassembly.Empty,
            ) { state, fragment -> GattFragmentCodec.accept(state, fragment) }

            assertArrayEquals(frame, GattFragmentCodec.finish(complete))
        }
    }

    @Test
    fun `negotiated packet size is a hard bound and round trips`() {
        val frame = BluetoothTestFixtures.challenge()

        // 23 is an already-derived ATT characteristic value bound, not an MTU.
        val fragments = GattFragmentCodec.encode(frame, maximumAttributeValueBytes = 23)

        assertTrue(fragments.size > 1)
        assertTrue(fragments.all { it.size <= 23 })
        val complete = fragments.fold<ByteArray, GattReassembly>(
            GattReassembly.Empty,
        ) { state, packet -> GattFragmentCodec.accept(state, packet) }
        assertArrayEquals(frame, GattFragmentCodec.finish(complete))
    }

    @Test
    fun `rejects duplicate out of order and inconsistent sequences`() {
        val fragments = GattFragmentCodec.encodeWithPayloadSizes(
            BluetoothTestFixtures.challenge(),
            listOf(40, 40, 169),
        )
        val afterFirst = GattFragmentCodec.accept(GattReassembly.Empty, fragments[0])

        assertThrows(FragmentFormatException::class.java) {
            GattFragmentCodec.accept(afterFirst, fragments[0])
        }
        assertThrows(FragmentFormatException::class.java) {
            GattFragmentCodec.accept(GattReassembly.Empty, fragments[1])
        }
        assertThrows(FragmentFormatException::class.java) {
            GattFragmentCodec.accept(afterFirst, fragments[2])
        }
    }

    @Test
    fun `rejects truncated extra and post completion data`() {
        val fragments = GattFragmentCodec.encodeWithPayloadSizes(
            BluetoothTestFixtures.challenge(),
            listOf(100, 149),
        )
        val partial = GattFragmentCodec.accept(GattReassembly.Empty, fragments[0])
        val complete = GattFragmentCodec.accept(partial, fragments[1])

        assertThrows(FragmentFormatException::class.java) {
            GattFragmentCodec.finish(partial)
        }
        assertThrows(FragmentFormatException::class.java) {
            GattFragmentCodec.accept(complete, fragments[1])
        }
        assertThrows(FragmentFormatException::class.java) {
            GattFragmentCodec.accept(GattReassembly.Empty, fragments[0] + byteArrayOf(0))
        }
    }

    @Test
    fun `rejects noncanonical headers lengths and last flags`() {
        val valid = GattFragmentCodec.encodeWithPayloadSizes(
            BluetoothTestFixtures.challenge(),
            listOf(100, 149),
        )
        val first = valid[0]
        val second = valid[1]
        val mutations = listOf(
            first.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() },
            first.copyOf().also { it[2] = 2 },
            first.copyOf().also { it[3] = 2 },
            first.copyOf().also { it[3] = 1 },
            first.copyOf().also { it[4] = 0; it[5] = 0 },
            first.copyOf().also { it[4] = 1; it[5] = (-1).toByte() },
            first.copyOf().also { it[8] = 0; it[9] = 0 },
            first.copyOf().also { it[9] = (it[9] - 1).toByte() },
        )

        mutations.forEach { mutation ->
            assertThrows(FragmentFormatException::class.java) {
                GattFragmentCodec.accept(GattReassembly.Empty, mutation)
            }
        }

        val partial = GattFragmentCodec.accept(GattReassembly.Empty, first)
        assertThrows(FragmentFormatException::class.java) {
            GattFragmentCodec.accept(
                partial,
                second.copyOf().also { it[4] = 0; it[5] = 1 },
            )
        }
        assertThrows(FragmentFormatException::class.java) {
            GattFragmentCodec.accept(
                partial,
                second.copyOf().also { it[3] = 0 },
            )
        }
    }

    @Test
    fun `rejects empty oversized and impossible packet bounds`() {
        assertThrows(IllegalArgumentException::class.java) {
            GattFragmentCodec.encode(byteArrayOf(), 23)
        }
        assertThrows(IllegalArgumentException::class.java) {
            GattFragmentCodec.encode(
                ByteArray(GattFragmentCodec.maxFrameLength + 1),
                23,
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            GattFragmentCodec.encode(byteArrayOf(1), GattFragmentCodec.headerLength)
        }
        assertThrows(IllegalArgumentException::class.java) {
            GattFragmentCodec.encode(
                byteArrayOf(1),
                GattFragmentCodec.maxAttributeValueBytes + 1,
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            GattFragmentCodec.encodeWithPayloadSizes(byteArrayOf(1, 2), listOf(1))
        }
    }

    @Test
    fun `fragment header layout is fixed and canonical`() {
        val packets = GattFragmentCodec.encodeWithPayloadSizes(
            byteArrayOf(10, 11, 12),
            listOf(1, 2),
        )

        assertEquals(GattFragmentCodec.headerLength + 1, packets[0].size)
        assertEquals(GattFragmentCodec.headerLength + 2, packets[1].size)
        assertEquals(0, packets[0][3].toInt())
        assertEquals(1, packets[1][3].toInt())
        assertEquals(0, packets[0][6].toInt())
        assertEquals(0, packets[0][7].toInt())
        assertEquals(0, packets[1][6].toInt())
        assertEquals(1, packets[1][7].toInt())
    }
}
