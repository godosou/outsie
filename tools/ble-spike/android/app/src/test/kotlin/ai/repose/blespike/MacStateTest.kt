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
    private val MAC_A = 0xABCD
    private val MAC_B = 0x1234

    @Test fun `nothing heard means unknown, not unlocked`() {
        MacState.forget()
        assertEquals(MacLockState.UNKNOWN, MacState.current(t0))
    }

    @Test fun `a fresh sighting is believed`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        assertEquals(MacLockState.LOCKED, MacState.current(t0 + 1_000))
    }

    @Test fun `a stale sighting decays to unknown rather than to a guess`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        val stale = t0 + SpikeContract.MAC_STATE_STALE_MS + 1
        assertEquals(MacLockState.UNKNOWN, MacState.current(stale))
    }

    @Test fun `a newer beacon from the same Mac replaces the older one`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        MacState.heard(macId = MAC_A, locked = false, nowUptime = t0 + 500)
        assertEquals(MacLockState.UNLOCKED, MacState.current(t0 + 1_000))
    }

    @Test fun `two Macs are two entries, not one that overwrites the other`() {
        // The bug this replaces: with a single slot, a locked Mac in the next
        // room became the answer to「你的 Mac 锁了吗」about the one in front of
        // you, simply by advertising more recently.
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        MacState.heard(macId = MAC_B, locked = false, nowUptime = t0 + 500)
        val seen = MacState.sightings(t0 + 1_000)
        assertEquals(2, seen.size)
        assertEquals(MacLockState.LOCKED, seen.first { it.macId == MAC_A }.state)
        assertEquals(MacLockState.UNLOCKED, seen.first { it.macId == MAC_B }.state)
    }

    @Test fun `two Macs that disagree have no single answer`() {
        // Picking one would be the phone choosing which of two facts to show.
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        MacState.heard(macId = MAC_B, locked = false, nowUptime = t0 + 500)
        assertEquals(MacLockState.UNKNOWN, MacState.current(t0 + 1_000))
    }

    @Test fun `two Macs that agree do have one`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        MacState.heard(macId = MAC_B, locked = true, nowUptime = t0 + 500)
        assertEquals(MacLockState.LOCKED, MacState.current(t0 + 1_000))
    }

    @Test fun `walking away from one Mac does not forget another still in range`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        val later = t0 + SpikeContract.MAC_STATE_STALE_MS - 1_000
        MacState.heard(macId = MAC_B, locked = true, nowUptime = later)
        // A is now stale, B is not.
        val seen = MacState.sightings(later + 2_000)
        assertEquals(1, seen.size)
        assertEquals(MAC_B, seen[0].macId)
        assertEquals(MacLockState.LOCKED, MacState.current(later + 2_000))
    }
}
