package ai.repose.blespike

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import kotlin.random.Random

/** One paired Mac. Tonight these are local placeholders — there is no real pairing store yet. */
data class MacDevice(
    val id: String,
    val name: String,
    val lastSeen: String,
    val enabled: Boolean,
)

/**
 * Local, persisted placeholder state for the product shell: whether the phone is
 * "paired", the pairing code the Mac is supposed to echo, the list of Macs this phone
 * can unlock, and a stand-in unlock counter. No crypto, no network — all deferred.
 */
class AppStore(context: Context) {

    private val prefs = context.applicationContext
        .getSharedPreferences("repose_key", Context.MODE_PRIVATE)

    /**
     * Whether the user has this phone acting as a key.
     *
     * Persisted, because the alternative was an in-memory flag on SpikeState --
     * so the beacon stopped whenever Android reclaimed the process, and the
     * only signal was a Mac that quietly went back to asking for a password.
     * For a product whose whole claim is「手机就是钥匙」, a key that silently
     * stops being one is the worst failure it has.
     *
     * The service itself is still the truth about whether it is running; this
     * records the user's decision, so the app can put it back.
     */
    var advertiseWanted: Boolean
        get() = prefs.getBoolean(KEY_ADVERTISE, false)
        set(value) { prefs.edit().putBoolean(KEY_ADVERTISE, value).apply() }

    /**
     * Eight random bytes naming this phone, generated once and kept.
     *
     * Not a secret and not a credential -- it never authenticates anything. It
     * exists so a Mac can tell 「this phone again」 from 「a second phone」. Without
     * it, re-pairing would leave the previous key sitting in another slot,
     * advertised by nobody, and the Mac's list would fill with ghosts.
     */
    val phoneId: String
        get() = prefs.getString(KEY_PHONE_ID, null) ?: run {
            val bytes = ByteArray(8).also { java.security.SecureRandom().nextBytes(it) }
            val hex = bytes.joinToString("") { "%02x".format(it) }
            prefs.edit().putString(KEY_PHONE_ID, hex).apply()
            hex
        }

    /**
     * The key slot this phone uses on the Mac it is pairing with next.
     *
     * One per Mac, because each Mac derives its own key -- the phone's key never
     * leaves its secure element, so there is nothing to copy from one Mac to
     * another. Ids are drawn from 1..255 and must not repeat within this phone,
     * or the phone could not tell its own keys apart.
     */
    var keyIds: List<Int>
        get() = prefs.getString(KEY_KEY_IDS, null)
            ?.split(',')
            ?.mapNotNull { it.trim().toIntOrNull() }
            ?.filter { it in 1..255 }
            ?: emptyList()
        set(value) {
            prefs.edit().putString(KEY_KEY_IDS, value.distinct().joinToString(",")).apply()
        }

    /** An id this phone is not already using, or null when all 255 are taken. */
    fun nextKeyId(): Int? {
        val used = keyIds.toSet()
        // Random rather than lowest-free: two phones pairing with one Mac pick
        // independently, and always starting at 1 would make them collide every
        // time instead of once in 255.
        val free = (1..255).filterNot { it in used }
        if (free.isEmpty()) return null
        return free[java.security.SecureRandom().nextInt(free.size)]
    }

    var paired: Boolean
        get() = prefs.getBoolean(KEY_PAIRED, false)
        set(value) { prefs.edit().putBoolean(KEY_PAIRED, value).apply() }

    /**
     * What the paired Mac calls itself, if it told us. Written only after the
     * six digits matched.
     *
     * Cosmetic and attacker-controllable in principle -- see
     * [SpikeContract.PAIR_CHAR_NAME]. It replaced the 8-character key
     * fingerprint as the headline on this screen, not as the thing anyone
     * verifies: a flow with two opaque codes in it made people ask which one
     * mattered, and being unsure about that is exactly the confusion a
     * man-in-the-middle needs.
     */
    var pairedMac: String?
        get() = prefs.getString(KEY_MAC_NAME, null)
        set(value) {
            prefs.edit().apply {
                if (value.isNullOrBlank()) remove(KEY_MAC_NAME) else putString(KEY_MAC_NAME, value)
            }.apply()
        }

