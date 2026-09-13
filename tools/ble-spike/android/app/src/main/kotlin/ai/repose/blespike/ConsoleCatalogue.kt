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

/** [cmdByte] is the App's own byte: 「切到这个 App」, no key pressed. Null from a Mac that predates it. */
data class ConsoleApp(val name: String, val actions: List<ConsoleAction>, val cmdByte: Int? = null)

data class ConsoleCatalogue(
    val revision: Long,
    val apps: List<ConsoleApp>,
    /**
     * Which key verified it — which is to say, which Mac sent it.
     *
     * Verification used to answer only 「验过了」 and throw away 「哪一把」, so the
     * 控制 screen could not name the computer its buttons belonged to. With two
     * Macs paired that is not a cosmetic gap: the buttons on screen are one
     * machine's, and nothing said which.
     *
     * 0 means unknown — a catalogue stored before this was recorded.
     */
    val keyId: Int = 0,
) {

    companion object {
        const val VERSION = 1
        const val TAG_LEN = 16
        const val LABEL = "repose-console-v1 catalogue"

        private const val PREFS = "repose_console"

        /** The storage namespace for one Mac. One catalogue per key slot (§04 §08). */
        fun slot(keyId: Int): String = "k$keyId."

        /**
         * The catalogue, if it is THIS Mac's; otherwise nothing.
         *
         * The control screen for one Mac must never borrow another Mac's
         * buttons: they look the same, the bytes are another machine's. A
         * catalogue with keyId 0 was stored before keys were recorded and
         * belongs to nobody.
         */
        fun own(cat: ConsoleCatalogue?, keyId: Int): ConsoleCatalogue? =
            cat?.takeIf { keyId != 0 && it.keyId == keyId }

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

            // Which key matched is kept, not discarded: it is the only thing in
            // the whole exchange that says which Mac this catalogue came from.
            val matched = PresenceKey.activeIds(context).firstOrNull { id ->
                if (!PresenceKey.hasConsoleKey(id)) return@firstOrNull false
                val full = runCatching { PresenceKey.consoleHmac(id, msg) }.getOrNull()
                    ?: return@firstOrNull false
                constantTimeEquals(full.copyOf(TAG_LEN), tag)
            } ?: return null

            var rev = 0L
            for (i in 1..4) rev = (rev shl 8) or (whole[i].toLong() and 0xFF)
            val json = String(body, 5, body.size - 5, Charsets.UTF_8)
            return parse(rev, json, matched)
        }

        fun parse(revision: Long, json: String, keyId: Int = 0): ConsoleCatalogue? = runCatching {
            val root = JSONObject(json)
            val apps = root.optJSONArray("apps") ?: return@runCatching ConsoleCatalogue(revision, emptyList(), keyId)
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
                val appByte = a.optInt("b", 0).takeIf { it >= SpikeContract.CMD_SHORTCUT_BASE && it <= 255 }
                if (list.isNotEmpty()) out += ConsoleApp(a.optString("n", "?"), list, appByte)
            }
            ConsoleCatalogue(revision, out, keyId)
        }.getOrNull()

        /** Kept across restarts, so the buttons are there before the Mac is. */
        fun save(context: Context, revision: Long, json: String, keyId: Int) {
            val s = slot(keyId)
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString(s + "json", json)
                .putLong(s + "revision", revision)
                .apply()
        }

        fun load(context: Context, keyId: Int): ConsoleCatalogue? {
            val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val s = slot(keyId)
            val json = p.getString(s + "json", null) ?: migrateLegacy(context, keyId) ?: return null
            return parse(p.getLong(s + "revision", 0L), json, keyId)
        }

        /**
         * The build before this one kept a single catalogue and remembered
         * which key verified it. Move it into that key's slot once, rather
         * than making everyone who updates sync again and wonder why their
         * buttons vanished. Returns the json if it was moved here.
         */
        private fun migrateLegacy(context: Context, keyId: Int): String? {
            val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val json = p.getString("catalogue_json", null) ?: return null
            if (p.getInt("catalogue_key_id", 0) != keyId || keyId == 0) return null
            val s = slot(keyId)
            p.edit()
                .putString(s + "json", json)
                .putLong(s + "revision", p.getLong("catalogue_revision", 0L))
                .remove("catalogue_json").remove("catalogue_revision").remove("catalogue_key_id")
                .apply()
            return json
        }

        /** When a Mac is removed from this phone, its catalogue goes with it. */
        fun forget(context: Context, keyId: Int) {
            val s = slot(keyId)
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .remove(s + "json").remove(s + "revision").apply()
        }

        private fun constantTimeEquals(a: ByteArray, b: ByteArray): Boolean {
            if (a.size != b.size) return false
            var diff = 0
            for (i in a.indices) diff = diff or (a[i].toInt() xor b[i].toInt())
            return diff == 0
        }
    }
}
