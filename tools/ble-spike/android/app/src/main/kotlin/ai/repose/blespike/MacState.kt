package ai.repose.blespike

import android.os.SystemClock

/**
 * What this phone believes about the paired Mac, and how it is allowed to say it.
 *
 * The presence beacon is one-way, so until now the phone knew nothing about the
 * Mac at all. Every sentence it showed was either about itself ("已发出") or an
 * invention. `repose-macstate-v1` is the Mac broadcasting its own lock state
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

object MacState {

    @Volatile private var state: MacLockState = MacLockState.UNKNOWN
    @Volatile private var heardAtUptime: Long = 0L

    /** Recorded only for a tag that verified. An unverified beacon is not news. */
    fun heard(locked: Boolean) {
        state = if (locked) MacLockState.LOCKED else MacLockState.UNLOCKED
        heardAtUptime = SystemClock.elapsedRealtime()
        SpikeState.notifyListeners()
    }

    fun forget() {
        state = MacLockState.UNKNOWN
        heardAtUptime = 0L
        SpikeState.notifyListeners()
    }

    /**
     * The current belief, aged out.
     *
     * Uptime, not wall clock: a phone whose clock jumps -- a timezone change, an
     * NTP correction -- would otherwise either freeze this answer or expire it
     * instantly, and the frozen case is the dangerous one. It would leave
     *「Mac 锁着」on screen for a Mac that is no longer there.
     */
    fun current(nowUptime: Long = SystemClock.elapsedRealtime()): MacLockState =
        if (heardAtUptime == 0L || nowUptime - heardAtUptime > SpikeContract.MAC_STATE_STALE_MS) {
            MacLockState.UNKNOWN
        } else {
            state
        }
}
