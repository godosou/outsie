package ai.repose.blespike

import android.content.Context
import org.json.JSONObject

/**
 * The buttons this phone may show, as the Mac described them.
 *
 * Read-only here, and that is deliberate: configuring a computer's keyboard
 * shortcuts on a phone is putting the hardest input on the smallest screen. The
 * Mac owns the list; this is a copy of it, and it is only ever trusted after its
 * signature checks out.
 */
data class ConsoleAction(
    /** The byte to put in the beacon. Assigned by the Mac and stable across edits. */
    val cmdByte: Int,
    val name: String,
    val icon: String?,
    /** The keys, spelled the way the Mac's own screen spells them. */
    val keys: String,
)

data class ConsoleApp(val name: String, val actions: List<ConsoleAction>)

data class ConsoleCatalogue(val revision: Long, val apps: List<ConsoleApp>) {

    companion object {
        const val VERSION = 1
        const val TAG_LEN = 16
        const val LABEL = "repose-console-v1 catalogue"

        private const val PREFS = "repose_console"
        private const val KEY_JSON = "catalogue_json"
        private const val KEY_REVISION = "catalogue_revision"

        /**
         * Check a received blob and parse it, or return null.
         *
         * Null covers every reason, on purpose: a caller that could tell "bad
         * signature" from "bad JSON" would be tempted to show the second kind
         * anyway, and an attacker controls both.
         *
         * The tag is tried against EVERY key this phone holds, because the
         * catalogue arrives before anything says which Mac sent it -- and a
         * catalogue is only as good as the key it verifies under, whichever
         * that turns out to be.
         */
        fun verify(context: Context, whole: ByteArray): ConsoleCatalogue? {
            if (whole.size < 1 + 4 + TAG_LEN) return null
            if ((whole[0].toInt() and 0xFF) != VERSION) return null
            val body = whole.copyOfRange(0, whole.size - TAG_LEN)
            val tag = whole.copyOfRange(whole.size - TAG_LEN, whole.size)
            val msg = LABEL.toByteArray(Charsets.US_ASCII) + body

            val matched = PresenceKey.activeIds(context).any { id ->
                if (!PresenceKey.hasConsoleKey(id)) return@any false
                val full = runCatching { PresenceKey.consoleHmac(id, msg) }.getOrNull() ?: return@any false
                constantTimeEquals(full.copyOf(TAG_LEN), tag)
            }
            if (!matched) return null

            var rev = 0L
            for (i in 1..4) rev = (rev shl 8) or (whole[i].toLong() and 0xFF)
            val json = String(body, 5, body.size - 5, Charsets.UTF_8)
            return parse(rev, json)
        }

        fun parse(revision: Long, json: String): ConsoleCatalogue? = runCatching {
            val root = JSONObject(json)
            val apps = root.optJSONArray("apps") ?: return@runCatching ConsoleCatalogue(revision, emptyList())
            val out = ArrayList<ConsoleApp>(apps.length())
            for (i in 0 until apps.length()) {
                val a = apps.getJSONObject(i)
                val acts = a.optJSONArray("a")
                val list = ArrayList<ConsoleAction>(acts?.length() ?: 0)
                for (j in 0 until (acts?.length() ?: 0)) {
                    val x = acts!!.getJSONObject(j)
                    val b = x.optInt("b", 0)
                    // A byte below the shortcut base is a protocol command --
                    // lock, and friends. A "shortcut" that locks the Mac is not
                    // a button anyone asked for.
                    if (b < SpikeContract.CMD_SHORTCUT_BASE || b > 255) continue
                    val name = x.optString("n", "").ifBlank { continue }
                    list += ConsoleAction(
                        cmdByte = b,
                        name = name,
                        icon = x.optString("i", "").ifBlank { null },
                        keys = x.optString("k", ""),
                    )
                }
                if (list.isNotEmpty()) out += ConsoleApp(a.optString("n", "?"), list)
            }
            ConsoleCatalogue(revision, out)
        }.getOrNull()

        /** Kept across restarts, so the buttons are there before the Mac is. */
        fun save(context: Context, revision: Long, json: String) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString(KEY_JSON, json)
                .putLong(KEY_REVISION, revision)
                .apply()
        }

        fun load(context: Context): ConsoleCatalogue? {
            val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val json = p.getString(KEY_JSON, null) ?: return null
            return parse(p.getLong(KEY_REVISION, 0L), json)
        }

        fun forget(context: Context) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().clear().apply()
        }

        private fun constantTimeEquals(a: ByteArray, b: ByteArray): Boolean {
            if (a.size != b.size) return false
            var diff = 0
            for (i in a.indices) diff = diff or (a[i].toInt() xor b[i].toInt())
            return diff == 0
        }
    }
}
