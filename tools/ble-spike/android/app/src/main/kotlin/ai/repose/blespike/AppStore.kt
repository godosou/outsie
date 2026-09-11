package ai.repose.blespike

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import kotlin.random.Random

/**
 * One Mac this phone has paired with.
 *
 * THIS LIST IS THE PHONE'S OWN RECORD, NOT THE MAC'S STATE.
 *
 * It says "I hold a key for this one", not "this one is switched on" or even
 * "this one still trusts me". A Mac whose owner deleted the key from the Mac
 * side has no way to tell this phone so, and this list will still show it --
 * which is why the screen says as much rather than implying otherwise.
 *
 * [macId] is filled in the first time a state beacon verifies under this slot,
 * so a freshly paired Mac has a name and no id until it is next heard from.
 */
data class PairedMac(
    /** The key slot this Mac's key lives in, on both ends. */
    val keyId: Int,
    /** What the Mac called itself at pairing. Cosmetic and chosen by it. */
    val name: String,
    val pairedAt: String,
    /** Four hex digits from the Mac's own beacon, once heard. */
    val macId: String?,
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

    /**
     * Every Mac this phone holds a key for.
     *
     * Built from the KEYS, not from the records. A key whose record is missing --
     * paired before this list existed, or written by a build that did not keep
     * one -- is still a key this phone advertises under and still something the
     * owner may want gone. Listing only the records would leave those invisible
     * and unremovable, which is the worst combination: the phone goes on
     * broadcasting for a Mac nobody can see and nobody can delete.
     */
    fun pairedMacs(context: Context): List<PairedMac> {
        val records = storedRecords().associateBy { it.keyId }
        return PresenceKey.activeIds(context).map { id ->
            records[id] ?: PairedMac(
                keyId = id,
                name = "一台 Mac",
                pairedAt = "",
                macId = null,
            )
        }
    }

    private fun storedRecords(): List<PairedMac> = runCatching {
        val arr = JSONArray(prefs.getString(KEY_MACS, null) ?: "[]")
        (0 until arr.length()).mapNotNull { i ->
            val o = arr.getJSONObject(i)
            val id = o.optInt("keyId", 0)
            if (id !in 1..255) return@mapNotNull null
            PairedMac(
                keyId = id,
                name = o.optString("name", "").ifBlank { "一台 Mac" },
                pairedAt = o.optString("pairedAt", ""),
                macId = o.optString("macId", "").ifBlank { null },
            )
        }
    }.getOrDefault(emptyList())

    /** Record a pairing. Replaces any entry already in that slot. */
    fun rememberMac(keyId: Int, name: String?, pairedAt: String) {
        val kept = storedRecords().filter { it.keyId != keyId }
        saveMacs(
            kept + PairedMac(
                keyId = keyId,
                name = name?.trim().orEmpty().ifBlank { "一台 Mac" },
                pairedAt = pairedAt,
                macId = null,
            ),
        )
    }

    /**
     * Fill in the id of whichever Mac is broadcasting under this slot.
     *
     * Only ever the first time, and only from a beacon that verified: a slot's
     * id is a fact about which machine holds that key, and overwriting it from a
     * later beacon would let a second Mac that somehow shares the slot rename
     * the first.
     */
    fun noteMacId(keyId: Int, macId: String) {
        val list = storedRecords()
        val existing = list.firstOrNull { it.keyId == keyId } ?: return
        if (existing.macId != null) return
        saveMacs(list.map { if (it.keyId == keyId) it.copy(macId = macId) else it })
    }

    /**
     * Forget one Mac: the record and both of its keys.
     *
     * The keys go with it, always. An entry removed from the list while its key
     * stayed in the keystore would be a phone that had stopped admitting to
     * opening a Mac it could still open.
     */
    fun forgetMac(context: Context, keyId: Int) {
        PresenceKey.delete(context, keyId)
        keyIds = keyIds.filter { it != keyId }
        saveMacs(storedRecords().filter { it.keyId != keyId })
    }

    private fun saveMacs(list: List<PairedMac>) {
        val arr = JSONArray()
        list.forEach { m ->
            arr.put(
                JSONObject()
                    .put("keyId", m.keyId)
                    .put("name", m.name)
                    .put("pairedAt", m.pairedAt)
                    .apply { m.macId?.let { put("macId", it) } },
            )
        }
        prefs.edit().putString(KEY_MACS, arr.toString()).apply()
    }

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
