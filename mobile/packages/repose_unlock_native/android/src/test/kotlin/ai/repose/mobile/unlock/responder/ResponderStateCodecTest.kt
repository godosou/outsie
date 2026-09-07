package ai.repose.mobile.unlock.responder

import java.math.BigInteger
import java.nio.file.Path
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.PublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class ResponderStateCodecTest {
    @Test
    fun `cached fixture state round trips with complete authority and exact response`() {
        val record = DurableResponderRecord.Active(
            fixturePairing(),
            DurableResponseState.Cached(
                counter = 42uL,
                consoleUid = 501u,
                auditSessionId = 0x1234_5678u,
                lockEpoch = 0x0102_0304_0506_0708uL,
                challengeId = 1uL,
                challengeFingerprint = java.security.MessageDigest.getInstance("SHA-256")
                    .digest(fixtureBytes("challenge_frame")),
                response = fixtureBytes("response_frame"),
            ),
        )

        val encoded = ResponderStateCodec.encode(record)

        assertEquals(record, ResponderStateCodec.decode(encoded))
    }

    @Test
    fun `unsigned generation and counter survive at the u64 maximum`() {
        val pairing = fixturePairing(generation = ULong.MAX_VALUE)
        val ready = DurableResponderRecord.Active(
            pairing,
            DurableResponseState.Ready(ULong.MAX_VALUE),
        )
        val revoked = DurableResponderRecord.Revoked(
            pairing.macId,
            pairing.deviceId,
            ULong.MAX_VALUE,
        )

        assertEquals(ready, ResponderStateCodec.decode(ResponderStateCodec.encode(ready)))
        assertEquals(revoked, ResponderStateCodec.decode(ResponderStateCodec.encode(revoked)))
    }

    @Test
    fun `truncated trailing and corrupted durable records fail closed`() {
        val valid = ResponderStateCodec.encode(
            DurableResponderRecord.Active(fixturePairing(), DurableResponseState.Ready()),
        )
        val mutations = listOf(
            valid.copyOf(valid.size - 1),
            valid + byteArrayOf(0),
            valid.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() },
            valid.copyOf().also { it[50] = (it[50].toInt() xor 1).toByte() },
        )

        mutations.forEach { mutation ->
            assertThrows(ResponderStoreException::class.java) {
                ResponderStateCodec.decode(mutation)
            }
        }
    }

    private fun fixturePairing(generation: ULong = 7uL): TrustedPairedMac = TrustedPairedMac(
        macId = fixtureBytes("mac_id"),
        deviceId = fixtureBytes("device_id"),
        pairingGeneration = generation,
        macIdentityPublicKey = publicKey(fixtureBytes("mac_signing_public_key")),
        phoneIdentityPublicKey = publicKey(fixtureBytes("phone_signing_public_key")),
    )

    private fun publicKey(raw: ByteArray): PublicKey {
        val point = ECPoint(
            BigInteger(1, raw.copyOfRange(1, 33)),
            BigInteger(1, raw.copyOfRange(33, 65)),
        )
        return KeyFactory.getInstance("EC").generatePublic(ECPublicKeySpec(point, p256Parameters()))
    }

    private fun p256Parameters(): ECParameterSpec = AlgorithmParameters.getInstance("EC").run {
        init(ECGenParameterSpec("secp256r1"))
        getParameterSpec(ECParameterSpec::class.java)
    }

    private fun fixtureBytes(name: String): ByteArray {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val json = root.resolve("protocol/fixtures/v1/crypto-vectors.json").toFile().readText()
        val hex = requireNotNull(
            Regex("\\\"${Regex.escape(name)}\\\"\\s*:\\s*\\\"([0-9a-f]+)\\\"")
                .find(json)
                ?.groupValues
                ?.get(1),
        )
        return hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }
}
