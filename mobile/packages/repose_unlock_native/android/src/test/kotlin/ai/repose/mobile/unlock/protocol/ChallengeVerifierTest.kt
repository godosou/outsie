package ai.repose.mobile.unlock.protocol

import java.lang.reflect.Modifier
import java.math.BigInteger
import java.nio.file.Path
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.PublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ChallengeVerifierTest {
    @Test
    fun `capability bytecode has no public constructor or seal accessor`() {
        assertTrue(
            "verified challenge must have no public JVM constructor",
            VerifiedMacChallenge::class.java.constructors.isEmpty(),
        )
        assertTrue(
            "signing request must have no public JVM constructor",
            PhoneResponseSigningRequest::class.java.constructors.isEmpty(),
        )
        assertTrue(
            "trusted paired record must have no public JVM constructor",
            PairedMacRecord::class.java.constructors.isEmpty(),
        )
        assertThrows(NoSuchMethodException::class.java) {
            VerifiedMacChallenge::class.java.getMethod("access\$getVerificationSeal\$cp")
        }
        assertThrows(NoSuchMethodException::class.java) {
            PhoneResponseSigningRequest::class.java.getMethod("access\$getSigningRequestSeal\$cp")
        }
    }

    @Test
    fun `paired record verifies the fixture and mints the only response signing capability`() {
        val sourceMacId = fixtureBytes("mac_id")
        val sourceDeviceId = fixtureBytes("device_id")
        val record = PairedMacRecord(
            sourceMacId,
            sourceDeviceId,
            7L,
            publicKey(fixtureBytes("mac_signing_public_key")),
        )
        sourceMacId.fill(0)
        sourceDeviceId.fill(0)

        val verified = ChallengeVerifier.verify(
            record,
            fixtureBytes("challenge_frame"),
        )
        val signingRequest = ProtocolV1.phoneResponseSigningRequest(
            verified,
            fixtureBytes("response_frame").copyOfRange(0, 326),
        )

        assertArrayEquals(
            fixtureBytes("signature_transcript_hash"),
            signingRequest.copyPrehashForSigner(),
        )
        val verifiedConstructor = VerifiedMacChallenge::class.java.declaredConstructors.single()
        assertTrue(Modifier.isPrivate(verifiedConstructor.modifiers))
        assertFalse(verifiedConstructor.isSynthetic)
        val factory = ProtocolV1::class.java.declaredMethods.single { method ->
            method.name.startsWith("phoneResponseSigningRequest")
        }
        assertEquals(VerifiedMacChallenge::class.java, factory.parameterTypes[0])
        assertEquals(ByteArray::class.java, factory.parameterTypes[1])

        val signingRequestConstructor = PhoneResponseSigningRequest::class.java
            .declaredConstructors
            .single()
        assertTrue(Modifier.isPrivate(signingRequestConstructor.modifiers))
        assertFalse(signingRequestConstructor.isSynthetic)
    }

    @Test
    fun `wrong paired key ids or generation cannot mint a verified challenge`() {
        val validMacId = fixtureBytes("mac_id")
        val validDeviceId = fixtureBytes("device_id")
        val validKey = publicKey(fixtureBytes("mac_signing_public_key"))
        val wrongKey = KeyPairGenerator.getInstance("EC").apply {
            initialize(ECGenParameterSpec("secp256r1"))
        }.generateKeyPair().public
        val records = listOf(
            pairedRecord(macIdentityPublicKey = wrongKey),
            pairedRecord(macId = validMacId.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() }),
            pairedRecord(
                deviceId = validDeviceId.copyOf().also {
                    it[0] = (it[0].toInt() xor 1).toByte()
                },
            ),
            pairedRecord(pairingGeneration = 8uL),
        )

        records.forEach { record ->
            assertThrows(ChallengeVerificationException::class.java) {
                ChallengeVerifier.verify(record, fixtureBytes("challenge_frame"))
            }
        }

        // Keep the valid key referenced so this test also detects an accidental caller-key API.
        assertEquals("EC", validKey.algorithm)
    }

    @Test
    fun `malformed changed or noncanonical challenges fail before capability creation`() {
        val valid = fixtureBytes("challenge_frame")
        val mutations = listOf(
            valid.copyOf(valid.size - 1),
            valid.copyOf().also { it[55] = (it[55].toInt() xor 1).toByte() },
            valid.copyOf().also { it[185] = (it[185].toInt() xor 1).toByte() },
            P256_HALF_ORDER.add(BigInteger.ONE).atScalar(valid, 217),
        )

        mutations.forEach { mutation ->
            assertThrows(ChallengeVerificationException::class.java) {
                ChallengeVerifier.verify(pairedRecord(), mutation)
            }
        }
    }

    @Test
    fun `source boundary exposes no parsed challenge or pigeon signing bypass`() {
        val root = workspaceRoot()
        val protocolSource = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/protocol/ProtocolV1.kt",
        ).toFile().readText()
        val verifierSource = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/protocol/ChallengeVerifier.kt",
        ).toFile().readText()
        val verifiedCapabilitySource = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/java/" +
                "ai/repose/mobile/unlock/protocol/VerifiedMacChallenge.java",
        ).toFile().readText()
        val signingCapabilitySource = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/java/" +
                "ai/repose/mobile/unlock/protocol/PhoneResponseSigningRequest.java",
        ).toFile().readText()
        val pigeonSchema = root.resolve(
            "mobile/packages/repose_unlock_native/pigeons/repose_unlock_api.dart",
        ).toFile().readText()

        assertTrue(protocolSource.contains("verifiedChallenge: VerifiedMacChallenge"))
        assertFalse(
            Regex("phoneResponseSigningRequest\\s*\\(\\s*challenge:\\s*ChallengeFrame")
                .containsMatchIn(protocolSource),
        )
        assertTrue(verifiedCapabilitySource.contains("private VerifiedMacChallenge("))
        assertTrue(signingCapabilitySource.contains("private PhoneResponseSigningRequest("))
        assertFalse(verifiedCapabilitySource.contains("Seal"))
        assertFalse(signingCapabilitySource.contains("Seal"))
        assertTrue(verifierSource.contains("P256SignatureCodec.derFromRaw(signature)"))
        assertTrue(verifierSource.contains("Signature.getInstance(\"NONEwithECDSA\")"))
        assertTrue(
            verifierSource.contains(
                "repose-unlock-v1 signature mac-to-phone challenge",
            ),
        )
        listOf("VerifiedMacChallenge", "PairedMacRecord", "ChallengeVerifier", "signPrehash")
            .forEach { forbidden -> assertFalse(pigeonSchema.contains(forbidden)) }
    }

    private fun pairedRecord(
        macId: ByteArray = fixtureBytes("mac_id"),
        deviceId: ByteArray = fixtureBytes("device_id"),
        pairingGeneration: ULong = 7uL,
        macIdentityPublicKey: PublicKey = publicKey(fixtureBytes("mac_signing_public_key")),
    ): PairedMacRecord = PairedMacRecord(
        macId,
        deviceId,
        pairingGeneration.toLong(),
        macIdentityPublicKey,
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

    private fun BigInteger.atScalar(frame: ByteArray, offset: Int): ByteArray {
        val encoded = toByteArray().takeLast(SCALAR_LENGTH)
        return frame.copyOf().also { mutated ->
            mutated.fill(0, offset, offset + SCALAR_LENGTH)
            encoded.forEachIndexed { index, byte ->
                mutated[offset + SCALAR_LENGTH - encoded.size + index] = byte
            }
        }
    }

    private fun fixtureBytes(name: String): ByteArray {
        val root = workspaceRoot()
        val json = root.resolve("protocol/fixtures/v1/crypto-vectors.json").toFile().readText()
        val hex = requireNotNull(
            Regex("\\\"${Regex.escape(name)}\\\"\\s*:\\s*\\\"([0-9a-f]+)\\\"")
                .find(json)
                ?.groupValues
                ?.get(1),
        )
        return hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }

    private fun workspaceRoot(): Path =
        Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))

    private companion object {
        const val SCALAR_LENGTH = 32
        val P256_ORDER = BigInteger(
            "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
            16,
        )
        val P256_HALF_ORDER: BigInteger = P256_ORDER.shiftRight(1)
    }
}
