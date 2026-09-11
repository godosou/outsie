package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The screen says「走过去，密码框留空，按一下回车就能进」on the strength of this,
 * so the failure that matters is a belief that outlives the evidence for it.
 *
 * The clock is injected. An earlier version of this test let MacState read the
 * real one, which in a unit test is stubbed to 0 -- and 0 is exactly how
 * MacState records "never heard anything", so the test failed on the stub
 * rather than on the behaviour.
 */
class MacStateTest {

    private val t0 = 1_000_000L

    @Test fun `nothing heard means unknown, not unlocked`() {
        MacState.forget()
        assertEquals(MacLockState.UNKNOWN, MacState.current(t0))
    }

    @Test fun `a fresh sighting is believed`() {
        MacState.forget()
        MacState.heard(locked = true, nowUptime = t0)
        assertEquals(MacLockState.LOCKED, MacState.current(t0 + 1_000))
    }

    @Test fun `a stale sighting decays to unknown rather than to a guess`() {
        MacState.forget()
        MacState.heard(locked = true, nowUptime = t0)
        val stale = t0 + SpikeContract.MAC_STATE_STALE_MS + 1
        assertEquals(MacLockState.UNKNOWN, MacState.current(stale))
    }

    @Test fun `the newest sighting wins`() {
        MacState.forget()
        MacState.heard(locked = true, nowUptime = t0)
        MacState.heard(locked = false, nowUptime = t0 + 500)
        assertEquals(MacLockState.UNLOCKED, MacState.current(t0 + 1_000))
    }
}
