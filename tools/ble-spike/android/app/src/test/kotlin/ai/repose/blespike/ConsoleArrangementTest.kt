package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The phone's own view of one Mac's catalogue (design doc §10).
 *
 * Every test is about one of the two ways this can betray someone: a sync that
 * looks broken because it quietly hid something, and a sync that silently
 * re-points a preference at the wrong action.
 */
class ConsoleArrangementTest {

    private fun action(cmd: Int, name: String) = ConsoleAction(cmd, name, null, "")

    private fun catalogue(vararg apps: Pair<String, List<ConsoleAction>>, keyId: Int = 166) =
        ConsoleCatalogue(1, apps.map { ConsoleApp(it.first, it.second) }, keyId)

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
    fun `hidden actions are not drawn, and the arranging view still sees them`() {
        val a = ConsoleArrangement(hidden = setOf(17))
        assertEquals(listOf(16, 18), ConsoleArrangement.arrange(cat, a)[0].actions.map { it.cmdByte })
        assertEquals(
            listOf(16, 17, 18),
            ConsoleArrangement.arrange(cat, a, includeHidden = true)[0].actions.map { it.cmdByte },
        )
    }

    @Test
    fun `an app with everything hidden stays in the row, with nothing in it`() {
        // §04: the chip stays and the grid says 「这个 App 的按钮都被你收起来了」.
        // A chip that vanished could not explain itself.
        val a = ConsoleArrangement(hidden = setOf(16, 17, 18))
        val out = ConsoleArrangement.arrange(cat, a)
        assertEquals(listOf("tmux", "Codex"), out.map { it.name })
        assertTrue(out[0].actions.isEmpty())
    }

    @Test
    fun `ordering moves what it names and leaves the rest where the Mac put them`() {
        val a = ConsoleArrangement(order = listOf(18, 16))
        assertEquals(listOf(18, 16, 17), ConsoleArrangement.arrange(cat, a)[0].actions.map { it.cmdByte })
    }

    @Test
    fun `an action the preferences have never heard of shows, and shows last`() {
        // The opposite default is a successful sync that looks exactly like a
        // broken one: you add it on the Mac, sync, and cannot find it.
        val grown = catalogue(
            "tmux" to listOf(action(16, "左右分屏"), action(17, "上下分屏"), action(18, "关闭"), action(99, "新的")),
        )
        val a = ConsoleArrangement(order = listOf(18, 17, 16), hidden = setOf(17))
        assertEquals(listOf(18, 16, 99), ConsoleArrangement.arrange(grown, a)[0].actions.map { it.cmdByte })
    }

    @Test
    fun `deleting one action on the Mac does not re-point the preferences at its neighbour`() {
        // The whole reason preferences key on the cmd byte and not the index.
        val a = ConsoleArrangement(hidden = setOf(18), order = listOf(18, 17, 16))
        val shrunk = catalogue("tmux" to listOf(action(16, "左右分屏"), action(17, "上下分屏")))
        val pruned = ConsoleArrangement.prune(shrunk, a)
        assertEquals(emptySet<Int>(), pruned.hidden)
        assertEquals(listOf(17, 16), pruned.order)
    }

    @Test
    fun `a byte that left the catalogue is forgotten, not merely ignored`() {
        // Bytes are reused once the other 240 are spent. A stale 「hidden」 would
        // make a brand-new action invisible from the moment it arrives.
        val a = ConsoleArrangement(hidden = setOf(18))
        val pruned = ConsoleArrangement.prune(catalogue("tmux" to listOf(action(16, "左右分屏"))), a)
        val reborn = catalogue("tmux" to listOf(action(16, "左右分屏"), action(18, "完全不同的操作")))
        assertEquals(listOf(16, 18), ConsoleArrangement.arrange(reborn, pruned)[0].actions.map { it.cmdByte })
    }

    @Test
    fun `dropping one tile on another writes the whole order, visible first then hidden`() {
        // §10: only storing the moved one would let a single drag shuffle
        // things nobody touched; un-hiding puts a tile back where it was.
        val a = ConsoleArrangement(hidden = setOf(17), order = listOf(16, 17, 18))
        val next = ConsoleArrangement.dropped(cat, "tmux", a, from = 16, onto = 18)
        assertEquals(listOf(18, 16, 17), next.order)
        assertEquals(listOf(18, 16), ConsoleArrangement.arrange(cat, next)[0].actions.map { it.cmdByte })
    }

    @Test
    fun `dropping keeps other apps' order untouched`() {
        val a = ConsoleArrangement(order = listOf(20))
        val next = ConsoleArrangement.dropped(cat, "tmux", a, from = 18, onto = 16)
        assertEquals(listOf(18, 16, 17, 20), next.order)
    }

    @Test
    fun `dropping a tile on itself or on something not in the app changes nothing`() {
        val a = ConsoleArrangement(order = listOf(16, 17, 18))
        assertEquals(a, ConsoleArrangement.dropped(cat, "tmux", a, from = 16, onto = 16))
        assertEquals(a, ConsoleArrangement.dropped(cat, "tmux", a, from = 16, onto = 20))
    }

    @Test
    fun `each Mac has its own slot in storage`() {
        // §04 §08: one catalogue and one arrangement per Mac, keyed on the key
        // that signed it. Two Macs must never share a preference key.
        assertTrue(ConsoleArrangement.slot(166) != ConsoleArrangement.slot(17))
        assertEquals(ConsoleArrangement.slot(166), ConsoleArrangement.slot(166))
    }
}
