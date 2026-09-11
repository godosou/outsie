package ai.repose.blespike

import android.content.Context
import android.security.keystore.KeyProperties
import android.security.keystore.KeyProtection
import android.util.Log
import java.io.File
import java.security.KeyStore
import java.security.MessageDigest
import javax.crypto.Mac
import javax.crypto.SecretKey
import javax.crypto.spec.SecretKeySpec

/**
 * The presence key `K`: a 256-bit secret shared with exactly one Mac, held here as a
 * **non-exportable** AndroidKeyStore `HMAC_SHA256` key. Every beacon tag is computed
 * *through* [hmac], so the raw bytes exist only for the one-shot import.
 *
 * This is the whole of device identity. A second copy of this app -- the `imposter`
 * flavour, or anyone who reads the public service UUID off the air -- has its own
 * empty Keystore, so it cannot mint a tag the Mac accepts. That is the property
 * `tests/e2e/impersonation_test.sh` measures.
 *
 * HOW `K` GETS HERE, AND WHAT THAT IS NOT
 * ---------------------------------------
 * [ingestProvisionedKey] reads a hex file dropped into the app's external files
 * directory (`adb push`) and deletes it after import. That is a **development
 * channel**, not the product's pairing: it assumes the person holding the USB cable
 * is the owner. The real bootstrap is the SAS-guarded ECDH exchange in
 * docs/plans/2026-09-10-authenticated-presence-design.md §1, which is not built yet.
 *
 * Being explicit about that matters here more than usual. This project has three
 * times shipped an artifact asserting a safeguard the code did not have; a
 * provisioning path that *looks* like pairing would be the fourth. It is called
 * provisioning everywhere, including in the UI.
 */
object PresenceKey {

    private const val ANDROID_KEY_STORE = "AndroidKeyStore"
    private const val ALIAS_PREFIX = "ai.repose.blespike.presence.hmac.v1.key."
    private const val PREFS = "presence-key"
    private const val PREF_FINGERPRINT = "fingerprint"

    /** The file `adb push` drops to provision a key. Deleted as soon as it is imported. */
    const val PROVISION_FILE = "presence-key.hex"

    private fun alias(keyId: Int) = "$ALIAS_PREFIX$keyId"

    private fun store(): KeyStore = KeyStore.getInstance(ANDROID_KEY_STORE).apply { load(null) }

    /**
     * Every slot this phone actually holds a key in.
     *
     * From v3 the phone picks a slot per Mac, because each Mac derives its own
     * key -- the phone's key never leaves its secure element, so there is
     * nothing to copy between them. Before v3 every pairing landed in slot 1
     * and recorded no id, which is the fallback: a Mac paired last night must
     * keep working after this update.
     */
    fun activeIds(context: Context): List<Int> {
        val stored = AppStore(context).keyIds.filter { has(it) }
        if (stored.isNotEmpty()) return stored
        return if (has(SpikeContract.PRESENCE_KEY_ID)) listOf(SpikeContract.PRESENCE_KEY_ID) else emptyList()
    }

    fun hasAny(context: Context): Boolean = activeIds(context).isNotEmpty()

    fun has(keyId: Int): Boolean =
        runCatching { store().containsAlias(alias(keyId)) }.getOrDefault(false)

    /**
     * A short, non-secret label for `K`, so a human can check the phone and the Mac hold
     * the same key without either of them showing the key. It is a hash of `K` under its
     * own domain label, never a prefix of `K` itself.
     */
    fun fingerprint(context: Context): String? =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(PREF_FINGERPRINT, null)

    fun importKey(context: Context, keyId: Int, k: ByteArray) {
        require(k.size == 32) { "presence key K must be 32 bytes, got ${k.size}" }
        val ks = store()
        if (ks.containsAlias(alias(keyId))) ks.deleteEntry(alias(keyId))
        ks.setEntry(
            alias(keyId),
            KeyStore.SecretKeyEntry(
                SecretKeySpec(k, KeyProperties.KEY_ALGORITHM_HMAC_SHA256) as SecretKey,
            ),
            KeyProtection.Builder(KeyProperties.PURPOSE_SIGN).build(),
        )
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString(PREF_FINGERPRINT, fingerprintOf(k)).apply()
    }

    fun delete(context: Context, keyId: Int) {
        runCatching { store().apply { if (containsAlias(alias(keyId))) deleteEntry(alias(keyId)) } }
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .remove(PREF_FINGERPRINT).apply()
    }

    /** HMAC-SHA-256(K[keyId], msg), computed inside the Keystore. */
    fun hmac(keyId: Int, msg: ByteArray): ByteArray {
        val key = store().getKey(alias(keyId), null) as SecretKey
        return Mac.getInstance("HmacSHA256").run { init(key); doFinal(msg) }
    }

    /**
     * Import a key dropped by `adb push`, if there is one. Returns a human-readable
     * outcome for the event log, or null if no file was waiting.
     *
     * The file is deleted whatever happens -- including on a malformed one, so a bad
     * push does not sit around being retried on every service start.
     */
    fun ingestProvisionedKey(context: Context, keyId: Int): String? {
        val dir = context.getExternalFilesDir(null) ?: return null
        val file = File(dir, PROVISION_FILE)
        if (!file.exists()) return null
        val text = runCatching { file.readText() }.getOrNull()?.trim().orEmpty()
        val wiped = file.delete()
        val k = decodeHex(text)
        return when {
            k == null || k.size != 32 ->
                "provisioning file rejected (need 64 hex chars, got ${text.length})"
            else -> {
                runCatching { importKey(context, keyId, k) }
                    .fold(
                        onSuccess = {
                            k.fill(0)
                            "presence key provisioned, fingerprint ${fingerprint(context)}" +
                                if (wiped) "" else " (WARNING: could not delete the hex file)"
                        },
                        onFailure = { e ->
                            Log.e(BleSpikeService.TAG, "key import failed", e)
                            "presence key import FAILED: ${e.message}"
                        },
                    )
            }
        }
    }

    fun fingerprintOf(k: ByteArray): String {
        val d = MessageDigest.getInstance("SHA-256")
            .digest("repose-presence-v1 fingerprint".toByteArray(Charsets.US_ASCII) + k)
        return d.copyOf(4).joinToString("") { "%02X".format(it) }
    }

    private fun decodeHex(s: String): ByteArray? {
        if (s.length % 2 != 0 || s.isEmpty()) return null
        return runCatching {
            ByteArray(s.length / 2) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
        }.getOrNull()
    }
}
