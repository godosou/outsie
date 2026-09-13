package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The 「让它一直在」 checklist (design doc §04 手机·主屏): what the phone must
 * allow for the key to survive the screen going dark, said as what each
 * thing gives you, shown only while something is missing. The guided step is
 * what a car-key app walks you through on these skins: 自启动、后台运行、不限耗电.
 */
class KeepAliveTest {

    private fun facts(
        bt: Boolean = true, notif: Boolean = true, batt: Boolean = true, bg: Boolean = true, brand: String = "realme",
    ) = KeepAliveFacts(bt, notif, batt, bg, brand)

    @Test fun `a realme phone gets the guided background step, a pixel does not`() {
        assertTrue(KeepAlive.needsBackgroundStep("realme"))
        assertTrue(KeepAlive.needsBackgroundStep("OPPO"))
        assertTrue(KeepAlive.needsBackgroundStep("Xiaomi"))
        assertTrue(KeepAlive.needsBackgroundStep("samsung"))
        assertFalse(KeepAlive.needsBackgroundStep("Google"))
        assertEquals(4, KeepAlive.items(facts()).size)
        assertEquals(3, KeepAlive.items(facts(brand = "Google")).size)
    }

    @Test fun `the guide names that skin's own switches`() {
        assertTrue(KeepAlive.backgroundGuide("realme").contains("完全允许后台行为"))
        assertTrue(KeepAlive.backgroundGuide("realme").contains("管理闲置应用"))
        assertTrue(KeepAlive.backgroundGuide("Xiaomi").contains("省电策略"))
        assertTrue(KeepAlive.backgroundGuide("HUAWEI").contains("应用启动管理"))
        assertTrue(KeepAlive.backgroundGuide("samsung").contains("不受限制"))
        assertTrue(KeepAlive.backgroundGuide("realme").contains("我做好了"))
    }

    @Test fun `every item says what it gives before what it needs, and ends with a full stop`() {
        for (item in KeepAlive.items(facts(bt = false, notif = false, batt = false, bg = false))) {
            assertTrue(item.title, item.title.isNotBlank())
            assertTrue(item.gives, item.gives.endsWith("。"))
            assertFalse(item.title, item.title.contains("权限"))
        }
    }

    @Test fun `the card leaves the screen only when everything required is done`() {
        assertTrue(KeepAlive.allDone(facts()))
        assertFalse(KeepAlive.allDone(facts(bt = false)))
        assertFalse(KeepAlive.allDone(facts(batt = false)))
        assertFalse(KeepAlive.allDone(facts(bg = false)))
        // Notifications are optional: the key works without the line in the shade.
        assertTrue(KeepAlive.allDone(facts(notif = false)))
        // A brand without the guided step does not wait for an acknowledgement.
        assertTrue(KeepAlive.allDone(facts(bg = false, brand = "Google")))
    }

    @Test fun `the summary counts what is missing, optional excluded`() {
        assertEquals("后台常驻：都设好了。", KeepAlive.summary(facts()))
        assertEquals("后台常驻：都设好了。", KeepAlive.summary(facts(notif = false)))
        assertEquals("后台常驻：还有 2 件没设。", KeepAlive.summary(facts(batt = false, bg = false)))
    }

    @Test fun `the order is the order things bite, notifications last`() {
        val steps = KeepAlive.items(facts()).map { it.step }
        assertEquals(listOf(KeepStep.BLUETOOTH, KeepStep.BATTERY, KeepStep.BACKGROUND, KeepStep.NOTIFICATIONS), steps)
    }

    @Test fun `the pictures are of that skin's own pages, and only where there are any`() {
        val realme = KeepAlive.pictures(KeepStep.BACKGROUND, "realme")
        assertEquals(listOf("keepalive_coloros_appinfo", "keepalive_coloros_power"), realme.map { it.first })
        assertTrue(realme.all { it.second.endsWith("。") })
        assertTrue(KeepAlive.pictures(KeepStep.BACKGROUND, "Google").isEmpty())
        assertTrue(KeepAlive.pictures(KeepStep.BATTERY, "realme").isEmpty())
    }
}
