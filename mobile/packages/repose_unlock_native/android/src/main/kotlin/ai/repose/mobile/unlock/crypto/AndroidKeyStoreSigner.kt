package ai.repose.mobile.unlock.crypto

import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import ai.repose.mobile.unlock.protocol.PhoneResponseSigningRequest
import java.nio.charset.StandardCharsets
import java.security.KeyFactory
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.PrivateKey
import java.security.PublicKey
import java.security.Signature
import java.security.spec.ECGenParameterSpec
import java.util.Base64

@SuppressLint("ApplySharedPref", "UseKtx")
internal class AndroidKeyStoreSigner(context: Context) {
    private val applicationContext = context.applicationContext
    private val policyMarkers = applicationContext.getSharedPreferences(
        POLICY_PREFERENCES,
        Context.MODE_PRIVATE,
    )

    @Synchronized
    internal fun publicKey(): PublicKey = getOrCreate().publicKey

    @Synchronized
    internal fun signProtocolV1PhoneResponsePrehash(
        request: PhoneResponseSigningRequest,
    ): PhoneResponseSignature {
        val signedPrehash = request.copyPrehashForSigner()
        check(signedPrehash.size == SHA256_LENGTH)
        val identity = getOrCreate()
        val providerDer = Signature.getInstance(SIGNATURE_ALGORITHM).run {
            initSign(identity.privateKey)
            update(signedPrehash)
            sign()
        }
        val rawSignature = try {
            P256SignatureCodec.canonicalRawFromDer(providerDer)
        } catch (_: IllegalArgumentException) {
            throw HardwareBackedKeyUnavailableException()
        }
        val verified = Signature.getInstance(SIGNATURE_ALGORITHM).run {
            initVerify(identity.publicKey)
            update(signedPrehash)
            verify(P256SignatureCodec.derFromRaw(rawSignature))
        }
        if (!verified) throw HardwareBackedKeyUnavailableException()
        return SelfVerifiedPhoneResponseSignature(rawSignature)
    }

    private fun getOrCreate(): ValidatedSigningKey {
        val keyStore = loadKeyStore()
        val storedEntry = keyStore.getEntry(PRODUCTION_ALIAS, null)
        if (storedEntry != null) {
            val existing = storedEntry as? KeyStore.PrivateKeyEntry
                ?: invalidateAndFail(keyStore)
            if (!trustedMarkerMatches(existing.certificate.publicKey)) {
                invalidateAndFail(keyStore)
            }
            return try {
                inspect(
                    keyStore = keyStore,
                    privateKey = existing.privateKey,
                    publicKey = existing.certificate.publicKey,
                    trustedPolicyMarkerMatches = true,
                )
            } catch (exception: HardwareBackedKeyUnavailableException) {
                throw exception
            } catch (_: Exception) {
                invalidateAndFail(keyStore)
            }
        }

        clearPolicyMarkerOrFail()
        return try {
            val keyPair = generateWithHardwarePreference(keyStore)
            val validated = inspect(
                keyStore = keyStore,
                privateKey = keyPair.private,
                publicKey = keyPair.public,
                trustedPolicyMarkerMatches = true,
            )
            persistTrustedPolicyMarkerOrFail(keyStore, validated.publicKey)
            validated
        } catch (exception: Exception) {
            invalidate(keyStore)
            throw exception
        }
    }

    private fun generateWithHardwarePreference(keyStore: KeyStore): KeyPair {
        val preferStrongBox = applicationContext.packageManager.hasSystemFeature(
            PackageManager.FEATURE_STRONGBOX_KEYSTORE,
        )
        return if (preferStrongBox) {
            try {
                generate(strongBox = true)
            } catch (_: StrongBoxUnavailableException) {
                keyStore.deleteEntry(PRODUCTION_ALIAS)
                generate(strongBox = false)
            }
        } else {
            generate(strongBox = false)
        }
    }

