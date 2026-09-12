package ai.repose.blespike

/** Where the phone's calibration screen is (design doc §05 §06). */
enum class CalStep { INTRO, NEAR, WALK, FAR, RETURN, OK, FAIL_ALIKE, FAIL_SILENT, NO_ANSWER }

/**
 * The phone's side of a calibration, as a value.
 *
 * The phone sends 「开始」 and then follows what the Mac's state beacon says:
 * the Mac is the ruler, the phone cannot hear its own signal. Every transition
 * here is a button the person pressed, a phase the Mac reported, or a clock
 * running out. Pure, so it is tested without a radio.
 *
 * THE FAR LEG NEVER STARTS ON ITS OWN. When the Mac says the near leg is done
 * (CAL_WAIT), this stops on WALK and waits for [arrived]. Starting to sample
 * while the person is still walking puts the walk into the far set, and that
 * is the most common way to get 「两边太像」.
 *
 * SILENCE AT THE FAR SPOT IS AN ANSWER. 「到了」 is itself a radio command. A far
 * spot the Mac cannot hear is one where that command never lands, and the
 * first version of this waited there for a Mac that could not possibly reply.
 * Now the phone counts its own 20 s; if the Mac never confirmed the leg by
 * then, the phone walks you back (RETURN) and keeps saying 「远处结束」 until
 * the Mac, in range again, records that far means silence.
 *
 * @property since when [step] began.
 * @property legSeen when the Mac first confirmed the current leg (CAL_NEAR for
 *   NEAR, CAL_FAR for FAR). The near countdown runs from here, not from the
 *   press: the command takes a second or three to land, and counting from the
 *   press would reach zero while the Mac is still sampling.
 */
data class CalFlow(val step: CalStep, val since: Long, val legSeen: Long? = null) {

    /** The person pressed 「到了，开始量」 and the far command went out. */
    fun arrived(now: Long): CalFlow = if (step == CalStep.WALK) CalFlow(CalStep.FAR, now) else this

    /**
     * Milliseconds left on the current leg's countdown, or null when there is
     * nothing to count: the near leg before the Mac has confirmed it, and every
     * step that is not a leg.
     *
     * The far leg counts from the press, because the Mac may never hear it and
     * there would then be nothing else to count from.
     */
    fun remainingMs(now: Long): Long? = when (step) {
        CalStep.NEAR -> legSeen?.let { (it + LEG_MS - now).coerceAtLeast(0L) }
        CalStep.FAR -> (since + LEG_MS - now).coerceAtLeast(0L)
        else -> null
    }

    /** What the Mac's latest beacon says, or null if nothing is heard. */
    fun on(beacon: MacBeaconState?, now: Long): CalFlow = when (step) {
        CalStep.NEAR -> when (beacon) {
            MacBeaconState.CAL_NEAR -> if (legSeen == null) copy(legSeen = now) else this
            MacBeaconState.CAL_WAIT -> CalFlow(CalStep.WALK, now)
            MacBeaconState.CAL_FAR -> CalFlow(CalStep.FAR, now, legSeen = now)
            MacBeaconState.CAL_OK -> CalFlow(CalStep.OK, now)
            MacBeaconState.CAL_FAIL -> CalFlow(CalStep.FAIL_ALIKE, now)
            MacBeaconState.CAL_SILENT -> CalFlow(CalStep.FAIL_SILENT, now)
            // Lock states or silence: the Mac has not picked the command up,
            // or dropped the phase. Once it confirmed, the clock runs from there.
            else -> if (now - (legSeen ?: since) > NO_ANSWER_MS) CalFlow(CalStep.NO_ANSWER, now) else this
        }
        CalStep.FAR -> when (beacon) {
            MacBeaconState.CAL_FAR -> if (legSeen == null) copy(legSeen = now) else this
            MacBeaconState.CAL_OK -> CalFlow(CalStep.OK, now)
            MacBeaconState.CAL_FAIL -> CalFlow(CalStep.FAIL_ALIKE, now)
            MacBeaconState.CAL_SILENT -> CalFlow(CalStep.FAIL_SILENT, now)
            else -> when (legSeen) {
                // The Mac never heard 「到了」: the far spot is out of range. When
                // the countdown is over, that is the finding. Walk back with it.
                null -> if (now >= since + LEG_MS) CalFlow(CalStep.RETURN, now) else this
                // The Mac is sampling. Its 20 s run a little behind this
                // phone's, and after a good verdict it restarts its pipeline
                // and says nothing for a few seconds. Neither is 「没回音」.
                else -> if (now >= legSeen + LEG_MS + VERDICT_GRACE_MS) CalFlow(CalStep.RETURN, now) else this
            }
        }
        CalStep.RETURN -> when (beacon) {
            MacBeaconState.CAL_OK -> CalFlow(CalStep.OK, now)
            MacBeaconState.CAL_FAIL -> CalFlow(CalStep.FAIL_ALIKE, now)
            MacBeaconState.CAL_SILENT -> CalFlow(CalStep.FAIL_SILENT, now)
            // CAL_FAR here means the Mac picked 「到了」 up late and is still
            // sampling; the 「远处结束」 it is about to hear cuts that short.
            // Lock states mean it is idle and has not heard that yet. Either
            // way: keep walking, keep asking.
            else -> if (now - since > RETURN_NO_ANSWER_MS) CalFlow(CalStep.NO_ANSWER, now) else this
        }
        else -> this
    }

    companion object {
        /** One leg, on both sides. The Mac samples for exactly this long. */
        const val LEG_MS = 20_000L

        /** After the far leg's 20 s: how long a confirmed leg may take to say its verdict. */
        const val VERDICT_GRACE_MS = 15_000L

        /** Before the Mac confirms a leg: 60 s covers the command's ride time many times over. */
        const val NO_ANSWER_MS = 60_000L

        /** Walking back and asking. Two minutes is a long walk. */
        const val RETURN_NO_ANSWER_MS = 120_000L

        /** How often 「远处结束」 goes out again while walking back. */
        const val RETURN_REPOST_MS = 10_000L

        /** After the near command went out. */
        fun start(now: Long) = CalFlow(CalStep.NEAR, now)
    }
}
