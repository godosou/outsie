package ai.repose.mobile.unlock.crypto

import java.math.BigInteger
import java.nio.file.Path
import java.security.KeyPairGenerator
import java.security.MessageDigest
import java.security.PublicKey
import java.security.Signature
import java.security.spec.ECGenParameterSpec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CryptoPolicyTest {
    @Test
    fun `identity key policy is p256 sha256 and background usable`() {
        assertEquals("secp256r1", SigningKeyPolicy.curveName)
        assertEquals("SHA-256", SigningKeyPolicy.digestName)
        assertEquals("NONE", SigningKeyPolicy.keyStoreDigestName)
        assertFalse(SigningKeyPolicy.userAuthenticationRequired)
        assertFalse(SigningKeyPolicy.unlockedDeviceRequired)
    }

    @Test
    fun `only tee and strongbox satisfy hardware key policy`() {
        assertTrue(SigningKeyPolicy.acceptsSecurityLevel(1))
        assertTrue(SigningKeyPolicy.acceptsSecurityLevel(2))
        assertFalse(SigningKeyPolicy.acceptsSecurityLevel(0))
        assertFalse(SigningKeyPolicy.acceptsSecurityLevel(-1))
        assertFalse(SigningKeyPolicy.acceptsSecurityLevel(-2))
        assertFalse(SigningKeyPolicy.acceptsSecurityLevel(99))
    }

    @Test
    fun `complete generated hardware p256 signing policy is accepted`() {
        assertTrue(policyAccepts(validPolicy()))
    }

    @Test
    fun `rsa and non p256 ec keys are rejected`() {
        val invalidPolicies = listOf(
            "RSA private-key algorithm" to validPolicy().copy(privateKeyAlgorithm = "RSA"),
            "RSA public key" to validPolicy().copy(publicKey = rsaPublicKey),
            "P-384 public key" to validPolicy().copy(publicKey = p384PublicKey),
        )

        invalidPolicies.forEach { (description, policy) ->
            assertFalse(description, policyAccepts(policy))
        }
    }

    @Test
    fun `wrong key metadata is rejected one field at a time`() {
        val invalidPolicies = listOf(
            "wrong key size" to validPolicy().copy(keySize = 384),
            "missing verify purpose" to validPolicy().copy(purposes = PURPOSE_SIGN),
            "missing sign purpose" to validPolicy().copy(purposes = PURPOSE_VERIFY),
            "extra purpose" to validPolicy().copy(
                purposes = REQUIRED_PURPOSES or PURPOSE_AGREE_KEY,
            ),
            "imported origin" to validPolicy().copy(origin = ORIGIN_IMPORTED),
            "software security level" to validPolicy().copy(securityLevel = SECURITY_SOFTWARE),
            "unknown secure security level" to validPolicy().copy(securityLevel = -1),
            "double-hashing SHA-256 digest" to validPolicy().copy(digests = listOf("SHA-256")),
            "additional digest" to validPolicy().copy(digests = listOf("NONE", "SHA-256")),
            "user authentication" to validPolicy().copy(userAuthenticationRequired = true),
            "exportable private key" to validPolicy().copy(privateKeyEncoded = byteArrayOf(1)),
            "missing or mismatched trusted marker" to validPolicy().copy(
                trustedPolicyMarkerMatches = false,
            ),
        )

        invalidPolicies.forEach { (description, policy) ->
            assertFalse(description, policyAccepts(policy))
        }
    }

    @Test
    fun `p256 der conversion emits fixed width low s raw signature`() {
        val highSDer = derSignature(BigInteger.ONE, P256_ORDER.subtract(BigInteger.ONE))

        val raw = canonicalRawFromDer(highSDer)

        assertEquals(64, raw.size)
        assertEquals(BigInteger.ONE, BigInteger(1, raw.copyOfRange(0, 32)))
        assertEquals(BigInteger.ONE, BigInteger(1, raw.copyOfRange(32, 64)))
    }

    @Test
    fun `canonical raw signature round trips through none with ecdsa verification`() {
        val keyPair = KeyPairGenerator.getInstance("EC").apply {
            initialize(ECGenParameterSpec("secp256r1"))
        }.generateKeyPair()
        val prehash = MessageDigest.getInstance("SHA-256")
            .digest("exactly-once prehash".toByteArray())
        val providerDer = Signature.getInstance("NONEwithECDSA").run {
            initSign(keyPair.private)
            update(prehash)
            sign()
        }

        val raw = canonicalRawFromDer(providerDer)
        val canonicalDer = derFromRaw(raw)

        assertEquals(64, raw.size)
        assertTrue(
            Signature.getInstance("NONEwithECDSA").run {
                initVerify(keyPair.public)
                update(prehash)
                verify(canonicalDer)
            },
        )
        val s = BigInteger(1, raw.copyOfRange(32, 64))
        assertTrue(s.signum() > 0)
        assertTrue(s <= P256_HALF_ORDER)
    }

    @Test
    fun `android signer source exposes only purpose specific prehash signing`() {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val source = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/crypto/AndroidKeyStoreSigner.kt",
        ).toFile().readText()

        assertTrue(source.contains("signProtocolV1PhoneResponsePrehash"))
        assertTrue(source.contains("PhoneResponseSigningRequest"))
        assertTrue(source.contains("PhoneResponseSignature"))
        assertTrue(
            Regex(
                "signProtocolV1PhoneResponsePrehash\\s*\\(\\s*" +
                    "request: PhoneResponseSigningRequest,?\\s*\\)" +
                    "\\s*:\\s*PhoneResponseSignature",
            ).containsMatchIn(source),
        )
        assertTrue(source.contains("NONEwithECDSA"))
        assertTrue(source.contains("KeyProperties.DIGEST_NONE"))
        assertFalse(source.contains("SHA256withECDSA"))
        assertFalse(source.contains("signProtocolV1PhoneResponsePrehash(prehash: ByteArray)"))
        assertFalse(source.contains("fun getOrCreate(alias:"))
        assertFalse(source.contains("GeneratedSigningKey"))

        val schema = root.resolve(
            "mobile/packages/repose_unlock_native/pigeons/repose_unlock_api.dart",
        ).toFile().readText()
        assertFalse(Regex("\\bsign(?:Digest|Prehash|Bytes)?\\s*\\(").containsMatchIn(schema))
    }

    private fun validPolicy(): TestPolicy = TestPolicy(
        privateKeyAlgorithm = "EC",
        privateKeyEncoded = null,
        publicKey = p256PublicKey,
        keySize = 256,
        purposes = REQUIRED_PURPOSES,
        origin = ORIGIN_GENERATED,
        securityLevel = SECURITY_TEE,
        digests = listOf("NONE"),
        userAuthenticationRequired = false,
        trustedPolicyMarkerMatches = true,
    )

    private fun policyAccepts(policy: TestPolicy): Boolean = SigningKeyPolicy.accepts(
        SigningKeyPolicySnapshot(
            privateKeyAlgorithm = policy.privateKeyAlgorithm,
            privateKeyEncoded = policy.privateKeyEncoded,
            publicKey = policy.publicKey,
            keySize = policy.keySize,
            purposes = policy.purposes,
            origin = policy.origin,
            securityLevel = policy.securityLevel,
            digests = policy.digests.toTypedArray(),
            userAuthenticationRequired = policy.userAuthenticationRequired,
            trustedPolicyMarkerMatches = policy.trustedPolicyMarkerMatches,
        ),
    )

    private fun ecPublicKey(curveName: String): PublicKey =
        KeyPairGenerator.getInstance("EC").apply {
            initialize(ECGenParameterSpec(curveName))
        }.generateKeyPair().public

    private fun canonicalRawFromDer(der: ByteArray): ByteArray =
        P256SignatureCodec.canonicalRawFromDer(der)

    private fun derFromRaw(raw: ByteArray): ByteArray = P256SignatureCodec.derFromRaw(raw)

    private fun derSignature(r: BigInteger, s: BigInteger): ByteArray {
        val encodedR = r.toByteArray()
        val encodedS = s.toByteArray()
        val bodyLength = 2 + encodedR.size + 2 + encodedS.size
        return byteArrayOf(0x30, bodyLength.toByte(), 0x02, encodedR.size.toByte()) +
            encodedR + byteArrayOf(0x02, encodedS.size.toByte()) + encodedS
    }

    private data class TestPolicy(
        val privateKeyAlgorithm: String,
        val privateKeyEncoded: ByteArray?,
        val publicKey: PublicKey,
        val keySize: Int,
        val purposes: Int,
        val origin: Int,
        val securityLevel: Int,
        val digests: List<String>,
        val userAuthenticationRequired: Boolean,
        val trustedPolicyMarkerMatches: Boolean,
    )

    private val p256PublicKey: PublicKey by lazy { ecPublicKey("secp256r1") }
    private val p384PublicKey: PublicKey by lazy { ecPublicKey("secp384r1") }
    private val rsaPublicKey: PublicKey by lazy {
        KeyPairGenerator.getInstance("RSA").apply { initialize(2048) }.generateKeyPair().public
    }

    private companion object {
        const val PURPOSE_SIGN = 4
        const val PURPOSE_VERIFY = 8
        const val PURPOSE_AGREE_KEY = 64
        const val REQUIRED_PURPOSES = PURPOSE_SIGN or PURPOSE_VERIFY
        const val ORIGIN_GENERATED = 1
        const val ORIGIN_IMPORTED = 2
        const val SECURITY_SOFTWARE = 0
        const val SECURITY_TEE = 1
        val P256_ORDER = BigInteger(
            "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
            16,
        )
        val P256_HALF_ORDER: BigInteger = P256_ORDER.shiftRight(1)
    }
}
