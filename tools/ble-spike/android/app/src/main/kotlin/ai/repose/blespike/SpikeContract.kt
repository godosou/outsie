package ai.repose.blespike

import java.util.UUID

/** Fixed contract shared with the macOS central spike. Do not change either side alone. */
object SpikeContract {
    // --- legacy B-spike (public, unauthenticated) -----------------------------
    //
    // Kept only for the GATT server, which is now a bring-up aid and no longer
    // part of the presence decision. Anything that decides "is my phone here"
    // must use the presence beacon below: these two constants are published in
    // this repository, so they identify nothing.
    val SERVICE_UUID: UUID = UUID.fromString("7265706F-7365-0001-8000-00805F9B34FB")
    val CHARACTERISTIC_UUID: UUID = UUID.fromString("7265706F-7365-0002-8000-00805F9B34FB")
    const val PAYLOAD = "repose-hello"

    // --- repose-presence-v1 ---------------------------------------------------
    //
    // A 16-bit service UUID, because a 128-bit one costs 18 of the 31 advertising
    // bytes and leaves no room for an authenticator. 0xFFF0 is in the member/
    // unassigned space; a collision with an unrelated device costs the Mac one
    // wasted HMAC verification, nothing more, because the tag is what decides.
    val PRESENCE_SERVICE_UUID: UUID = UUID.fromString("0000FFF0-0000-1000-8000-00805F9B34FB")

    const val PRESENCE_VERSION = 0x02
    const val WINDOW_SECONDS = 30L
    const val TAG_LEN = 8

    /**
     * Domain separation. Byte-identical on the Mac verifier -- if these two
     * strings ever differ the tag never verifies, which looks exactly like a
     * phone that is out of range.
     *
     * Bumped v1 -> v2 with the command fields below. Deliberately not left at
     * v1: the payload grew, and a label that still matched would let the two
     * ends agree on some fields and disagree on others. Half-compatible is
     * harder to diagnose than plainly incompatible, so both ends ship together
     * or nothing verifies at all.
     */
    const val PRESENCE_BEACON_LABEL = "repose-presence-v2 beacon"

    // --- phone -> Mac commands ------------------------------------------------
    //
    // Carried INSIDE the beacon, covered by the same HMAC, rather than over a
    // new connection. See docs/plans/2026-09-11-phone-commands.md.
    //
    // The reason this shape was chosen over a connectable command service: the
    // requirement is "only works when the phone is near the Mac", and here that
    // is free. The Mac has to *hear* the advertisement to get the command at
    // all, and the distance it can hear over is the distance. No separate
    // proximity check to write, and therefore none to get wrong. The beacon
    // also stays non-connectable, which was a deliberate property worth keeping.

    const val CMD_NONE = 0
    /** Lock this Mac now. */
    const val CMD_LOCK = 1
    /**
     * RESERVED. Nothing sends this, and the Mac does not act on it.
     *
     * It was built and then withdrawn. The command made the Mac write an unlock
     * permit -- which a near, switched-on phone already causes it to do several
     * times a minute, so the button had no observable effect. The code point is
     * kept rather than recycled so that a future strict mode ("being near is not
     * enough; tap to allow") can take it back with its original meaning intact,
     * instead of some later feature inheriting 2 and making old recordings mean
     * something new.
     */
    const val CMD_ALLOW_UNLOCK_RESERVED = 2

    /**
     * How long a command keeps going out before the beacon returns to CMD_NONE.
     *
     * WHY 25 AND NOT 6
     *
     * 6 was the first guess, and it was a coin flip. The Mac does not hear every
     * advertisement -- macOS's scan cadence for a single advertiser leaves gaps
     * measured at p99 ~7s and max ~9.3s, and a live capture during this feature's
     * bring-up saw three sightings in fourteen seconds. A 6s window therefore
     * often contained zero sightings, so the lock button worked sometimes.
     *
     * That tail is macOS's, not the phone's: address stability, tx power and
     * in-place payload updates were each tried and none of them moved it. See
     * docs/validation/2026-09-10-scan-cadence.md. So the only lever is to keep
     * repeating, and 25s is ~2.7x the worst gap measured.
     *
     * The cost of a longer window is bounded by the sequence number, not by
     * this value: however many times a command is heard, it is obeyed once.
     */
    const val COMMAND_BROADCAST_MS = 25_000L

