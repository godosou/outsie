package ai.repose.blespike

/** Where the phone's calibration screen is (design doc §05 §06). */
enum class CalStep { INTRO, NEAR, WALK, FAR, OK, FAIL_ALIKE, FAIL_SILENT, NO_ANSWER }

/**
 * The phone's side of a calibration, as a value.
 *
 * The phone sends 「开始」 and then only follows what the Mac's state beacon
 * says: the Mac is the ruler, the phone cannot hear its own signal. So every
 * transition here is either a button the person pressed or a phase the Mac
 * reported. Pure, so it is tested without a radio.
 *
 * THE FAR LEG NEVER STARTS ON ITS OWN. When the Mac says the near leg is done
 * (CAL_WAIT), this stops on WALK and waits for [arrived]. Starting to sample
 * while the person is still walking puts the walk into the far set, and that
 * is the most common way to get 「两边太像」.
 */
data class CalFlow(val step: CalStep, val since: Long) {

    /** The person pressed 「到了，开始量」 and the far command went out. */
    fun arrived(now: Long): CalFlow = if (step == CalStep.WALK) CalFlow(CalStep.FAR, now) else this

    /** What the Mac's latest beacon says, or null if nothing is heard. */
    fun on(beacon: MacBeaconState?, now: Long): CalFlow = when (step) {
        CalStep.NEAR -> when (beacon) {
            MacBeaconState.CAL_NEAR -> this
            MacBeaconState.CAL_WAIT -> CalFlow(CalStep.WALK, now)
            MacBeaconState.CAL_FAR -> CalFlow(CalStep.FAR, now)
            MacBeaconState.CAL_OK -> CalFlow(CalStep.OK, now)
            MacBeaconState.CAL_FAIL -> CalFlow(CalStep.FAIL_ALIKE, now)
            MacBeaconState.CAL_SILENT -> CalFlow(CalStep.FAIL_SILENT, now)
            // Lock states or silence: the Mac has not picked the command up.
            else -> if (now - since > NO_ANSWER_MS) CalFlow(CalStep.NO_ANSWER, now) else this
        }
        CalStep.FAR -> when (beacon) {
            MacBeaconState.CAL_OK -> CalFlow(CalStep.OK, now)
            MacBeaconState.CAL_FAIL -> CalFlow(CalStep.FAIL_ALIKE, now)
            MacBeaconState.CAL_SILENT -> CalFlow(CalStep.FAIL_SILENT, now)
            // Silence here is expected for a while: after a good far leg the
            // Mac restarts its pipeline to load the new thresholds, and says
            // nothing for a few seconds. Give it longer than the near leg.
            else -> if (now - since > FAR_NO_ANSWER_MS) CalFlow(CalStep.NO_ANSWER, now) else this
        }
        else -> this
    }

    companion object {
        /** The near leg needs 15 s of readings, plus the command's ride time. */
        const val NO_ANSWER_MS = 60_000L
        const val FAR_NO_ANSWER_MS = 120_000L

        /** After the near command went out. */
        fun start(now: Long) = CalFlow(CalStep.NEAR, now)
    }
}