    private fun generate(strongBox: Boolean): KeyPair {
        val generator = KeyPairGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_EC,
            ANDROID_KEY_STORE,
        )
        val spec = KeyGenParameterSpec.Builder(
            PRODUCTION_ALIAS,
            KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY,
        )
            .setKeySize(SigningKeyPolicy.requiredKeySize)
            .setAlgorithmParameterSpec(ECGenParameterSpec(SigningKeyPolicy.curveName))
            .setDigests(KeyProperties.DIGEST_NONE)
            .setUserAuthenticationRequired(false)
            .setUnlockedDeviceRequired(false)
            .setIsStrongBoxBacked(strongBox)
            .build()
        generator.initialize(spec)
        return generator.generateKeyPair()
    }

    private fun inspect(
        keyStore: KeyStore,
        privateKey: PrivateKey,
        publicKey: PublicKey,
        trustedPolicyMarkerMatches: Boolean,
    ): ValidatedSigningKey {
        val keyInfo = KeyFactory.getInstance(privateKey.algorithm, ANDROID_KEY_STORE)
            .getKeySpec(privateKey, KeyInfo::class.java)
        val hardwareBacked =
            keyInfo.securityLevel == KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT ||
                keyInfo.securityLevel == KeyProperties.SECURITY_LEVEL_STRONGBOX
        val snapshot = SigningKeyPolicySnapshot(
            privateKeyAlgorithm = privateKey.algorithm,
            privateKeyEncoded = privateKey.encoded,
            publicKey = publicKey,
            keySize = keyInfo.keySize,
            purposes = keyInfo.purposes,
            origin = keyInfo.origin,
            securityLevel = keyInfo.securityLevel,
            digests = keyInfo.digests,
            userAuthenticationRequired = keyInfo.isUserAuthenticationRequired,
            trustedPolicyMarkerMatches = trustedPolicyMarkerMatches,
        )
        if (!hardwareBacked || !SigningKeyPolicy.accepts(snapshot)) invalidateAndFail(keyStore)
        return ValidatedSigningKey(privateKey, publicKey)
    }

    private fun trustedMarkerMatches(publicKey: PublicKey): Boolean {
        val stored = policyMarkers.getString(POLICY_MARKER_KEY, null) ?: return false
        val expected = trustedPolicyMarker(publicKey) ?: return false
        return MessageDigest.isEqual(
            stored.toByteArray(StandardCharsets.US_ASCII),
            expected.toByteArray(StandardCharsets.US_ASCII),
        )
    }

    private fun persistTrustedPolicyMarkerOrFail(keyStore: KeyStore, publicKey: PublicKey) {
        val marker = trustedPolicyMarker(publicKey) ?: invalidateAndFail(keyStore)
        if (!policyMarkers.edit().putString(POLICY_MARKER_KEY, marker).commit()) {
            invalidateAndFail(keyStore)
        }
    }

    private fun trustedPolicyMarker(publicKey: PublicKey): String? {
        val encoded = publicKey.encoded ?: return null
        val digest = MessageDigest.getInstance(SigningKeyPolicy.digestName).apply {
            update(POLICY_FINGERPRINT.toByteArray(StandardCharsets.US_ASCII))
            update(0)
            update(encoded)
        }.digest()
        return Base64.getEncoder().withoutPadding().encodeToString(digest)
    }

    private fun clearPolicyMarkerOrFail() {
        if (!policyMarkers.edit().remove(POLICY_MARKER_KEY).commit()) {
            throw HardwareBackedKeyUnavailableException()
        }
    }

    private fun invalidateAndFail(keyStore: KeyStore): Nothing {
        invalidate(keyStore)
        throw HardwareBackedKeyUnavailableException()
    }

    private fun invalidate(keyStore: KeyStore) {
        keyStore.deleteEntry(PRODUCTION_ALIAS)
        policyMarkers.edit().remove(POLICY_MARKER_KEY).commit()
    }

    private fun loadKeyStore(): KeyStore = KeyStore.getInstance(ANDROID_KEY_STORE).apply {
        load(null)
    }

    private data class ValidatedSigningKey(
        val privateKey: PrivateKey,
        val publicKey: PublicKey,
    )

    private companion object {
        const val ANDROID_KEY_STORE = "AndroidKeyStore"
        const val SIGNATURE_ALGORITHM = "NONEwithECDSA"
        const val SHA256_LENGTH = 32
        const val PRODUCTION_ALIAS = "ai.repose.mobile.unlock.identity.p256.v1"
        const val POLICY_PREFERENCES = "repose_unlock_key_policy"
        const val POLICY_MARKER_KEY = "identity.p256.v1"
        const val POLICY_FINGERPRINT =
            "v1:EC:P-256:256:SIGN|VERIFY:GENERATED:HARDWARE:DIGEST-NONE:" +
                "PREHASH-SHA-256:NO-AUTH:UNLOCKED-OK"
    }
}

internal sealed interface PhoneResponseSignature {
    fun encoded(): ByteArray
}

private class SelfVerifiedPhoneResponseSignature(rawSignature: ByteArray) : PhoneResponseSignature {
    private val rawSignature = rawSignature.copyOf()

    override fun encoded(): ByteArray = rawSignature.copyOf()
}

internal class HardwareBackedKeyUnavailableException : IllegalStateException(
    "Repose requires a non-exportable P-256 identity key protected by TEE or StrongBox",
)