    /** The only pairing slot the spike uses. Real pairing will allocate these. */
    const val PRESENCE_KEY_ID = 1

    // --- repose-pair-v2 -------------------------------------------------------
    //
    // A separate 16-bit UUID from the presence beacon, and advertised
    // CONNECTABLE, which the beacon deliberately is not. Pairing is a mode the
    // user enters on purpose for a couple of minutes; presence is what runs for
    // the rest of the time. Keeping them on different UUIDs means a scanner
    // looking for one never has to reason about the other.
    val PAIRING_SERVICE_UUID: UUID = UUID.fromString("0000FFF1-0000-1000-8000-00805F9B34FB")

    /** Mac writes PK_M (65 bytes, SEC1 uncompressed). */
    val PAIR_CHAR_PKM: UUID = UUID.fromString("0000FFF2-0000-1000-8000-00805F9B34FB")

    /** Phone returns PK_P(65) ‖ Cp(32). Refused before PK_M has arrived. */
    val PAIR_CHAR_PKP: UUID = UUID.fromString("0000FFF3-0000-1000-8000-00805F9B34FB")

    /** Mac writes Nm (16 bytes). */
    val PAIR_CHAR_NM: UUID = UUID.fromString("0000FFF4-0000-1000-8000-00805F9B34FB")

    /**
     * Phone returns Np (16 bytes) -- the reveal.
     *
     * Refused until Nm has been written. That ordering IS the protocol: a phone
     * that hands over Np before it has seen Nm has given away its nonce for
     * free, and the commitment it made stops being a commitment to anything.
     */
    val PAIR_CHAR_NP: UUID = UUID.fromString("0000FFF5-0000-1000-8000-00805F9B34FB")

    /**
     * Display names, exchanged after the digits match. Phone READs its name out,
     * Mac WRITEs its own in.
     *
     * COSMETIC, AND THE CODE MUST KEEP TREATING IT THAT WAY. Nothing here is
     * covered by the SAS transcript, so a name is whatever the other end chose
     * to type -- it identifies nothing and must never be the thing a person
     * checks. It exists because "已和 Jingmin 的 MacBook 配对" is something a
     * human can hold in their head, and an 8-character hex fingerprint is not.
     *
     * The fingerprint is still computed and still comparable; it just stopped
     * being the headline. Two opaque codes in one flow -- six digits to compare
     * and eight hex to ignore -- made people ask which one mattered, which is
     * the worst possible question to be unsure about in this particular flow.
     */
    val PAIR_CHAR_NAME: UUID = UUID.fromString("0000FFF6-0000-1000-8000-00805F9B34FB")

    // --- the Mac's own beacon (repose-macstate-v1) ----------------------------
    //
    // The other direction. The presence beacon is one-way, so the phone could
    // never say anything true about the Mac -- only what it had just done. This
    // is the Mac broadcasting one byte of its own state under the same paired
    // key, so the phone can say「Mac 锁着，走过去按回车就能进」and mean it.
    //
    // A DIFFERENT label from the presence beacon, and that is load-bearing: a
    // tag minted for "this Mac is unlocked" must never also verify as "the
    // phone is present", or a recording of one becomes a forgery of the other.

    val MAC_STATE_SERVICE_UUID: UUID = UUID.fromString("0000FFF7-0000-1000-8000-00805F9B34FB")
    const val MAC_STATE_LABEL = "repose-macstate-v1 beacon"
    const val MAC_STATE_VERSION = 0x01

    /**
     * How long a heard state stays believable.
     *
     * Well past the measured scan tail (p99 ~7s, max ~9.3s for a single
     * advertiser) so an ordinary gap does not read as "the Mac went away", and
     * well inside the ±1 window the tag itself is valid for. After this the
     * phone says it does not know, which is the honest answer and the one the
     * screen must be able to show.
     */
    const val MAC_STATE_STALE_MS = 20_000L

    /** How long a pairing window stays open. Ephemerals die with it. */
    const val PAIRING_WINDOW_SECONDS = 180L
}
