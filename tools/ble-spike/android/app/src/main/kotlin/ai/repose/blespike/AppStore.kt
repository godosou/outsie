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

    var paired: Boolean
        get() = prefs.getBoolean(KEY_PAIRED, false)
        set(value) { prefs.edit().putBoolean(KEY_PAIRED, value).apply() }

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

    val unlocksToday: Int
        get() = prefs.getInt(KEY_UNLOCKS, 4)

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

    private fun seedMacs(): List<MacDevice> = listOf(
        MacDevice("mbp", "MacBook Pro（工作）", "上次 14:22", enabled = true),
        MacDevice("mba", "MacBook Air（家）", "3 天前", enabled = true),
    )

    private companion object {
        const val KEY_PAIRED = "paired"
        const val KEY_CODE = "pairing_code"
        const val KEY_UNLOCKS = "unlocks_today"
        const val KEY_MACS = "macs_json"
    }
}
