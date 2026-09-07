package ai.repose.mobile.unlock.responder

import ai.repose.mobile.unlock.protocol.PhoneResponseSigningRequest
import java.math.BigInteger
import java.nio.file.Path
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.KeyPair
import java.security.PrivateKey
import java.security.PublicKey
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPrivateKeySpec
import java.security.spec.ECPublicKeySpec
import javax.crypto.KeyAgreement
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneResponseBuilderTest {
    @Test
    fun `authenticated fixture builds the exact Rust response bytes`() {
        val pairing = fixturePairing()
        val repository = MemoryPairedMacRepository(listOf(pairing))
        val challenge = TrustedChallengeAuthenticator.authenticate(
            repository,
            fixtureBytes("challenge_frame"),
        )
        val signer = FixtureIdentitySigner(
            pairing.phoneIdentityPublicKey,
            fixtureBytes("signature_transcript_hash"),
            fixtureBytes("response_signature_raw_low_s"),
        )
        val entropy = FixedResponseEntropy(
            KeyPair(
                publicKey(fixtureBytes("phone_ephemeral_public_key")),
                privateKey(fixtureBytes("phone_ephemeral_private_key_test_only")),
            ),
            fixtureBytes("phone_nonce"),
        )

        val response = PhoneResponseBuilder(entropy).build(
            challenge = challenge,
            previousCounter = 0uL,
            signer = signer,
        )

        assertArrayEquals(fixtureBytes("response_frame"), response)
        assertEquals(1, signer.calls)
        assertTrue(entropy.lastAgreementClosed)
    }

    @Test
    fun `pairing generation uses the full unsigned wire domain including zero`() {
        val fixture = fixturePairing()

        val generationZero = TrustedPairedMac(
            fixture.macId,
            fixture.deviceId,
            0uL,
            fixture.macIdentityPublicKey,
            fixture.phoneIdentityPublicKey,
        )

        assertEquals(0uL, generationZero.pairingGeneration)
    }

    @Test
    fun `trusted pairing rejects an off curve public point even with P256 parameters`() {
        val fixture = fixturePairing()
        val offCurve = object : ECPublicKey {
            override fun getAlgorithm(): String = "EC"
            override fun getFormat(): String = "X.509"
            override fun getEncoded(): ByteArray = ByteArray(0)
            override fun getW(): ECPoint = ECPoint(BigInteger.ONE, BigInteger.ONE)
            override fun getParams(): ECParameterSpec = p256Parameters()
        }

        assertThrows(ResponderUnavailableException::class.java) {
            TrustedPairedMac(
                fixture.macId,
                fixture.deviceId,
                fixture.pairingGeneration,
                offCurve,
                fixture.phoneIdentityPublicKey,
            )
        }
    }

    @Test
    fun `trusted pairing rejects point at infinity without leaking a provider exception`() {
        val fixture = fixturePairing()
        val infinity = object : ECPublicKey {
            override fun getAlgorithm(): String = "EC"
            override fun getFormat(): String = "X.509"
            override fun getEncoded(): ByteArray = ByteArray(0)
            override fun getW(): ECPoint = ECPoint.POINT_INFINITY
            override fun getParams(): ECParameterSpec = p256Parameters()
        }

        assertThrows(ResponderUnavailableException::class.java) {
            TrustedPairedMac(
                fixture.macId,
                fixture.deviceId,
                fixture.pairingGeneration,
                infinity,
                fixture.phoneIdentityPublicKey,
            )
        }
    }

    @Test
    fun `HKDF derives both reserved and active directions from the Rust fixture`() {
        val material = SessionKeyDerivation.derive(
            sharedSecret = fixtureBytes("ecdh_shared_secret"),
            challengeFrame = fixtureBytes("challenge_frame"),
            responsePrefix = fixtureBytes("response_frame").copyOfRange(0, 278),
        )

        material.use {
            assertArrayEquals(fixtureBytes("mac_to_phone_key"), it.copyMacToPhoneKey())
            assertArrayEquals(fixtureBytes("mac_to_phone_nonce"), it.copyMacToPhoneNonce())
            assertArrayEquals(fixtureBytes("phone_to_mac_key"), it.copyPhoneToMacKey())
            assertArrayEquals(fixtureBytes("phone_to_mac_nonce"), it.copyPhoneToMacNonce())
        }
        assertThrows(ResponderUnavailableException::class.java) {
            material.copyPhoneToMacKey()
        }
    }

    @Test
    fun `tampered AEAD output fails proof round trip before the phone identity signs`() {
        val pairing = fixturePairing()
        val challenge = TrustedChallengeAuthenticator.authenticate(
            MemoryPairedMacRepository(listOf(pairing)),
            fixtureBytes("challenge_frame"),
        )
        val signer = FixtureIdentitySigner(
            pairing.phoneIdentityPublicKey,
            fixtureBytes("signature_transcript_hash"),
            fixtureBytes("response_signature_raw_low_s"),
        )
        val tamperingEncryptor = object : ResponseAeadEncryptor {
            override fun encrypt(
                key: ByteArray,
                nonce: ByteArray,
                aad: ByteArray,
                plaintext: ByteArray,
            ): ByteArray = (fixtureBytes("response_ciphertext") + fixtureBytes("response_tag"))
                .also { it[0] = (it[0].toInt() xor 1).toByte() }
        }

        val entropy = FixedResponseEntropy(
            KeyPair(
                publicKey(fixtureBytes("phone_ephemeral_public_key")),
                privateKey(fixtureBytes("phone_ephemeral_private_key_test_only")),
            ),
            fixtureBytes("phone_nonce"),
        )
        assertThrows(ResponderUnavailableException::class.java) {
            PhoneResponseBuilder(
                entropy,
                tamperingEncryptor,
            ).build(challenge, 0uL, signer)
        }
        assertEquals(0, signer.calls)
        assertTrue(entropy.lastAgreementClosed)
    }

    @Test
    fun `response is withheld when ephemeral key destruction cannot be confirmed`() {
        val pairing = fixturePairing()
        val challenge = TrustedChallengeAuthenticator.authenticate(
            MemoryPairedMacRepository(listOf(pairing)),
            fixtureBytes("challenge_frame"),
        )
        val entropy = FixedResponseEntropy(
            KeyPair(
                publicKey(fixtureBytes("phone_ephemeral_public_key")),
                privateKey(fixtureBytes("phone_ephemeral_private_key_test_only")),
            ),
            fixtureBytes("phone_nonce"),
            failClose = true,
        )

        assertThrows(ResponderUnavailableException::class.java) {
            PhoneResponseBuilder(entropy).build(
                challenge,
                0uL,
                FixtureIdentitySigner(
                    pairing.phoneIdentityPublicKey,
                    fixtureBytes("signature_transcript_hash"),
                    fixtureBytes("response_signature_raw_low_s"),
                ),
            )
        }
    }

    private class FixedResponseEntropy(
        private val keyPair: KeyPair,
        private val nonce: ByteArray,
        private val failClose: Boolean = false,
    ) : ResponseEntropy {
        var lastAgreementClosed = false
            private set

        override fun openEphemeralAgreement(): EphemeralAgreement = object : EphemeralAgreement {
            override val publicKey: PublicKey = keyPair.public

            override fun deriveSharedSecret(peerPublicKey: PublicKey): ByteArray =
                KeyAgreement.getInstance("ECDH").run {
                    init(keyPair.private)
                    doPhase(peerPublicKey, true)
                    generateSecret()
                }

            override fun close() {
                lastAgreementClosed = true
                if (failClose) throw ResponderUnavailableException()
            }
        }

        override fun fillPhoneNonce(output: ByteArray) {
            nonce.copyInto(output)
        }
    }

    private class FixtureIdentitySigner(
        override val publicKey: PublicKey,
        private val expectedPrehash: ByteArray,
        private val signature: ByteArray,
    ) : PhoneIdentitySigner {
        var calls = 0
            private set

        override fun sign(request: PhoneResponseSigningRequest): ByteArray {
            calls += 1
            assertArrayEquals(expectedPrehash, request.copyPrehashForSigner())
            return signature.copyOf()
        }
    }

    private fun fixturePairing(): TrustedPairedMac = TrustedPairedMac(
        macId = fixtureBytes("mac_id"),
        deviceId = fixtureBytes("device_id"),
        pairingGeneration = 7uL,
        macIdentityPublicKey = publicKey(fixtureBytes("mac_signing_public_key")),
        phoneIdentityPublicKey = publicKey(fixtureBytes("phone_signing_public_key")),
    )

    private fun publicKey(raw: ByteArray): PublicKey {
        require(raw.size == 65 && raw[0] == 4.toByte())
        val point = ECPoint(
            BigInteger(1, raw.copyOfRange(1, 33)),
            BigInteger(1, raw.copyOfRange(33, 65)),
        )
        return KeyFactory.getInstance("EC").generatePublic(ECPublicKeySpec(point, p256Parameters()))
    }

    private fun privateKey(raw: ByteArray): PrivateKey = KeyFactory.getInstance("EC")
        .generatePrivate(ECPrivateKeySpec(BigInteger(1, raw), p256Parameters()))

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
