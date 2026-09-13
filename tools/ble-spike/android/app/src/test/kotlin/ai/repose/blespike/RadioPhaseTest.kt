package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The state of 「当你的钥匙」 as one derived value (design doc §04 手机·主屏).
 *
 * The bug this replaces: the service kept two booleans (running, advertising)
 * and the screen printed 「正在启动…」 whenever the first was true and the
 * second false -- which is also what a service looks like after Bluetooth
 * was switched off underneath it, forever. The facts below are what the
 * service actually knows; the phase is derived, never stored, so it cannot
 * go stale on its own.
 */
class RadioPhaseTest {

    private val t0 = 1_000_000L
    private fun facts(
        running: Boolean = true,
        hasKeys: Boolean = true,
        bluetoothOn: Boolean = true,
        advertising: Boolean = false,
        failure: String? = null,
        startingSince: Long? = t0,
    ) = RadioFacts(running, hasKeys, bluetoothOn, advertising, failure, startingSince)

    @Test fun `off means the service is not running, whatever else is true`() {
        assertEquals(RadioPhase.OFF, facts(running = false, advertising = true).phase(t0))
        assertEquals(RadioPhase.OFF, facts(running = false, bluetoothOn = false).phase(t0))
    }

    @Test fun `no key comes before everything but off`() {
        assertEquals(RadioPhase.NO_KEY, facts(hasKeys = false, bluetoothOn = false).phase(t0))
        assertEquals(RadioPhase.NO_KEY, facts(hasKeys = false, advertising = true).phase(t0))
    }

    @Test fun `bluetooth off is its own phase, not starting`() {
        // The phone's own radio is the reason; the sentence has to say so and
        // say what ends it (turning Bluetooth on), not 「正在启动…」.
        val f = facts(bluetoothOn = false)
        assertEquals(RadioPhase.BLUETOOTH_OFF, f.phase(t0))
        assertEquals(RadioPhase.BLUETOOTH_OFF, f.phase(t0 + 3_600_000))
    }

    @Test fun `starting is brief, and becomes failed when it is not`() {
        val f = facts(startingSince = t0)
        assertEquals(RadioPhase.STARTING, f.phase(t0 + RadioFacts.STARTING_GRACE_MS - 1))
        assertEquals(RadioPhase.FAILED, f.phase(t0 + RadioFacts.STARTING_GRACE_MS + 1))
    }

    @Test fun `a reported failure wins over starting`() {
        assertEquals(RadioPhase.FAILED, facts(failure = "广播失败：3").phase(t0))
    }

    @Test fun `on is advertising confirmed by the callback`() {
        assertEquals(RadioPhase.ON, facts(advertising = true).phase(t0))
        // A callback failure after a success is still a failure.
        assertEquals(RadioPhase.FAILED, facts(advertising = true, failure = "x").phase(t0))
    }

    @Test fun `every phase has a sentence that says what ends the wait`() {
        for (p in RadioPhase.values()) {
            val s = p.sentence(failure = "广播失败：3")
            assertTrue("$p: $s", s.endsWith("。"))
        }
        assertEquals("关着。现在谁都认不出这部手机。", RadioPhase.OFF.sentence(null))
        assertEquals("还没配对，Mac 认不出这部手机。", RadioPhase.NO_KEY.sentence(null))
        assertEquals("手机的蓝牙关着。打开蓝牙，它自己接着广播。", RadioPhase.BLUETOOTH_OFF.sentence(null))
        assertEquals("正在开始广播…几秒就好。", RadioPhase.STARTING.sentence(null))
        assertEquals("开着。附近的 Mac 认得你。", RadioPhase.ON.sentence(null))
        assertEquals("广播没开起来：广播失败：3。关掉再开一次。", RadioPhase.FAILED.sentence("广播失败：3"))
        assertEquals("广播没开起来。关掉再开一次。", RadioPhase.FAILED.sentence(null))
    }

    @Test fun `the switch shows what you asked for, not what the radio managed`() {
        // Bluetooth off with the key wanted on: the switch stays on and the
        // sentence explains. Snapping it off would tell you to switch it on
        // again, which changes nothing.
        assertTrue(facts(bluetoothOn = false).phase(t0).switchOn)
        assertTrue(RadioPhase.STARTING.switchOn)
        assertTrue(RadioPhase.FAILED.switchOn)
        assertTrue(RadioPhase.ON.switchOn)
        assertEquals(false, RadioPhase.OFF.switchOn)
        assertEquals(false, RadioPhase.NO_KEY.switchOn)
    }

    @Test fun `only on can carry a command or hear a Mac`() {
        assertTrue(RadioPhase.ON.onAir)
        for (p in RadioPhase.values()) if (p != RadioPhase.ON) assertEquals(false, p.onAir)
    }
}
