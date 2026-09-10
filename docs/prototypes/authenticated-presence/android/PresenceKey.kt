package ai.repose.blespike

import android.security.keystore.KeyProperties
import android.security.keystore.KeyProtection
import java.security.KeyStore
import javax.crypto.Mac
import javax.crypto.SecretKey
import javax.crypto.spec.SecretKeySpec

/**
 * The presence HMAC key `K` (§1.5), imported into AndroidKeyStore as a **non-exportable**
 * `HMAC_SHA256` key so the 256-bit secret never sits in app-readable storage. The rotating
 * beacon tag is computed *through* Keystore ([hmac]) — the raw bytes are used only for the
 * one-shot import and then discarded by the caller.
 *
 * One key per `keyId` (the pairing slot / `K` selector carried in the advertisement).
 */
object PresenceKey {

    private const val ANDROID_KEY_STORE = "AndroidKeyStore"
    private const val ALIAS_PREFIX = "ai.repose.blespike.presence.hmac.v1.key."

    private fun alias(keyId: Int): String = "$ALIAS_PREFIX$keyId"

    private fun store(): KeyStore = KeyStore.getInstance(ANDROID_KEY_STORE).apply { load(null) }

    /**
     * Import raw `K` (32 bytes) as a non-exportable HMAC-SHA-256 Keystore key under [keyId].
     * Replaces any existing key for the same slot (a re-pair mints a fresh `K`, §1.6).
     */
    fun importKey(keyId: Int, k: ByteArray) {
        require(k.size == 32) { "presence key K must be 32 bytes" }
        val ks = store()
        if (ks.containsAlias(alias(keyId))) ks.deleteEntry(alias(keyId))
        val secret: SecretKey = SecretKeySpec(k, KeyProperties.KEY_ALGORITHM_HMAC_SHA256)
        ks.setEntry(
            alias(keyId),
            KeyStore.SecretKeyEntry(secret),
            KeyProtection.Builder(KeyProperties.PURPOSE_SIGN).build(),
        )
    }

    fun has(keyId: Int): Boolean = runCatching { store().containsAlias(alias(keyId)) }.getOrDefault(false)

    /** Compute HMAC-SHA-256(K[keyId], msg) through the hardware key. */
    fun hmac(keyId: Int, msg: ByteArray): ByteArray {
        val key = store().getKey(alias(keyId), null) as SecretKey
        return Mac.getInstance("HmacSHA256").run {
            init(key)
            doFinal(msg)
        }
    }

    fun delete(keyId: Int) {
        runCatching { store().apply { if (containsAlias(alias(keyId))) deleteEntry(alias(keyId)) } }
    }
}
