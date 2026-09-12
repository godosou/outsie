package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Calibration is driven from the phone (design doc §05 §06): the phone sends
 * 「开始量」, the Mac samples and reports its phase in the state beacon, and the
 * phone's screen follows that phase. This is the follow.
 *
 * The one rule that matters most: the far leg never starts on its own. The
 * walk itself would land in the far set, and that is how you get 「两边太像」.
 */
class CalFlowTest {

    private val t0 = 1_000_000L

    @Test fun `the near leg ends when the Mac says it is waiting, and stops there`() {
        val f = CalFlow.start(t0).on(MacBeaconState.CAL_NEAR, t0 + 1000)
        assertEquals(CalStep.NEAR, f.step)
        val w = f.on(MacBeaconState.CAL_WAIT, t0 + 16_000)
        assertEquals(CalStep.WALK, w.step)
        // Still walking: the Mac keeps saying WAIT and nothing moves. Only the
        // person's 「到了」 starts the far leg.
        assertEquals(CalStep.WALK, w.on(MacBeaconState.CAL_WAIT, t0 + 60_000).step)
    }

    @Test fun `arriving starts the far leg, and the Mac's verdict ends it`() {
        val f = CalFlow.start(t0).on(MacBeaconState.CAL_WAIT, t0 + 16_000).arrived(t0 + 30_000)
        assertEquals(CalStep.FAR, f.step)
        assertEquals(CalStep.OK, f.on(MacBeaconState.CAL_OK, t0 + 50_000).step)
        assertEquals(CalStep.FAIL_ALIKE, f.on(MacBeaconState.CAL_FAIL, t0 + 50_000).step)
        assertEquals(CalStep.FAIL_SILENT, f.on(MacBeaconState.CAL_SILENT, t0 + 50_000).step)
    }

    @Test fun `the Mac restarting between far and verdict is not a failure`() {
        // After a good calibration the Mac restarts its pipeline to load the new
        // thresholds; for a few seconds nothing is heard. That silence must not
        // read as 「没回音」.
        val f = CalFlow.start(t0).on(MacBeaconState.CAL_WAIT, t0 + 16_000).arrived(t0 + 30_000)
        assertEquals(CalStep.FAR, f.on(null, t0 + 30_000 + 20_000).step)
        assertEquals(CalStep.OK, f.on(null, t0 + 50_000).on(MacBeaconState.CAL_OK, t0 + 55_000).step)
    }

    @Test fun `no answer at all, for long enough, is said as such`() {
        val f = CalFlow.start(t0)
        assertEquals(CalStep.NEAR, f.on(null, t0 + CalFlow.NO_ANSWER_MS - 1).step)
        assertEquals(CalStep.NO_ANSWER, f.on(null, t0 + CalFlow.NO_ANSWER_MS + 1).step)
        // A Mac still reporting the ordinary lock states never picked up the
        // command -- that is also no answer.
        assertEquals(CalStep.NO_ANSWER, f.on(MacBeaconState.UNLOCKED, t0 + CalFlow.NO_ANSWER_MS + 1).step)
    }

    @Test fun `a verdict screen does not move on its own`() {
        val ok = CalFlow(CalStep.OK, t0)
        assertEquals(CalStep.OK, ok.on(MacBeaconState.LOCKED, t0 + 99_000).step)
        assertEquals(CalStep.OK, ok.on(null, t0 + 999_000).step)
    }

    @Test fun `state bytes map to beacon states, and unknown bytes to nothing`() {
        assertEquals(MacBeaconState.UNLOCKED, MacBeaconState.of(0))
        assertEquals(MacBeaconState.LOCKED, MacBeaconState.of(1))
        assertEquals(MacBeaconState.CAL_NEAR, MacBeaconState.of(2))
        assertEquals(MacBeaconState.CAL_WAIT, MacBeaconState.of(3))
        assertEquals(MacBeaconState.CAL_FAR, MacBeaconState.of(4))
        assertEquals(MacBeaconState.CAL_OK, MacBeaconState.of(5))
        assertEquals(MacBeaconState.CAL_FAIL, MacBeaconState.of(6))
        assertEquals(MacBeaconState.CAL_SILENT, MacBeaconState.of(7))
        assertEquals(null, MacBeaconState.of(8))
        assertEquals(null, MacBeaconState.of(255))
    }
}
