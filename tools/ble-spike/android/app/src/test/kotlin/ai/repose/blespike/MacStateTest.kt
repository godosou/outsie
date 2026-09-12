package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
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
        assertTrue(MacState.sightings(t0).isEmpty())
    }

    @Test fun `a fresh sighting is believed`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        assertEquals(MacLockState.LOCKED, MacState.sightings(t0 + 1_000).single().state)
    }

    @Test fun `a stale sighting decays to unknown rather than to a guess`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        val stale = t0 + SpikeContract.MAC_STATE_STALE_MS + 1
        assertTrue(MacState.sightings(stale).isEmpty())
    }

    @Test fun `a newer beacon from the same Mac replaces the older one`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        MacState.heard(macId = MAC_A, locked = false, nowUptime = t0 + 500)
        assertEquals(MacLockState.UNLOCKED, MacState.sightings(t0 + 1_000).single().state)
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

    @Test fun `walking away from one Mac does not forget another still in range`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        val later = t0 + SpikeContract.MAC_STATE_STALE_MS - 1_000
        MacState.heard(macId = MAC_B, locked = true, nowUptime = later)
        // A is now stale, B is not.
        val seen = MacState.sightings(later + 2_000)
        assertEquals(1, seen.size)
        assertEquals(MAC_B, seen[0].macId)
    }

    @Test fun `a calibrating Mac is heard but its lock state is not claimed`() {
        // While the Mac is measuring, its beacon carries the phase instead of
        // the lock state. The card must still count it as present (the buttons
        // stay live) without pretending to know whether it is locked.
        MacState.forget()
        MacState.heard(macId = MAC_A, beacon = MacBeaconState.CAL_NEAR, nowUptime = t0)
        val s = MacState.sightings(t0 + 1_000).single()
        assertEquals(MacBeaconState.CAL_NEAR, s.beacon)
        assertEquals(MacLockState.UNKNOWN, s.state)
    }

    @Test fun `the beacon for one Mac is readable by its id`() {
        MacState.forget()
        MacState.heard(macId = MAC_A, beacon = MacBeaconState.CAL_WAIT, nowUptime = t0)
        MacState.heard(macId = MAC_B, locked = true, nowUptime = t0)
        assertEquals(MacBeaconState.CAL_WAIT, MacState.beaconOf(MAC_A, t0 + 500))
        assertEquals(MacBeaconState.LOCKED, MacState.beaconOf(MAC_B, t0 + 500))
        assertEquals(null, MacState.beaconOf(0x9999, t0 + 500))
        assertEquals(null, MacState.beaconOf(MAC_A, t0 + SpikeContract.MAC_STATE_STALE_MS + 1))
    }

    @Test fun `a sighting is findable by the key slot it verified under`() {
        // The bug this replaces: the home card matched a Mac by the four hex
        // digits stored in its record, and a slot paired by an older build has
        // no record -- so the real Mac verified beacon after beacon and its card
        // stayed grey. The slot IS the identity: a tag that verified under key
        // 166 was minted by the Mac that holds key 166, whatever its id.
        MacState.forget()
        MacState.heard(macId = MAC_A, beacon = MacBeaconState.UNLOCKED, keyId = 166, nowUptime = t0)
        MacState.heard(macId = MAC_B, beacon = MacBeaconState.LOCKED, keyId = 17, nowUptime = t0)
        assertEquals(MAC_A, MacState.sightingFor(166, t0 + 500)?.macId)
        assertEquals(MacLockState.LOCKED, MacState.sightingFor(17, t0 + 500)?.state)
        assertEquals(null, MacState.sightingFor(15, t0 + 500))
        assertEquals(null, MacState.sightingFor(166, t0 + SpikeContract.MAC_STATE_STALE_MS + 1))
        assertEquals(166, MacState.sightings(t0 + 500).first { it.macId == MAC_A }.keyId)
    }
}
