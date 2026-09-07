@file:Suppress("DEPRECATION") // RequiresDevice is still the runner's physical-device filter.

package ai.repose.mobile.unlock.protocol

import android.content.Context
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.LargeTest
import androidx.test.filters.RequiresDevice
import androidx.test.filters.SdkSuppress
import ai.repose.mobile.unlock.crypto.AndroidKeyStoreSigner
import ai.repose.mobile.unlock.crypto.HardwareBackedKeyUnavailableException
import ai.repose.mobile.unlock.crypto.P256SignatureCodec
import java.math.BigInteger
import java.nio.charset.StandardCharsets
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** NOT RUN in Task 10 CI: requires a physical API 36 device. */
@RunWith(AndroidJUnit4::class)
@RequiresDevice
@SdkSuppress(minSdkVersion = 36)
@LargeTest
class AndroidKeyStoreSignerTest {
    private val context = ApplicationProvider.getApplicationContext<Context>()

    @After
    fun deleteTestKey() {
        KeyStore.getInstance("AndroidKeyStore").apply {
            load(null)
            deleteEntry(PRODUCTION_ALIAS)
        }
        context.getSharedPreferences(POLICY_PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .remove(POLICY_MARKER_KEY)
            .commit()
    }

    @Test
    fun generatedIdentityKeyIsNonExportableHardwareP256Sha256AndBackgroundUsable() {
        val signer = AndroidKeyStoreSigner(context)
        val publicKey = signer.publicKey() as ECPublicKey
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val entry = keyStore.getEntry(PRODUCTION_ALIAS, null) as KeyStore.PrivateKeyEntry
        val privateKey = entry.privateKey
        val keyInfo = KeyFactory.getInstance(privateKey.algorithm, "AndroidKeyStore")
            .getKeySpec(privateKey, KeyInfo::class.java)

        assertEquals(KeyProperties.KEY_ALGORITHM_EC, privateKey.algorithm)
        assertEquals(KeyProperties.KEY_ALGORITHM_EC, publicKey.algorithm)
        assertNull(privateKey.encoded)
        assertEquals(256, publicKey.params.curve.field.fieldSize)
        assertEquals(256, keyInfo.keySize)
        assertEquals(
            KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY,
            keyInfo.purposes,
        )
        assertEquals(KeyProperties.ORIGIN_GENERATED, keyInfo.origin)
        assertArrayEquals(arrayOf(KeyProperties.DIGEST_NONE), keyInfo.digests)
        assertFalse(keyInfo.isUserAuthenticationRequired)
        assertTrue(
            keyInfo.securityLevel == KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT ||
                keyInfo.securityLevel == KeyProperties.SECURITY_LEVEL_STRONGBOX,
        )
        assertTrue(
            context.getSharedPreferences(POLICY_PREFERENCES, Context.MODE_PRIVATE)
                .getString(POLICY_MARKER_KEY, null)
                ?.isNotBlank() == true,
        )

        val transcript = canonicalSigningTranscript()
        val request = ProtocolV1.phoneResponseSigningRequest(
            transcript.verifiedChallenge,
            transcript.responsePrefix,
        )
        val prehash = MessageDigest.getInstance("SHA-256").apply {
            update(PHONE_RESPONSE_SIGNATURE_LABEL)
            update(transcript.challenge)
            update(transcript.responsePrefix)
        }.digest()
        val signature = signer.signProtocolV1PhoneResponsePrehash(request).encoded()
        assertEquals(64, signature.size)
        val s = BigInteger(1, signature.copyOfRange(32, 64))
        assertTrue(s.signum() > 0)
        assertTrue(s <= P256_HALF_ORDER)
        assertTrue(
            Signature.getInstance("NONEwithECDSA").run {
                initVerify(publicKey)
                update(prehash)
                verify(P256SignatureCodec.derFromRaw(signature))
            },
        )
    }

    private fun canonicalSigningTranscript(): CanonicalSigningTranscript {
        val macIdentity = KeyPairGenerator.getInstance("EC").apply {
            initialize(ECGenParameterSpec("secp256r1"))
        }.generateKeyPair()
        val challenge = ByteArray(ProtocolV1.challengeFrameLength).apply {
            writeHeader(kind = 1, payloadLength = ProtocolV1.challengePayloadLength)
            MAC_ID.copyInto(this, destinationOffset = 12)
            DEVICE_ID.copyInto(this, destinationOffset = 28)
            writeULong(offset = 44, value = 7uL)
            writeULong(offset = 68, value = 1uL)
            writeULong(offset = 76, value = 1uL)
            writeUInt(offset = 84, value = 5_000u)
            P256_GENERATOR.copyInto(this, destinationOffset = 120)
        }
        val challengePrehash = MessageDigest.getInstance("SHA-256").apply {
            update(MAC_CHALLENGE_SIGNATURE_LABEL)
            update(challenge, 0, CHALLENGE_SIGNED_PREFIX_LENGTH)
        }.digest()
        val challengeSignature = Signature.getInstance("NONEwithECDSA").run {
            initSign(macIdentity.private)
            update(challengePrehash)
            P256SignatureCodec.canonicalRawFromDer(sign())
        }
        challengeSignature.copyInto(challenge, destinationOffset = CHALLENGE_SIGNED_PREFIX_LENGTH)
        val verifiedChallenge = ChallengeVerifier.verify(
            PairedMacRecord(
                MAC_ID,
                DEVICE_ID,
                7L,
                macIdentity.public,
            ),
            challenge,
        )
        val responsePrefix = ByteArray(RESPONSE_SIGNING_PREFIX_LENGTH).apply {
            writeHeader(kind = 2, payloadLength = ProtocolV1.responsePayloadLength)
            challenge.copyInto(this, destinationOffset = 12, startIndex = 12, endIndex = 76)
            writeULong(offset = 76, value = 2uL)
            challenge.copyInto(this, destinationOffset = 84, startIndex = 88, endIndex = 120)
            challenge.copyInto(this, destinationOffset = 148, startIndex = 120, endIndex = 185)
            P256_GENERATOR.copyInto(this, destinationOffset = 213)
        }
        return CanonicalSigningTranscript(verifiedChallenge, challenge, responsePrefix)
    }

    private data class CanonicalSigningTranscript(
        val verifiedChallenge: VerifiedMacChallenge,
        val challenge: ByteArray,
        val responsePrefix: ByteArray,
    )

    private fun ByteArray.writeHeader(kind: Int, payloadLength: Int) {
        byteArrayOf(0x52, 0x50, 0x55, 0x4b).copyInto(this)
        this[4] = 1
        this[5] = kind.toByte()
        writeUInt(offset = 8, value = payloadLength.toUInt())
    }

    private fun ByteArray.writeUInt(offset: Int, value: UInt) {
        repeat(UInt.SIZE_BYTES) { index ->
            this[offset + index] = (value shr (8 * (UInt.SIZE_BYTES - index - 1))).toByte()
        }
    }

    private fun ByteArray.writeULong(offset: Int, value: ULong) {
        repeat(ULong.SIZE_BYTES) { index ->
            this[offset + index] = (value shr (8 * (ULong.SIZE_BYTES - index - 1))).toByte()
        }
    }

    @Test
    fun existingIdentityWithoutMatchingTrustedMarkerIsDeletedAndFailsClosed() {
        val signer = AndroidKeyStoreSigner(context)
        val preferences = context.getSharedPreferences(POLICY_PREFERENCES, Context.MODE_PRIVATE)

        listOf<String?>(null, "mismatched-policy-fingerprint").forEach { marker ->
            signer.publicKey()
            val editor = preferences.edit()
            if (marker == null) editor.remove(POLICY_MARKER_KEY) else {
                editor.putString(POLICY_MARKER_KEY, marker)
            }
            assertTrue(editor.commit())

            assertThrows(HardwareBackedKeyUnavailableException::class.java) {
                signer.publicKey()
            }
            assertFalse(
                KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
                    .containsAlias(PRODUCTION_ALIAS),
            )
        }
    }

    private companion object {
        const val PRODUCTION_ALIAS = "ai.repose.mobile.unlock.identity.p256.v1"
        const val POLICY_PREFERENCES = "repose_unlock_key_policy"
        const val POLICY_MARKER_KEY = "identity.p256.v1"
        const val RESPONSE_SIGNING_PREFIX_LENGTH = 326
        const val CHALLENGE_SIGNED_PREFIX_LENGTH = 185
        val MAC_ID: ByteArray = ByteArray(16) { it.toByte() }
        val DEVICE_ID: ByteArray = ByteArray(16) { (it + 16).toByte() }
        val MAC_CHALLENGE_SIGNATURE_LABEL: ByteArray =
            "repose-unlock-v1 signature mac-to-phone challenge"
                .toByteArray(StandardCharsets.US_ASCII)
        val PHONE_RESPONSE_SIGNATURE_LABEL: ByteArray =
            "repose-unlock-v1 signature phone-to-mac"
                .toByteArray(StandardCharsets.US_ASCII)
        val P256_GENERATOR: ByteArray = (
            "04" +
                "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296" +
                "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5"
            ).hexBytes()
        val P256_HALF_ORDER: BigInteger = BigInteger(
            "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
            16,
        ).shiftRight(1)

        fun String.hexBytes(): ByteArray =
            chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }
}
