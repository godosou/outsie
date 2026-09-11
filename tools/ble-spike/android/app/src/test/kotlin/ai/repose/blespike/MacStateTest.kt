package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The screen says「按回车就能进」on the strength of this, so the failure that
 * matters is a belief that outlives the evidence for it.
 */
class MacStateTest {

    @Test fun `nothing heard means unknown, not unlocked`() {
        MacState.forget()
        assertEquals(MacLockState.UNKNOWN, MacState.current(1_000))
    }

    @Test fun `a fresh sighting is believed`() {
        MacState.forget()
        MacState.heard(locked = true)
        assertEquals(MacLockState.LOCKED, MacState.current(SystemClockShim.now() + 1_000))
    }

    @Test fun `a stale sighting decays to unknown rather than to a guess`() {
        MacState.forget()
        MacState.heard(locked = true)
        val stale = SystemClockShim.now() + SpikeContract.MAC_STATE_STALE_MS + 1
        assertEquals(MacLockState.UNKNOWN, MacState.current(stale))
    }

    /** Reads the same clock MacState stamps with, so the two cannot drift. */
    private object SystemClockShim {
        fun now(): Long = android.os.SystemClock.elapsedRealtime()
    }
}
