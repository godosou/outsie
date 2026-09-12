package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The phone's own view of the Mac's catalogue.
 *
 * Every test here is about one of the two ways this feature can betray someone:
 * a sync that looks broken because it quietly hid something, and a sync that
 * silently re-points a preference at the wrong action.
 */
class ConsoleArrangementTest {

    private fun action(cmd: Int, name: String) = ConsoleAction(cmd, name, null, "")

    private fun catalogue(vararg apps: Pair<String, List<ConsoleAction>>) =
        ConsoleCatalogue(1, apps.map { ConsoleApp(it.first, it.second) })

    private val cat = catalogue(
        "tmux" to listOf(action(16, "左右分屏"), action(17, "上下分屏"), action(18, "关闭")),
        "Codex" to listOf(action(20, "新建任务")),
    )

    @Test
    fun `with no preferences the Mac's own order is what shows`() {
        val out = ConsoleArrangement.arrange(cat, ConsoleArrangement())
        assertEquals(listOf("tmux", "Codex"), out.map { it.name })
        assertEquals(listOf(16, 17, 18), out[0].actions.map { it.cmdByte })
    }

    @Test
    fun `hidden actions are not drawn, and the arranging screen still sees them`() {
        val a = ConsoleArrangement(hiddenActions = setOf(17))
        assertEquals(listOf(16, 18), ConsoleArrangement.arrange(cat, a)[0].actions.map { it.cmdByte })
        // Without this, turning one off would be irreversible from the only
        // screen that can turn it back on.
        assertEquals(
            listOf(16, 17, 18),
            ConsoleArrangement.arrange(cat, a, includeHidden = true)[0].actions.map { it.cmdByte },
        )
    }

    @Test
    fun `an app with nothing left visible disappears from the tab row`() {
        val a = ConsoleArrangement(hiddenActions = setOf(16, 17, 18))
        assertEquals(listOf("Codex"), ConsoleArrangement.arrange(cat, a).map { it.name })
        assertEquals(2, ConsoleArrangement.arrange(cat, a, includeHidden = true).size)
    }

    @Test
    fun `ordering moves what it names and leaves the rest where the Mac put them`() {
        val a = ConsoleArrangement(actionOrder = listOf(18, 16), appOrder = listOf("Codex"))
        val out = ConsoleArrangement.arrange(cat, a)
        assertEquals(listOf("Codex", "tmux"), out.map { it.name })
        assertEquals(listOf(18, 16, 17), out[1].actions.map { it.cmdByte })
    }

    @Test
    fun `an action the preferences have never heard of shows, and shows last`() {
        // The opposite default is a successful sync that looks exactly like a
        // broken one: you add it on the Mac, sync, and cannot find it.
        val grown = catalogue(
            "tmux" to listOf(action(16, "左右分屏"), action(17, "上下分屏"), action(18, "关闭"), action(99, "新的")),
        )
        val a = ConsoleArrangement(actionOrder = listOf(18, 17, 16), hiddenActions = setOf(17))
        val out = ConsoleArrangement.arrange(grown, a)
        assertEquals(listOf(18, 16, 99), out[0].actions.map { it.cmdByte })
    }

    @Test
    fun `deleting one action on the Mac does not re-point the preferences at its neighbour`() {
        // The whole reason preferences key on the cmd byte and not the index.
        val a = ConsoleArrangement(hiddenActions = setOf(18), actionOrder = listOf(18, 17, 16))
        val shrunk = catalogue("tmux" to listOf(action(16, "左右分屏"), action(17, "上下分屏")))
        val pruned = ConsoleArrangement.prune(shrunk, a)
        assertEquals(emptySet<Int>(), pruned.hiddenActions)
        assertEquals(listOf(17, 16), pruned.actionOrder)
        assertEquals(listOf(17, 16), ConsoleArrangement.arrange(shrunk, pruned)[0].actions.map { it.cmdByte })
    }

    @Test
    fun `a byte that left the catalogue is forgotten, not merely ignored`() {
        // Bytes are reused once the other 240 are spent. A stale 「hidden」 record
        // would make a brand-new action invisible from the moment it arrives.
        val a = ConsoleArrangement(hiddenActions = setOf(18))
        val pruned = ConsoleArrangement.prune(catalogue("tmux" to listOf(action(16, "左右分屏"))), a)
        val reborn = catalogue("tmux" to listOf(action(16, "左右分屏"), action(18, "完全不同的操作")))
        assertEquals(listOf(16, 18), ConsoleArrangement.arrange(reborn, pruned)[0].actions.map { it.cmdByte })
    }

    @Test
    fun `pruning also drops apps the Mac no longer has`() {
        val a = ConsoleArrangement(hiddenApps = setOf("Codex"), appOrder = listOf("Codex", "tmux"))
        val pruned = ConsoleArrangement.prune(catalogue("tmux" to listOf(action(16, "左右分屏"))), a)
        assertEquals(emptySet<String>(), pruned.hiddenApps)
        assertEquals(listOf("tmux"), pruned.appOrder)
        assertTrue(pruned.isDefault.not() || pruned.appOrder.isNotEmpty())
    }

    @Test
    fun `moving writes down the whole order, so untouched items stay untouched`() {
        assertEquals(listOf(17, 16, 18), ConsoleArrangement.moved(listOf(16, 17, 18), 1, 0))
        assertEquals(listOf(16, 18, 17), ConsoleArrangement.moved(listOf(16, 17, 18), 1, 2))
        assertEquals(listOf(16, 17, 18), ConsoleArrangement.moved(listOf(16, 17, 18), 1, 1))
        assertEquals(listOf(16, 17, 18), ConsoleArrangement.moved(listOf(16, 17, 18), 0, 9))
    }

    @Test
    fun `an app name with a space survives a round trip through storage`() {
        // Names come from the Mac and are whatever a person typed. A separator
        // that can appear inside one splits it in two, and the split halves
        // match nothing -- so the ordering silently resets.
        val a = ConsoleArrangement(hiddenApps = setOf("VS Code"), appOrder = listOf("VS Code", "飞书, 桌面版"))
        assertEquals(a.appOrder, roundTrip(a).appOrder)
        assertEquals(a.hiddenApps, roundTrip(a).hiddenApps)
    }

    /** What [ConsoleArrangement.save] then [ConsoleArrangement.load] does, without Android. */
    private fun roundTrip(a: ConsoleArrangement): ConsoleArrangement {
        return ConsoleArrangement(
            hiddenApps = a.hiddenApps.joinToString(ConsoleArrangement.SEP).split(ConsoleArrangement.SEP).filter { it.isNotEmpty() }.toSet(),
            appOrder = a.appOrder.joinToString(ConsoleArrangement.SEP).split(ConsoleArrangement.SEP).filter { it.isNotEmpty() },
        )
    }
}
