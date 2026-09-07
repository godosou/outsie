package ai.repose.mobile.unlock.protocol

import java.lang.reflect.Modifier
import java.math.BigInteger
import java.nio.file.Path
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.PublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ContractVectorTest {
    @Test
    fun `challenge fixture decodes and re-encodes byte for byte`() {
        val fixture = fixtureBytes("challenge_frame")

        val challenge = ProtocolV1.decodeChallenge(fixture)

        assertEquals(249, fixture.size)
        assertArrayEquals(fixture, challenge.encoded())
        assertEquals(7uL, challenge.pairingGeneration)
        assertEquals(501u, challenge.consoleUid)
        assertEquals(0x1234_5678u, challenge.auditSessionId)
        assertEquals(0x0102_0304_0506_0708uL, challenge.lockEpoch)
        assertEquals(1uL, challenge.challengeId)
        assertEquals(41uL, challenge.counterFloor)
        assertEquals(5_000u, challenge.ttlMillis)
        assertArrayEquals(fixtureBytes("mac_id"), challenge.macId)
        assertArrayEquals(fixtureBytes("device_id"), challenge.deviceId)
        assertArrayEquals(fixtureBytes("mac_nonce"), challenge.macNonce)
    }

    @Test
    fun `response fixture decodes and re-encodes byte for byte`() {
        val fixture = fixtureBytes("response_frame")

        val response = ProtocolV1.decodeResponse(fixture)

        assertEquals(390, fixture.size)
        assertArrayEquals(fixture, response.encoded())
        assertEquals(7uL, response.pairingGeneration)
        assertEquals(1uL, response.challengeId)
        assertEquals(42uL, response.counter)
        assertArrayEquals(fixtureBytes("response_ciphertext"), response.ciphertext)
        assertArrayEquals(fixtureBytes("response_tag"), response.tag)
        assertArrayEquals(fixtureBytes("response_signature_raw_low_s"), response.signature)
    }

    @Test
    fun `v1 parser rejects every noncanonical header class`() {
        val valid = fixtureBytes("challenge_frame")
        val mutations = listOf(
            valid.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() },
            valid.copyOf().also { it[4] = 2 },
            valid.copyOf().also { it[5] = 2 },
            valid.copyOf().also { it[5] = 0x7f },
            valid.copyOf().also { it[7] = 1 },
            valid.copyOf().also { it[11] = (it[11] - 1).toByte() },
            valid.copyOf().also { it[8] = 0; it[9] = 0; it[10] = 1; it[11] = 123 },
            valid.copyOf().also { it[84] = 0; it[85] = 0; it[86] = 0; it[87] = 0 },
            valid.copyOf().also { it[120] = 3 },
            valid.copyOf(valid.size - 1),
            valid + byteArrayOf(0),
        )

        mutations.forEach { mutation ->
            assertThrows(ProtocolFormatException::class.java) {
                ProtocolV1.decodeChallenge(mutation)
            }
        }
    }

    @Test
    fun `challenge parser rejects noncanonical raw signature scalars`() {
        assertRejectsNonCanonicalSignature(
            fixture = fixtureBytes("challenge_frame"),
            signatureOffset = 185,
            decode = ProtocolV1::decodeChallenge,
        )
    }

    @Test
    fun `response parser rejects noncanonical raw signature scalars`() {
        assertRejectsNonCanonicalSignature(
            fixture = fixtureBytes("response_frame"),
            signatureOffset = 326,
            decode = ProtocolV1::decodeResponse,
        )
    }

    @Test
    fun `phone response signing request hashes only the canonical v1 transcript`() {
        val challenge = verifiedChallenge()
        val responsePrefix = fixtureBytes("response_frame").copyOfRange(0, 326)

        val request = phoneResponseSigningRequest(challenge, responsePrefix)

        assertTrue(request.javaClass.interfaces.isEmpty())
        assertTrue(
            request.javaClass.declaredConstructors.filterNot { it.isSynthetic }.all { constructor ->
                Modifier.isPrivate(constructor.modifiers)
            },
        )
        assertArrayEquals(
            fixtureBytes("signature_transcript_hash"),
            requestPrehash(request),
        )
    }

    @Test
    fun `phone response signing request rejects malformed or mismatched prefixes`() {
        val challenge = verifiedChallenge()
        val responsePrefix = fixtureBytes("response_frame").copyOfRange(0, 326)
        val mutations = listOf(
            responsePrefix.copyOf(responsePrefix.size - 1),
            responsePrefix.copyOf().also { it[5] = 1 },
            responsePrefix.copyOf().also { it[68] = (it[68].toInt() xor 1).toByte() },
            responsePrefix.copyOf().also { it[84] = (it[84].toInt() xor 1).toByte() },
            responsePrefix.copyOf().also { it[148] = 3 },
            responsePrefix.copyOf().also { it[213] = 3 },
        )

        mutations.forEach { mutation ->
            assertThrows(ProtocolFormatException::class.java) {
                phoneResponseSigningRequest(challenge, mutation)
            }
        }
    }

    private fun phoneResponseSigningRequest(
        challenge: VerifiedMacChallenge,
        responsePrefix: ByteArray,
    ): PhoneResponseSigningRequest =
        ProtocolV1.phoneResponseSigningRequest(challenge, responsePrefix)

    private fun verifiedChallenge(): VerifiedMacChallenge = ChallengeVerifier.verify(
        PairedMacRecord(
            fixtureBytes("mac_id"),
            fixtureBytes("device_id"),
            7L,
            publicKey(fixtureBytes("mac_signing_public_key")),
        ),
        fixtureBytes("challenge_frame"),
    )

    private fun publicKey(raw: ByteArray): PublicKey {
        require(raw.size == 65 && raw[0] == 4.toByte())
        val parameters = AlgorithmParameters.getInstance("EC").apply {
            init(ECGenParameterSpec("secp256r1"))
        }.getParameterSpec(ECParameterSpec::class.java)
        val point = ECPoint(
            BigInteger(1, raw.copyOfRange(1, 33)),
            BigInteger(1, raw.copyOfRange(33, 65)),
        )
        return KeyFactory.getInstance("EC").generatePublic(ECPublicKeySpec(point, parameters))
    }

    private fun requestPrehash(request: PhoneResponseSigningRequest): ByteArray =
        request.copyPrehashForSigner()

    private fun assertRejectsNonCanonicalSignature(
        fixture: ByteArray,
        signatureOffset: Int,
        decode: (ByteArray) -> Any,
    ) {
        val rOffset = signatureOffset
        val sOffset = signatureOffset + SCALAR_LENGTH
        val mutations = listOf(
            "r = 0" to BigInteger.ZERO.atScalar(fixture, rOffset),
            "s = 0" to BigInteger.ZERO.atScalar(fixture, sOffset),
            "r = n" to P256_ORDER.atScalar(fixture, rOffset),
            "s = n" to P256_ORDER.atScalar(fixture, sOffset),
            "high-S" to P256_HALF_ORDER.add(BigInteger.ONE).atScalar(fixture, sOffset),
        )

        mutations.forEach { (description, mutation) ->
            assertThrows(description, ProtocolFormatException::class.java) {
                decode(mutation)
            }
        }
    }

    private fun BigInteger.atScalar(frame: ByteArray, offset: Int): ByteArray {
        val encoded = toByteArray()
        require(encoded.size <= SCALAR_LENGTH + 1)
        val unsigned = encoded.takeLast(SCALAR_LENGTH)
        return frame.copyOf().also { mutated ->
            mutated.fill(0, offset, offset + SCALAR_LENGTH)
            unsigned.forEachIndexed { index, byte ->
                mutated[offset + SCALAR_LENGTH - unsigned.size + index] = byte
            }
        }
    }

    private fun fixtureBytes(name: String): ByteArray {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val json = root.resolve("protocol/fixtures/v1/crypto-vectors.json").toFile().readText()
        val hex = requireNotNull(
            Regex("\\\"${Regex.escape(name)}\\\"\\s*:\\s*\\\"([0-9a-f]+)\\\"")
                .find(json)
                ?.groupValues
                ?.get(1),
        ) { "missing fixture field $name" }
        return hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }

    private companion object {
        const val SCALAR_LENGTH = 32
        val P256_ORDER = BigInteger(
            "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
            16,
        )
        val P256_HALF_ORDER: BigInteger = P256_ORDER.shiftRight(1)
    }
}
