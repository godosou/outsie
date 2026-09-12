package ai.repose.blespike

import android.content.Context

/**
 * What THIS phone shows of ONE Mac's catalogue, and in what order (design doc §10).
 *
 * WHY THE PHONE GETS A SAY AT ALL
 *
 * The Mac owns what EXISTS — which app, which keys, which cmd byte. That stays
 * on the Mac: recording a shortcut needs a real keyboard, and the byte is part
 * of the protocol. But one desk's catalogue came to 87 actions, and no phone
 * needs 87 buttons. Choosing and ordering is what fingers are good at.
 *
 * NOTHING HERE IS A PERMISSION
 *
 * These preferences never leave the phone. They are not sent back, they do not
 * touch the catalogue, they do not change a single cmd byte, and they are not
 * part of the signature. Hiding an action does not stop its byte from working.
 * The real boundary is on the Mac, one switch per phone. The screen says so.
 *
 * KEYED ON THE BYTE, NEVER THE POSITION
 *
 * An action is remembered by its cmd byte, which the Mac assigns once and does
 * not reuse until the other 240 are gone. Remembering "the third one" would
 * mean that deleting an action on the Mac silently re-points every preference
 * at its neighbour — the same reason the protocol itself does not send indices.
 *
 * ONE PER MAC
 *
 * Stored under the key slot of the Mac the catalogue came from (§04 §08). Two
 * Macs assign bytes independently; a shared preference would let 「hidden 17」
 * on one Mac hide an unrelated action on the other.
 */
data class ConsoleArrangement(
    /** cmd bytes this phone does not draw. */
    val hidden: Set<Int> = emptySet(),
    /** cmd bytes, first drawn first. Anything absent keeps the Mac's order, last. */
    val order: List<Int> = emptyList(),
) {
    val isDefault: Boolean get() = hidden.isEmpty() && order.isEmpty()

    fun toggle(cmd: Int): ConsoleArrangement =
        copy(hidden = if (cmd in hidden) hidden - cmd else hidden + cmd)

    companion object {
        private const val PREFS = "repose_console_view"

        /** The storage namespace for one Mac. Distinct per key slot, stable across runs. */
        fun slot(keyId: Int): String = "k$keyId."

        /**
         * Stored apart from the catalogue, on purpose.
         *
         * The catalogue is what arrived and is overwritten whole on every sync;
         * this is what the person chose, and a sync must not touch it. Kept in
         * one file, the next sync would wipe the ordering — and that shows up
         * as "my ordering did not save", a bug nobody can report clearly.
         */
        fun load(context: Context, keyId: Int): ConsoleArrangement {
            val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val s = slot(keyId)
            return ConsoleArrangement(
                hidden = ints(p.getString(s + "hidden", "")).toSet(),
                order = ints(p.getString(s + "order", "")),
            )
        }

        fun save(context: Context, keyId: Int, a: ConsoleArrangement) {
            val s = slot(keyId)
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString(s + "hidden", a.hidden.joinToString(","))
                .putString(s + "order", a.order.joinToString(","))
                .apply()
        }

        /** When a Mac is removed from this phone, its preferences go with it. */
        fun forget(context: Context, keyId: Int) {
            val s = slot(keyId)
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .remove(s + "hidden").remove(s + "order").apply()
        }

        private fun ints(raw: String?): List<Int> =
            raw.orEmpty().split(",").mapNotNull { it.trim().toIntOrNull() }

        /**
         * Drop what the catalogue no longer contains.
         *
         * Not merely ignored at draw time — actually removed, on every sync. A
         * freed cmd byte is reused once the other 240 are spent, and a stale
         * 「hidden」 record would then make a brand-new action invisible from the
         * moment it arrives, with nothing on screen able to explain why.
         */
        fun prune(cat: ConsoleCatalogue, a: ConsoleArrangement): ConsoleArrangement {
            val bytes = cat.apps.flatMap { app -> app.actions.map { it.cmdByte } }.toSet()
            return ConsoleArrangement(
                hidden = a.hidden.filterTo(LinkedHashSet()) { it in bytes },
                order = a.order.filter { it in bytes },
            )
        }

        /**
         * The catalogue as this phone wants to see it.
         *
         * Apps are never dropped, even with everything inside hidden: the chip
         * stays and the grid says so (§04). A chip that vanished could not
         * explain itself.
         *
         * Anything the preferences have never heard of sorts LAST and stays
         * VISIBLE. The other way round is much worse: you add an action on the
         * Mac, sync, and cannot find it — a successful sync that looks exactly
         * like a broken one.
         */
        fun arrange(
            cat: ConsoleCatalogue,
            a: ConsoleArrangement,
            includeHidden: Boolean = false,
        ): List<ConsoleApp> = cat.apps.map { app ->
            ConsoleApp(
                app.name,
                inOrder(app.actions, a.order) { it.cmdByte }
                    .filter { includeHidden || it.cmdByte !in a.hidden },
            )
        }

        /** Stable: known keys in the given order, unknown ones after, as they came. */
        private fun <T, K> inOrder(items: List<T>, order: List<K>, key: (T) -> K): List<T> {
            val rank = HashMap<K, Int>(order.size)
            order.forEachIndexed { i, k -> if (!rank.containsKey(k)) rank[k] = i }
            return items.withIndex()
                .sortedBy { (i, item) -> rank[key(item)] ?: (order.size + i) }
                .map { it.value }
        }

        /**
         * One tile dragged onto another. Returns the arrangement to store.
         *
         * The WHOLE order is written down — this app's visible tiles in their
         * new order, then its hidden ones, then whatever other apps had — so a
         * drag cannot shuffle anything nobody touched, and un-hiding puts a
         * tile back where it was rather than at the Mac's position.
         */
        fun dropped(cat: ConsoleCatalogue, appName: String, a: ConsoleArrangement, from: Int, onto: Int): ConsoleArrangement {
            if (from == onto) return a
            val app = arrange(cat, a, includeHidden = true).firstOrNull { it.name == appName } ?: return a
            val vis = app.actions.map { it.cmdByte }.filter { it !in a.hidden }.toMutableList()
            val hid = app.actions.map { it.cmdByte }.filter { it in a.hidden }
            val i = vis.indexOf(from)
            val j = vis.indexOf(onto)
            if (i < 0 || j < 0) return a
            vis.add(j, vis.removeAt(i))
            val mine = (vis + hid).toSet()
            return a.copy(order = vis + hid + a.order.filter { it !in mine })
        }
    }
}