    /**
     * The next command sequence number, persisted.
     *
     * Monotonic across reboots and reinstalls-with-data, because it is the only
     * thing standing between a recorded advertisement and someone replaying
     * 「允许解锁」 at the Mac later. The Mac keeps a high-water mark and refuses
     * anything that is not strictly larger.
     *
     * A reinstall clears it back to 1, which the Mac would then reject as a
     * replay — that is the safe direction to fail, and re-pairing resets the
     * Mac's mark along with the key.
     */
    fun nextCommandSeq(): Long {
        val next = prefs.getLong(KEY_CMD_SEQ, 0L) + 1L
        prefs.edit().putLong(KEY_CMD_SEQ, next).apply()
        return next
    }

    /** Stable across launches so it can be compared against what the Mac shows. */
    val pairingCode: String
        get() {
            val existing = prefs.getString(KEY_CODE, null)
            if (existing != null) return existing
            val generated = buildString {
                val alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
                repeat(6) { append(alphabet[Random.nextInt(alphabet.length)]) }
            }
            prefs.edit().putString(KEY_CODE, generated).apply()
            return generated
        }

    /**
     * Defaulted to 4 -- a number the home screen printed as "今天解锁 4 次" on a phone
     * that had never unlocked anything. Nothing counts unlocks yet (the Mac does the
     * unlocking and never reports back), so the honest default is 0 and the screen
     * that used it no longer does.
     */
    val unlocksToday: Int
        get() = prefs.getInt(KEY_UNLOCKS, 0)

    fun macs(): List<MacDevice> {
        val raw = prefs.getString(KEY_MACS, null) ?: return seedMacs().also { saveMacs(it) }
        return runCatching {
            val arr = JSONArray(raw)
            (0 until arr.length()).map { i ->
                val o = arr.getJSONObject(i)
                MacDevice(
                    id = o.getString("id"),
                    name = o.getString("name"),
                    lastSeen = o.getString("lastSeen"),
                    enabled = o.getBoolean("enabled"),
                )
            }
        }.getOrElse { seedMacs().also { saveMacs(it) } }
    }

    fun setEnabled(id: String, enabled: Boolean) {
        saveMacs(macs().map { if (it.id == id) it.copy(enabled = enabled) else it })
    }

    fun disableAll() {
        saveMacs(macs().map { it.copy(enabled = false) })
    }

    private fun saveMacs(list: List<MacDevice>) {
        val arr = JSONArray()
        list.forEach { m ->
            arr.put(
                JSONObject()
                    .put("id", m.id)
                    .put("name", m.name)
                    .put("lastSeen", m.lastSeen)
                    .put("enabled", m.enabled),
            )
        }
        prefs.edit().putString(KEY_MACS, arr.toString()).apply()
    }

    /**
     * Empty, and deliberately so.
     *
     * This used to return two invented Macs -- "MacBook Pro（工作）· 上次 14:22" --
     * which the home screen then displayed as the machines this phone could unlock.
     * On a phone that had never been paired with anything, that is a screen making up
     * a security relationship. No pairing store exists yet, so the truthful answer is
     * that this phone knows of no Macs, and the screens say that.
     */
    private fun seedMacs(): List<MacDevice> = emptyList()

    private companion object {
        const val KEY_PAIRED = "paired"
        const val KEY_PHONE_ID = "phone_id"
        const val KEY_KEY_IDS = "key_ids"
        const val KEY_ADVERTISE = "advertise_wanted"
        const val KEY_MAC_NAME = "paired_mac_name"
        const val KEY_CMD_SEQ = "command_seq"
        const val KEY_CODE = "pairing_code"
        const val KEY_UNLOCKS = "unlocks_today"
        const val KEY_MACS = "macs_json"
    }
}
