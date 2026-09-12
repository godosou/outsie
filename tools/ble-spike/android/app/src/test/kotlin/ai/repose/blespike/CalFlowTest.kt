package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Calibration is driven from the phone (design doc §05 §06): the phone sends
 * 「开始量」, the Mac samples for a fixed 20 s and reports its phase in the state
 * beacon, and the phone's screen follows that phase. This is the follow.
 *
 * Two rules matter most. The far leg never starts on its own -- the walk itself
 * would land in the far set, and that is how you get 「两边太像」. And silence at
 * the far spot is an answer, not a failure: the 「到了」 press is itself a radio
 * command, so a far spot the Mac cannot hear is one where the command never
 * lands. The phone notices that by itself, walks you back, and tells the Mac
 * 「远处结束」 when it is in range again.
 */
class CalFlowTest {

    private val t0 = 1_000_000L

    @Test fun `the near leg ends when the Mac says it is waiting, and stops there`() {
        val f = CalFlow.start(t0).on(MacBeaconState.CAL_NEAR, t0 + 1000)
        assertEquals(CalStep.NEAR, f.step)
        val w = f.on(MacBeaconState.CAL_WAIT, t0 + 21_000)
        assertEquals(CalStep.WALK, w.step)
        // Still walking: the Mac keeps saying WAIT and nothing moves. Only the
        // person's 「到了」 starts the far leg.
        assertEquals(CalStep.WALK, w.on(MacBeaconState.CAL_WAIT, t0 + 60_000).step)
    }

    @Test fun `the near countdown starts when the Mac confirms the leg, not at the press`() {
        // The command takes a second or three to land. Counting from the press
        // would have the phone reach zero while the Mac is still sampling.
        val f = CalFlow.start(t0)
        assertEquals(null, f.remainingMs(t0 + 1000))
        val seen = f.on(MacBeaconState.CAL_NEAR, t0 + 2000)
        assertEquals(CalFlow.LEG_MS, seen.remainingMs(t0 + 2000))
        assertEquals(10_000L, seen.remainingMs(t0 + 12_000))
        assertEquals(0L, seen.remainingMs(t0 + 40_000))
        // A second confirmation does not restart the clock.
        assertEquals(10_000L, seen.on(MacBeaconState.CAL_NEAR, t0 + 5000).remainingMs(t0 + 12_000))
    }

    @Test fun `arriving starts the far leg, and the Mac's verdict ends it`() {
        val f = CalFlow.start(t0).on(MacBeaconState.CAL_WAIT, t0 + 21_000).arrived(t0 + 30_000)
        assertEquals(CalStep.FAR, f.step)
        assertEquals(CalStep.OK, f.on(MacBeaconState.CAL_OK, t0 + 50_000).step)
        assertEquals(CalStep.FAIL_ALIKE, f.on(MacBeaconState.CAL_FAIL, t0 + 50_000).step)
        assertEquals(CalStep.FAIL_SILENT, f.on(MacBeaconState.CAL_SILENT, t0 + 50_000).step)
    }

    @Test fun `the far countdown runs from the press`() {
        // The Mac may never hear this command, so there may be nothing to
        // count from but the press itself.
        val f = CalFlow.start(t0).on(MacBeaconState.CAL_WAIT, t0 + 21_000).arrived(t0 + 30_000)
        assertEquals(CalFlow.LEG_MS, f.remainingMs(t0 + 30_000))
        assertEquals(15_000L, f.remainingMs(t0 + 35_000))
        assertEquals(0L, f.remainingMs(t0 + 60_000))
    }

    @Test fun `a far spot the Mac cannot hear ends in walking back`() {
        // The Mac never confirmed the far leg: it kept saying WAIT, or nothing
        // at all. When the countdown is over the phone concludes the far spot
        // is out of range -- which is the clearest possible answer -- and
        // walks you back to tell the Mac so.
        val f = CalFlow.start(t0).on(MacBeaconState.CAL_WAIT, t0 + 21_000).arrived(t0 + 30_000)
        assertEquals(CalStep.FAR, f.on(null, t0 + 30_000 + CalFlow.LEG_MS - 1).step)
        assertEquals(CalStep.FAR, f.on(MacBeaconState.CAL_WAIT, t0 + 30_000 + CalFlow.LEG_MS - 1).step)
        assertEquals(CalStep.RETURN, f.on(null, t0 + 30_000 + CalFlow.LEG_MS).step)
        assertEquals(CalStep.RETURN, f.on(MacBeaconState.CAL_WAIT, t0 + 30_000 + CalFlow.LEG_MS).step)
    }

