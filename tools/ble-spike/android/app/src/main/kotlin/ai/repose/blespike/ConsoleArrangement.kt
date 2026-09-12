package ai.repose.blespike

import android.content.Context

/**
 * What THIS phone shows, and in what order.
 *
 * WHY THE PHONE GETS A SAY AT ALL
 *
 * The Mac owns what EXISTS — which app, which keys, which cmd byte. That stays
 * on the Mac: recording a shortcut needs a real keyboard, and the byte is part
 * of the protocol. But one desk's catalogue came to 87 actions, and no phone
 * needs 87 buttons. Choosing and ordering is what fingers are good at, and it
 * is the part that differs from phone to phone.
 *
 * NOTHING HERE IS A PERMISSION
 *
 * These preferences never leave the phone. They are not sent back, they do not
 * touch the catalogue, they do not change a single cmd byte, and they are not
 * part of the signature. Hiding an action does not stop its byte from working —
 * reinstall the app and it is back. The real boundary is on the Mac, one switch
 * per phone. The screen has to say so, or hiding gets mistaken for revoking.
 *
 * KEYED ON THE BYTE, NEVER THE POSITION
 *
 * An action is remembered by its cmd byte, which the Mac assigns once and never
 * reuses until the other 240 are gone. Remembering "the third one" would mean
 * that deleting an action on the Mac silently re-points every preference at its
 * neighbour — the same reason the protocol itself does not send indices.
 *
 * Apps have no stable id yet (the catalogue carries only their names), so they
 * are keyed on the name and a rename on the Mac resets their order. Sending
 * bundleId with the catalogue would fix it and costs one field.
 */
data class ConsoleArrangement(
    /** cmd bytes this phone does not draw. */
    val hiddenActions: Set<Int> = emptySet(),
    /** cmd bytes, first drawn first. Anything absent keeps the Mac's order, last. */
    val actionOrder: List<Int> = emptyList(),
    /** App names this phone does not draw. */
    val hiddenApps: Set<String> = emptySet(),
    val appOrder: List<String> = emptyList(),
) {
    val isDefault: Boolean
        get() = hiddenActions.isEmpty() && actionOrder.isEmpty() &&
            hiddenApps.isEmpty() && appOrder.isEmpty()

    fun toggleAction(cmd: Int): ConsoleArrangement =
        copy(hiddenActions = if (cmd in hiddenActions) hiddenActions - cmd else hiddenActions + cmd)

    fun toggleApp(name: String): ConsoleArrangement =
        copy(hiddenApps = if (name in hiddenApps) hiddenApps - name else hiddenApps + name)

    companion object {
        private const val PREFS = "repose_console_view"
        private const val K_HIDDEN_ACTIONS = "hidden_actions"
        private const val K_ACTION_ORDER = "action_order"
        private const val K_HIDDEN_APPS = "hidden_apps"
        private const val K_APP_ORDER = "app_order"

        /**
         * App names come from the Mac and are whatever a person typed —
         * commas and spaces included ("VS Code", "飞书"). NUL is the one
         * separator that cannot appear inside one, so it is the only one
         * that cannot silently split a name in two.
         */
        internal const val SEP = "\u0000"

        /**
         * Stored apart from the catalogue, on purpose.
         *
         * The catalogue is what arrived and is overwritten whole on every sync;
         * this is what the person chose, and a sync must not touch it. Kept in
         * one file, the next sync would wipe the ordering — and that shows up
         * as "my ordering did not save", a bug nobody can report clearly.
         */
        fun load(context: Context): ConsoleArrangement {
            val p = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            return ConsoleArrangement(
                hiddenActions = ints(p.getString(K_HIDDEN_ACTIONS, "")).toSet(),
                actionOrder = ints(p.getString(K_ACTION_ORDER, "")),
                hiddenApps = strings(p.getString(K_HIDDEN_APPS, "")).toSet(),
                appOrder = strings(p.getString(K_APP_ORDER, "")),
            )
        }

        fun save(context: Context, a: ConsoleArrangement) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString(K_HIDDEN_ACTIONS, a.hiddenActions.joinToString(","))
                .putString(K_ACTION_ORDER, a.actionOrder.joinToString(","))
                .putString(K_HIDDEN_APPS, a.hiddenApps.joinToString(SEP))
                .putString(K_APP_ORDER, a.appOrder.joinToString(SEP))
                .apply()
        }

        fun forget(context: Context) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().clear().apply()
        }

        private fun ints(raw: String?): List<Int> =
            raw.orEmpty().split(",").mapNotNull { it.trim().toIntOrNull() }

        private fun strings(raw: String?): List<String> =
            raw.orEmpty().split(SEP).filter { it.isNotEmpty() }

        /**
         * Drop what the catalogue no longer contains.
         *
         * Not merely ignored at draw time — actually removed, and on every sync.
         * A freed cmd byte is reused once the other 240 are spent, and a stale
         * 「hidden」 record would then make a brand-new action invisible from the
         * moment it arrives, with nothing on screen able to explain why.
         */
        fun prune(cat: ConsoleCatalogue, a: ConsoleArrangement): ConsoleArrangement {
            val bytes = cat.apps.flatMap { app -> app.actions.map { it.cmdByte } }.toSet()
            val names = cat.apps.map { it.name }.toSet()
            return ConsoleArrangement(
                hiddenActions = a.hiddenActions.filterTo(LinkedHashSet()) { it in bytes },
                actionOrder = a.actionOrder.filter { it in bytes },
                hiddenApps = a.hiddenApps.filterTo(LinkedHashSet()) { it in names },
                appOrder = a.appOrder.filter { it in names },
            )
        }

        /**
         * The catalogue as this phone wants to see it.
         *
         * [includeHidden] is what the arranging screen draws: it has to show the
         * things you turned off, or turning one off would be irreversible from
         * the only screen that can turn it back on.
         *
         * An app with nothing visible left is dropped from the normal view. A
         * tab you can open onto an empty card is a tab that cannot explain
         * itself.
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
        ): List<ConsoleApp> {
            val apps = inOrder(cat.apps, a.appOrder) { it.name }
                .filter { includeHidden || it.name !in a.hiddenApps }
            return apps.map { app ->
                ConsoleApp(
                    app.name,
                    inOrder(app.actions, a.actionOrder) { it.cmdByte }
                        .filter { includeHidden || it.cmdByte !in a.hiddenActions },
                )
            }.filter { includeHidden || it.actions.isNotEmpty() }
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
         * Move one item and write down the WHOLE resulting order.
         *
         * Storing just the moved item would leave the rest to fall back on the
         * Mac's order, so one drag could shuffle things nobody touched.
         */
        fun <K> moved(current: List<K>, from: Int, to: Int): List<K> {
            if (from !in current.indices || to !in current.indices || from == to) return current
            val out = current.toMutableList()
            out.add(to, out.removeAt(from))
            return out
        }
    }
}
