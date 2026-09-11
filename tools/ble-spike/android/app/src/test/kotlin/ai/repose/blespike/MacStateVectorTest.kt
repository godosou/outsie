package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * The same known answers presence-verify.swift checks itself against.
 *
 * The Mac mints these tags and the phone recomputes them; if the two build the
 * pre-image even slightly differently — a field in the wrong order, a mac id
 * written little-endian — every beacon fails to verify and the phone silently
 * shows「不知道」. That is indistinguishable on screen from being out of range,
 * which is why it needs a test rather than a walk across the room.
 *
 * The expected values were computed independently with Python's hmac, not read
 * out of either implementation: a vector produced by the code it checks agrees
 * with itself no matter what either of them says.
 */
class MacStateVectorTest {

    private val keyHex = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"

    /** The phone's side of the pre-image, written out the way the scanner does. */
    private fun tag(keyId: Int, macId: Int, counter: Long, locked: Boolean): String {
        val msg = SpikeContract.MAC_STATE_LABEL.toByteArray(Charsets.US_ASCII) +
            byteArrayOf(
                keyId.toByte(),
                ((macId shr 8) and 0xFF).toByte(),
                (macId and 0xFF).toByte(),
            ) +
            PresenceBeacon.beLong(counter) +
            byteArrayOf(if (locked) 1 else 0)
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(hex(keyHex), "HmacSHA256"))
        return mac.doFinal(msg).copyOf(SpikeContract.TAG_LEN)
            .joinToString("") { "%02x".format(it) }
    }

    private fun hex(s: String) = ByteArray(s.length / 2) {
        ((Character.digit(s[it * 2], 16) shl 4) or Character.digit(s[it * 2 + 1], 16)).toByte()
    }

    @Test fun `locked matches the Mac`() {
        assertEquals("d16d729bc487b663", tag(1, 0xABCD, 58_000_000L, true))
    }

    @Test fun `unlocked matches the Mac`() {
        assertEquals("ee8a24179231a934", tag(1, 0xABCD, 58_000_000L, false))
    }

    @Test fun `a different Mac produces a different tag`() {
        // If this ever equals the vector above, the mac id has fallen out of the
        // authenticated message and become a label anyone can set.
        assertEquals("eedf1ad32139ab89", tag(1, 0x0001, 58_000_000L, true))
        assertNotEquals(tag(1, 0xABCD, 58_000_000L, true), tag(1, 0x0001, 58_000_000L, true))
    }

    @Test fun `the label is not the presence beacon's`() {
        // A tag minted for "this Mac is unlocked" must never also verify as
        // "the phone is present", or a recording of one forges the other.
        assertNotEquals(SpikeContract.MAC_STATE_LABEL, SpikeContract.PRESENCE_BEACON_LABEL)
    }
}
