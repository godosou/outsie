package ai.repose.blespike

import android.os.SystemClock

/**
 * What this phone believes about the paired Mac, and how it is allowed to say it.
 *
 * The presence beacon is one-way, so until now the phone knew nothing about the
 * Mac at all. Every sentence it showed was either about itself ("已发出") or an
 * invention. `repose-macstate-v2` is the Mac broadcasting its own lock state
 * under the same key, which finally makes「Mac 锁着，按回车就能进」a fact rather
 * than a guess.
 *
 * Three states, not two. "Unknown" is not a placeholder for a missing feature —
 * it is the ordinary state whenever the Mac is out of range, asleep, off, or
 * simply not running Outsie, and it is what the screen must show then. A
 * two-state model would have to pick one of the real answers to stand in for
 * "no idea", and both choices are wrong in a way the user would act on.
 */
enum class MacLockState { LOCKED, UNLOCKED, UNKNOWN }

/**
 * What the Mac's state beacon can say (protocol §14). 0 and 1 are the lock
 * state; 2..7 are the phases of a distance calibration the phone asked for.
 *
 * While the Mac is measuring, the byte carries the phase INSTEAD of the lock
 * state, so a calibrating Mac is heard but its lock state is unknown -- see
 * [lock].
 */
enum class MacBeaconState(val byte: Int) {
    UNLOCKED(0), LOCKED(1),
    /** Sampling the near leg. */
    CAL_NEAR(2),
    /** Near leg done. Waiting for the person to walk away and say 「到了」. */
    CAL_WAIT(3),
    /** Sampling the far leg. */
    CAL_FAR(4),
    /** Both legs separated cleanly; the new thresholds are in force. */
    CAL_OK(5),
    /** The two legs overlapped. The old thresholds are untouched. */
    CAL_FAIL(6),
    /** A leg produced too few readings: the Mac could not hear the phone. */
    CAL_SILENT(7);

    val lock: MacLockState
        get() = when (this) {
            LOCKED -> MacLockState.LOCKED
            UNLOCKED -> MacLockState.UNLOCKED
            else -> MacLockState.UNKNOWN
        }

    companion object {
        /** Null for a byte this build does not know. An unknown state is not news. */
        fun of(byte: Int): MacBeaconState? = entries.firstOrNull { it.byte == byte }
    }
}

/** One Mac, as this phone currently believes it to be. */
data class MacSighting(val macId: Int, val state: MacLockState, val beacon: MacBeaconState)

object MacState {

    /**
     * One entry per Mac, keyed by the id inside the authenticated beacon.
     *
     * It used to be a single state, which was correct only while a phone could
     * pair with exactly one Mac. With several, the last beacon to arrive
     * overwrote the others -- so a locked Mac in the next room could silently
     * become the answer to「你的 Mac 锁了吗」about the one in front of you.
     *
     * Each entry ages out on its own clock: walking away from one Mac must not
     * make the phone forget a second one it is still hearing.
     */
    private val heard = java.util.concurrent.ConcurrentHashMap<Int, Pair<MacBeaconState, Long>>()

    /**
     * Recorded only for a tag that verified. An unverified beacon is not news.
     *
     * The clock is a parameter for the same reason [current] takes one: a
     * class that reads the clock itself cannot be tested without a device, and
     * the first version of this was written that way -- its test asserted that
     * a fresh sighting is believed, and failed, because the stubbed
     * elapsedRealtime() in a unit test returns 0 and 0 is how this records
     * "never heard anything".
     */
    fun heard(macId: Int, locked: Boolean, nowUptime: Long = SystemClock.elapsedRealtime()) =
        heard(macId, if (locked) MacBeaconState.LOCKED else MacBeaconState.UNLOCKED, nowUptime)

    fun heard(macId: Int, beacon: MacBeaconState, nowUptime: Long = SystemClock.elapsedRealtime()) {
        // Guard the sentinel: a clock reading of 0 would mean "forget it".
        heard[macId] = beacon to if (nowUptime == 0L) 1L else nowUptime
        SpikeState.notifyListeners()
    }

    /** The latest thing one Mac said, if it said it recently enough. */
    fun beaconOf(macId: Int, nowUptime: Long = SystemClock.elapsedRealtime()): MacBeaconState? =
        heard[macId]?.takeIf { nowUptime - it.second <= SpikeContract.MAC_STATE_STALE_MS }?.first

    fun forget() {
        heard.clear()
        SpikeState.notifyListeners()
    }

    /**
     * Every Mac still within its freshness window, most recently heard first.
     *
     * Stale entries are dropped rather than returned as UNKNOWN: a Mac this
     * phone has not heard from in a minute is not a Mac with an unknown state,
     * it is a Mac that is not here. "Unknown" belongs to [current], which
     * answers about the whole set.
     */
    fun sightings(nowUptime: Long = SystemClock.elapsedRealtime()): List<MacSighting> =
        heard.entries
            .filter { nowUptime - it.value.second <= SpikeContract.MAC_STATE_STALE_MS }
            .sortedByDescending { it.value.second }
            .map { MacSighting(it.key, it.value.first.lock, it.value.first) }

    /**
     * The one-line answer for the home screen, aged out.
     *
     * Uptime, not wall clock: a phone whose clock jumps -- a timezone change, an
     * NTP correction -- would otherwise either freeze this answer or expire it
     * instantly, and the frozen case is the dangerous one. It would leave
     *「Mac 锁着」on screen for a Mac that is no longer there.
     *
     * With several Macs in range this reports UNKNOWN unless they agree. Two
     * Macs in different states have no single true answer, and picking one
     * would be the phone choosing which of two facts to show -- see the
     * three-state note above.
     */
    fun current(nowUptime: Long = SystemClock.elapsedRealtime()): MacLockState {
        val states = sightings(nowUptime).map { it.state }.distinct()
        return if (states.size == 1) states[0] else MacLockState.UNKNOWN
    }
}
