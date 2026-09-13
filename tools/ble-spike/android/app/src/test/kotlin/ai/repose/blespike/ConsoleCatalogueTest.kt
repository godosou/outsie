package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * A catalogue belongs to the Mac whose key signed it (design doc §04 §08).
 *
 * The control screen for one Mac must never borrow another Mac's buttons: the
 * buttons look the same, the bytes are another machine's.
 */
class ConsoleCatalogueTest {

    private val json = """{"apps":[{"n":"tmux","a":[{"b":16,"n":"左右分屏","k":"^b %"}]}]}"""

    @Test
    fun `parse keeps which key the catalogue came from`() {
        val cat = ConsoleCatalogue.parse(3, json, keyId = 166)
        assertEquals(166, cat!!.keyId)
        assertEquals(3, cat.revision)
        assertEquals("tmux", cat.apps[0].name)
    }

    @Test
    fun `a catalogue only answers for its own Mac`() {
        val cat = ConsoleCatalogue.parse(3, json, keyId = 166)!!
        assertEquals(cat, ConsoleCatalogue.own(cat, 166))
        assertNull(ConsoleCatalogue.own(cat, 17))
        assertNull(ConsoleCatalogue.own(null, 166))
    }

    @Test
    fun `a catalogue stored before keys were recorded belongs to nobody`() {
        // keyId 0 is "unknown". Showing it for every Mac would be exactly the
        // borrowing this test exists to forbid.
        val old = ConsoleCatalogue.parse(3, json, keyId = 0)!!
        assertNull(ConsoleCatalogue.own(old, 166))
    }

    @Test
    fun `each Mac has its own slot in storage`() {
        assertTrue(ConsoleCatalogue.slot(166) != ConsoleCatalogue.slot(17))
    }

    @Test
    fun `an App's own byte is parsed, and a Mac without one leaves it null`() {
        val withByte = """{"apps":[{"n":"飞书","b":40,"a":[{"b":41,"n":"搜索","k":"⌘F"}]}]}"""
        val cat = ConsoleCatalogue.parse(3, withByte, keyId = 72)!!
        assertEquals(40, cat.apps[0].cmdByte)
        assertEquals(41, cat.apps[0].actions[0].cmdByte)
        val old = ConsoleCatalogue.parse(3, json, keyId = 72)!!
        assertEquals(null, old.apps[0].cmdByte)
        // A byte below the shortcut base is a protocol command, never an App.
        val bad = ConsoleCatalogue.parse(3, """{"apps":[{"n":"x","b":1,"a":[{"b":16,"n":"y","k":""}]}]}""", keyId = 72)!!
        assertEquals(null, bad.apps[0].cmdByte)
    }
}
