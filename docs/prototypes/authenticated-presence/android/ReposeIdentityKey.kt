package ai.repose.blespike

import android.content.Context
import android.content.pm.PackageManager
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec

/**
 * The phone's long-term P-256 identity key `IK_P` (§1.5), ported from codex
 * `AndroidKeyStoreSigner` / `SigningKeyPolicy`: non-exportable, hardware-backed
 * (StrongBox-preferred, TEE-fallback), `DIGEST_NONE` + prehash-SHA-256, alias reused verbatim.
 * The private key never leaves hardware; only its SEC1-uncompressed public form crosses the wire.
 *
 * For this task the identity key's role is to fix `IK_P` into the pairing transcript and SAS.
 * [signPrehash] is kept for reuse by the later codex `repose-unlock-v1` Response path.
 */
class ReposeIdentityKey(context: Context) {

    private val appContext = context.applicationContext

    /** SEC1-uncompressed (`0x04 ‖ X ‖ Y`, 65 bytes) public key, creating the key on first use. */
    @Synchronized
    fun publicKeySec1(): ByteArray = PairingCrypto.encodePublicKey(getOrCreate().public)

    /** NONEwithECDSA over a 32-byte prehash — the codex Response signing shape (DER-encoded). */
    @Synchronized
    fun signPrehash(prehash: ByteArray): ByteArray {
        require(prehash.size == 32) { "identity signer expects a 32-byte SHA-256 prehash" }
        val pair = getOrCreate()
        return Signature.getInstance(SIGNATURE_ALGORITHM).run {
            initSign(pair.private)
            update(prehash)
            sign()
        }
    }

    private fun getOrCreate(): KeyPair {
        val keyStore = KeyStore.getInstance(ANDROID_KEY_STORE).apply { load(null) }
        val existing = keyStore.getEntry(ALIAS, null) as? KeyStore.PrivateKeyEntry
        if (existing != null) {
            return KeyPair(existing.certificate.publicKey, existing.privateKey)
        }
        return generateWithHardwarePreference(keyStore)
    }

    private fun generateWithHardwarePreference(keyStore: KeyStore): KeyPair {
        val preferStrongBox = appContext.packageManager
            .hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE)
        return if (preferStrongBox) {
            try {
                generate(strongBox = true)
            } catch (_: StrongBoxUnavailableException) {
                keyStore.deleteEntry(ALIAS)
                generate(strongBox = false)
            }
        } else {
            generate(strongBox = false)
        }
    }

    private fun generate(strongBox: Boolean): KeyPair {
        val spec = KeyGenParameterSpec.Builder(
            ALIAS,
            KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY,
        )
            .setKeySize(256)
            .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
            .setDigests(KeyProperties.DIGEST_NONE)
            .setUserAuthenticationRequired(false)
            .setUnlockedDeviceRequired(false)
            .setIsStrongBoxBacked(strongBox)
            .build()
        return KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, ANDROID_KEY_STORE).run {
            initialize(spec)
            generateKeyPair()
        }.also { require(it.public is ECPublicKey) }
    }

    /** Destroy the identity key on "撤销设备 / 移除并还原" (§1.6). */
    @Synchronized
    fun destroy() {
        runCatching {
            KeyStore.getInstance(ANDROID_KEY_STORE).apply { load(null) }.deleteEntry(ALIAS)
        }
    }

    private companion object {
        const val ANDROID_KEY_STORE = "AndroidKeyStore"
        const val SIGNATURE_ALGORITHM = "NONEwithECDSA"
        // Alias reused verbatim from codex so the identity is the same key material contract.
        const val ALIAS = "ai.repose.mobile.unlock.identity.p256.v1"
    }
}