    @Test fun `a far leg the Mac confirmed waits a little longer for its verdict`() {
        // The Mac heard 「到了」 and is sampling. Its 20 s run a little behind
        // the phone's, and after a good verdict it restarts its pipeline and
        // says nothing for a few seconds. Neither is 「没回音」.
        val pressed = t0 + 30_000
        val f = CalFlow.start(t0).on(MacBeaconState.CAL_WAIT, t0 + 21_000).arrived(pressed)
        val seen = f.on(MacBeaconState.CAL_FAR, pressed + 2000)
        assertEquals(CalStep.FAR, seen.step)
        assertEquals(CalStep.FAR, seen.on(null, pressed + CalFlow.LEG_MS).step)
        val grace = pressed + 2000 + CalFlow.LEG_MS + CalFlow.VERDICT_GRACE_MS
        assertEquals(CalStep.FAR, seen.on(null, grace - 1).step)
        assertEquals(CalStep.OK, seen.on(null, grace - 1).on(MacBeaconState.CAL_OK, grace).step)
        // No verdict even after the grace: walk back, and ask for one there.
        assertEquals(CalStep.RETURN, seen.on(null, grace).step)
    }

    @Test fun `walking back ends with the Mac's verdict`() {
        val back = CalFlow(CalStep.RETURN, t0)
        assertEquals(CalStep.OK, back.on(MacBeaconState.CAL_OK, t0 + 30_000).step)
        assertEquals(CalStep.FAIL_ALIKE, back.on(MacBeaconState.CAL_FAIL, t0 + 30_000).step)
        assertEquals(CalStep.FAIL_SILENT, back.on(MacBeaconState.CAL_SILENT, t0 + 30_000).step)
        // The Mac picked the far command up late and is still sampling; the
        // 「远处结束」 it is about to hear will cut that short. Stay.
        assertEquals(CalStep.RETURN, back.on(MacBeaconState.CAL_FAR, t0 + 30_000).step)
        assertEquals(CalStep.RETURN, back.on(MacBeaconState.UNLOCKED, t0 + 30_000).step)
    }

    @Test fun `walking back with no verdict, for long enough, is no answer`() {
        val back = CalFlow(CalStep.RETURN, t0)
        assertEquals(CalStep.RETURN, back.on(null, t0 + CalFlow.RETURN_NO_ANSWER_MS - 1).step)
        assertEquals(CalStep.NO_ANSWER, back.on(null, t0 + CalFlow.RETURN_NO_ANSWER_MS + 1).step)
        assertEquals(CalStep.NO_ANSWER, back.on(MacBeaconState.LOCKED, t0 + CalFlow.RETURN_NO_ANSWER_MS + 1).step)
    }

    @Test fun `no answer at all, for long enough, is said as such`() {
        val f = CalFlow.start(t0)
        assertEquals(CalStep.NEAR, f.on(null, t0 + CalFlow.NO_ANSWER_MS - 1).step)
        assertEquals(CalStep.NO_ANSWER, f.on(null, t0 + CalFlow.NO_ANSWER_MS + 1).step)
        // A Mac still reporting the ordinary lock states never picked up the
        // command -- that is also no answer.
        assertEquals(CalStep.NO_ANSWER, f.on(MacBeaconState.UNLOCKED, t0 + CalFlow.NO_ANSWER_MS + 1).step)
        // Once the Mac has confirmed the leg, the clock runs from there.
        val seen = f.on(MacBeaconState.CAL_NEAR, t0 + 5000)
        assertEquals(CalStep.NEAR, seen.on(null, t0 + CalFlow.NO_ANSWER_MS + 1).step)
        assertEquals(CalStep.NO_ANSWER, seen.on(null, t0 + 5000 + CalFlow.NO_ANSWER_MS + 1).step)
    }

    @Test fun `a verdict screen does not move on its own`() {
        val ok = CalFlow(CalStep.OK, t0)
        assertEquals(CalStep.OK, ok.on(MacBeaconState.LOCKED, t0 + 99_000).step)
        assertEquals(CalStep.OK, ok.on(null, t0 + 999_000).step)
        assertEquals(null, ok.remainingMs(t0 + 1000))
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

    @Test fun `the far-done byte is the one the spec names`() {
        // Protocol §14: 6 = 远处结束，定下来. The Mac's watcher keys on the number.
        assertEquals(6, SpikeContract.CMD_CALIBRATE_FAR_DONE)
    }
}
