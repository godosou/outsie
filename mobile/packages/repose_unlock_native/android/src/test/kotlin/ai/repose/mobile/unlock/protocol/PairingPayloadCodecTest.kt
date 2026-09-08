package ai.repose.mobile.unlock.protocol

import java.nio.file.Path
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class PairingPayloadCodecTest {
    @Test
    fun `android decodes and re-encodes the shared pairing fixture byte for byte`() {
        val fixture = fixture()

        val payload = PairingProtocolV1.decode(fixture)

        assertEquals(164, fixture.size)
        assertArrayEquals(fixture, payload.encoded())
        assertArrayEquals(SESSION_ID, payload.sessionId)
        assertEquals(1_800_000_000_000uL, payload.expiresAtEpochMillis)
        assertArrayEquals(MAC_ID, payload.macId)
        assertArrayEquals(MAC_PUBLIC_KEY, payload.macIdentityPublicKey)
        assertArrayEquals(PAIRING_SECRET, payload.pairingSecret)
        assertEquals("Repose MacBook Pro", payload.macName)
        assertArrayEquals(
            fixture,
            PairingProtocolV1.encode(
                payload.sessionId,
                payload.expiresAtEpochMillis,
                payload.macId,
                payload.macIdentityPublicKey,
                payload.pairingSecret,
                payload.macName,
            ),
        )
    }

    @Test
    fun `pairing parser rejects noncanonical lengths headers and trailing bytes`() {
        val fixture = fixture()
        val mutations = listOf(
            fixture.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() },
            fixture.copyOf().also { it[4] = 2 },
            fixture.copyOf().also { it[5] = 1 },
            fixture.copyOf().also { it[7] = (it[7] - 1).toByte() },
            fixture.copyOf().also { it[7] = (it[7] + 1).toByte() },
            fixture.copyOf().also { it[145] = 17 },
            fixture.copyOf(fixture.size - 1),
            fixture + byteArrayOf(0),
        )

        mutations.forEach { mutation ->
            assertThrows(PairingProtocolFormatException::class.java) {
                PairingProtocolV1.decode(mutation)
            }
        }
    }

    @Test
    fun `pairing codec rejects invalid identifiers key secret and utf8 name`() {
        val fixture = fixture()
        val mutations = listOf(
            fixture.copyOf().also { it.fill(0, 8, 24) },
            fixture.copyOf().also { it.fill(0, 24, 32) },
            fixture.copyOf().also { it.fill(0, 32, 48) },
            fixture.copyOf().also { it[48] = 3 },
            fixture.copyOf().also { it.fill(0, 48, 113); it[48] = 4 },
            fixture.copyOf().also { it.fill(0, 113, 145) },
            fixture.copyOf().also { it[146] = 0xff.toByte() },
            fixture.copyOf().also { it[146] = '\n'.code.toByte() },
        )

        mutations.forEach { mutation ->
            assertThrows(PairingProtocolFormatException::class.java) {
                PairingProtocolV1.decode(mutation)
            }
        }
    }

    @Test
    fun `pairing encoder enforces utf8 byte length instead of character count`() {
        assertThrows(PairingProtocolFormatException::class.java) {
            PairingProtocolV1.encode(
                SESSION_ID,
                1_800_000_000_000uL,
                MAC_ID,
                MAC_PUBLIC_KEY,
                PAIRING_SECRET,
                "钥".repeat(22),
            )
        }
    }

    @Test
    fun `gatt identifiers are fixed for both platforms`() {
        assertEquals("A53E0001-7A6B-4D59-9F2E-5245504F5345", PairingProtocolV1.bleServiceUuid)
        assertEquals(
            "A53E0002-7A6B-4D59-9F2E-5245504F5345",
            PairingProtocolV1.bleControlCharacteristicUuid,
        )
        assertEquals(
            "A53E0003-7A6B-4D59-9F2E-5245504F5345",
            PairingProtocolV1.bleStatusCharacteristicUuid,
        )
        assertEquals(210, PairingProtocolV1.maxFrameLength)
    }

    private fun fixture(): ByteArray {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        return root.resolve("protocol/fixtures/v1/pairing-payload.bin").toFile().readBytes()
    }

    private companion object {
        val SESSION_ID = hex("a0a1a2a3a4a5a6a7a8a9aaabacadaeaf")
        val MAC_ID = hex("000102030405060708090a0b0c0d0e0f")
        val MAC_PUBLIC_KEY = hex(
            "04e2534a3532d08fbba02dde659ee62bd0031fe2db785596ef509302446b030852" +
                "e0f1575a4c633cc719dfee5fda862d764efc96c3f30ee0055c42c23f184ed8c6",
        )
        val PAIRING_SECRET = hex(
            "c0c1c2c3c4c5c6c7c8c9cacbcccdcecfd0d1d2d3d4d5d6d7d8d9dadbdcdddedf",
        )

        fun hex(value: String): ByteArray = value.chunked(2)
            .map { it.toInt(16).toByte() }
            .toByteArray()
    }
}
