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

/**
 * One Mac, as this phone currently believes it to be.
 *
 * [keyId] is the slot the beacon verified under. It is the identity that
 * matters to the home screen: a tag that verified under key 166 was minted by
 * whichever Mac holds key 166, and that is the Mac the card for slot 166 is
 * about -- whether or not the phone has ever stored its four hex digits.
 * 0 means the caller did not say (older tests).
 */
data class MacSighting(val macId: Int, val state: MacLockState, val beacon: MacBeaconState, val keyId: Int = 0)

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
    private class Entry(val beacon: MacBeaconState, val at: Long, val keyId: Int)

    private val heard = java.util.concurrent.ConcurrentHashMap<Int, Entry>()

    /**
     * Recorded only for a tag that verified. An unverified beacon is not news.
     *
     * The clock is a parameter for the same reason [sightings] and [beaconOf] take one: a
     * class that reads the clock itself cannot be tested without a device, and
     * the first version of this was written that way -- its test asserted that
     * a fresh sighting is believed, and failed, because the stubbed
     * elapsedRealtime() in a unit test returns 0 and 0 is how this records
     * "never heard anything".
     */
    fun heard(macId: Int, locked: Boolean, nowUptime: Long = SystemClock.elapsedRealtime()) =
        heard(macId, if (locked) MacBeaconState.LOCKED else MacBeaconState.UNLOCKED, nowUptime)

    fun heard(
        macId: Int,
        beacon: MacBeaconState,
        nowUptime: Long = SystemClock.elapsedRealtime(),
        keyId: Int = 0,
    ) {
        // Guard the sentinel: a clock reading of 0 would mean "forget it".
        heard[macId] = Entry(beacon, if (nowUptime == 0L) 1L else nowUptime, keyId)
        SpikeState.notifyListeners()
    }

    /** The latest thing one Mac said, if it said it recently enough. */
    fun beaconOf(macId: Int, nowUptime: Long = SystemClock.elapsedRealtime()): MacBeaconState? =
        heard[macId]?.takeIf { nowUptime - it.at <= SpikeContract.MAC_STATE_STALE_MS }?.beacon

    /**
     * The Mac broadcasting under one key slot, if it was heard recently enough.
     *
     * This is how a card finds its Mac. The bug it replaces: cards matched by
     * the four hex digits in the stored record, and a slot paired by an older
     * build had no record -- so the real Mac verified beacon after beacon and
     * its card stayed grey.
     */
    fun sightingFor(keyId: Int, nowUptime: Long = SystemClock.elapsedRealtime()): MacSighting? =
        sightings(nowUptime).firstOrNull { it.keyId == keyId }

    fun forget() {
        heard.clear()
        SpikeState.notifyListeners()
    }

    /**
     * Every Mac still within its freshness window, most recently heard first.
     *
     * Stale entries are dropped rather than returned as UNKNOWN: a Mac this
     * phone has not heard from in a minute is not a Mac with an unknown state,
     * it is a Mac that is not here. "Unknown" belongs to a Mac that is heard
     * but calibrating -- see [MacBeaconState.lock].
     */
    fun sightings(nowUptime: Long = SystemClock.elapsedRealtime()): List<MacSighting> =
        heard.entries
            .filter { nowUptime - it.value.at <= SpikeContract.MAC_STATE_STALE_MS }
            .sortedByDescending { it.value.at }
            .map { MacSighting(it.key, it.value.beacon.lock, it.value.beacon, it.value.keyId) }
}
