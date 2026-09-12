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
        MacState.reset()
        assertTrue(MacState.sightings(t0).isEmpty())
    }

    @Test fun `a fresh sighting is believed`() {
        MacState.reset()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        assertEquals(MacLockState.LOCKED, MacState.sightings(t0 + 1_000).single().state)
    }

    @Test fun `a stale sighting decays to unknown rather than to a guess`() {
        MacState.reset()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        val stale = t0 + SpikeContract.MAC_STATE_STALE_MS + 1
        assertTrue(MacState.sightings(stale).isEmpty())
    }

    @Test fun `a newer beacon from the same Mac replaces the older one`() {
        MacState.reset()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        MacState.heard(macId = MAC_A, locked = false, nowUptime = t0 + 500)
        assertEquals(MacLockState.UNLOCKED, MacState.sightings(t0 + 1_000).single().state)
    }

    @Test fun `two Macs are two entries, not one that overwrites the other`() {
        // The bug this replaces: with a single slot, a locked Mac in the next
        // room became the answer to「你的 Mac 锁了吗」about the one in front of
        // you, simply by advertising more recently.
        MacState.reset()
        MacState.heard(macId = MAC_A, locked = true, nowUptime = t0)
        MacState.heard(macId = MAC_B, locked = false, nowUptime = t0 + 500)
        val seen = MacState.sightings(t0 + 1_000)
        assertEquals(2, seen.size)
        assertEquals(MacLockState.LOCKED, seen.first { it.macId == MAC_A }.state)
        assertEquals(MacLockState.UNLOCKED, seen.first { it.macId == MAC_B }.state)
    }

    @Test fun `walking away from one Mac does not forget another still in range`() {
        MacState.reset()
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
        MacState.reset()
        MacState.heard(macId = MAC_A, beacon = MacBeaconState.CAL_NEAR, nowUptime = t0)
        val s = MacState.sightings(t0 + 1_000).single()
        assertEquals(MacBeaconState.CAL_NEAR, s.beacon)
        assertEquals(MacLockState.UNKNOWN, s.state)
    }

    @Test fun `the beacon for one Mac is readable by its id`() {
        MacState.reset()
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
        MacState.reset()
        MacState.heard(macId = MAC_A, beacon = MacBeaconState.UNLOCKED, keyId = 166, nowUptime = t0)
        MacState.heard(macId = MAC_B, beacon = MacBeaconState.LOCKED, keyId = 17, nowUptime = t0)
        assertEquals(MAC_A, MacState.sightingFor(166, t0 + 500)?.macId)
        assertEquals(MacLockState.LOCKED, MacState.sightingFor(17, t0 + 500)?.state)
        assertEquals(null, MacState.sightingFor(15, t0 + 500))
        assertEquals(null, MacState.sightingFor(166, t0 + SpikeContract.MAC_STATE_STALE_MS + 1))
        assertEquals(166, MacState.sightings(t0 + 500).first { it.macId == MAC_A }.keyId)
    }

    // ---- 「刚才还听得到」: a memory that outlives the staleness window ----

    @Test fun `a stale sighting is gone from sightings but when it was heard is still known`() {
        // Two different questions about one entry. sightings() answers「它现在
        // 在吗」and must forget a Mac after 20 s of silence. The card's「刚才还
        // 听得到，现在听不到了」answers「它刚刚还在吗」, which needs the time of
        // the last beacon long after the beacon itself stopped counting.
        MacState.reset()
        MacState.heard(macId = MAC_A, beacon = MacBeaconState.LOCKED, keyId = 166, nowUptime = t0)
        val stale = t0 + SpikeContract.MAC_STATE_STALE_MS + 1
        assertTrue(MacState.sightings(stale).isEmpty())
        assertEquals(null, MacState.sightingFor(166, stale))
        assertEquals(t0, MacState.lastHeardFor(166))
        assertEquals(t0, MacState.lastHeardAt(MAC_A))
    }

    @Test fun `never heard means no time at all, not zero`() {
        MacState.reset()
        assertEquals(null, MacState.lastHeardFor(166))
        assertEquals(null, MacState.lastHeardAt(MAC_A))
    }

    @Test fun `the time remembered for a slot is its newest beacon`() {
        MacState.reset()
        MacState.heard(macId = MAC_A, beacon = MacBeaconState.LOCKED, keyId = 166, nowUptime = t0)
        MacState.heard(macId = MAC_A, beacon = MacBeaconState.UNLOCKED, keyId = 166, nowUptime = t0 + 5_000)
        MacState.heard(macId = MAC_B, beacon = MacBeaconState.LOCKED, keyId = 17, nowUptime = t0 + 1_000)
        assertEquals(t0 + 5_000, MacState.lastHeardFor(166))
        assertEquals(t0 + 1_000, MacState.lastHeardFor(17))
        assertEquals(null, MacState.lastHeardFor(15))
    }

    // ---- The card's sentence: three kinds of silence, and one that is the phone's ----

    private val heardNow = MacSighting(MAC_A, MacLockState.LOCKED, MacBeaconState.LOCKED, keyId = 166)

    @Test fun `heard now says what the Mac said, whatever the phone's radio is doing`() {
        assertEquals("锁着。走过去，密码框留空，按回车。", macCardSentence(heardNow, t0, t0, phoneOnAir = true))
        // Still starting up and already hearing the Mac: the Mac's word wins.
        assertEquals("锁着。走过去，密码框留空，按回车。", macCardSentence(heardNow, t0, t0, phoneOnAir = false))
        val open = heardNow.copy(state = MacLockState.UNLOCKED, beacon = MacBeaconState.UNLOCKED)
        assertEquals("开着，不用解锁。", macCardSentence(open, t0, t0, phoneOnAir = true))
        val measuring = heardNow.copy(state = MacLockState.UNKNOWN, beacon = MacBeaconState.CAL_NEAR)
        assertEquals("正在量距离。看手机上量距离那一屏，一两分钟就好。", macCardSentence(measuring, t0, t0, phoneOnAir = true))
    }

    @Test fun `not heard because this phone is not broadcasting blames the phone, not the Mac`() {
        val s = macCardSentence(null, lastHeard = t0, now = t0 + 30_000, phoneOnAir = false)
        assertEquals("这部手机的钥匙没在广播，所以听不到它。", s)
        assertEquals(s, macCardSentence(null, lastHeard = null, now = t0, phoneOnAir = false))
    }

    @Test fun `heard within ten minutes but not now is 刚才`() {
        val justNow = t0 + SpikeContract.MAC_STATE_STALE_MS + 1
        assertEquals(
            "刚才还听得到，现在听不到了。走远了的话，它已经自己锁上。",
            macCardSentence(null, lastHeard = t0, now = justNow, phoneOnAir = true),
        )
        val edge = t0 + MacState.RECENTLY_HEARD_MS
        assertEquals(
            "刚才还听得到，现在听不到了。走远了的话，它已经自己锁上。",
            macCardSentence(null, lastHeard = t0, now = edge, phoneOnAir = true),
        )
    }

    @Test fun `never heard, or heard long ago, is the ordinary silence`() {
        val ordinary = "没听到它。可能不在附近、睡着了，或者没开 Outsie。"
        assertEquals(ordinary, macCardSentence(null, lastHeard = null, now = t0, phoneOnAir = true))
        val longAgo = t0 + MacState.RECENTLY_HEARD_MS + 1
        assertEquals(ordinary, macCardSentence(null, lastHeard = t0, now = longAgo, phoneOnAir = true))
    }

    @Test fun `forgetting what is heard now keeps when it was last heard`() {
        // forget() runs every time the phone's own radio is torn down -- a
        // Bluetooth cycle included. The card's 「刚才还听得到」 is about the Mac,
        // and must not be erased by this phone's radio blinking.
        MacState.reset()
        MacState.heard(macId = MAC_A, beacon = MacBeaconState.LOCKED, keyId = 166, nowUptime = t0)
        MacState.forget()
        assertTrue(MacState.sightings(t0 + 1).isEmpty())
        assertEquals(t0, MacState.lastHeardFor(166))
        MacState.reset()
        assertEquals(null, MacState.lastHeardFor(166))
    }
}
