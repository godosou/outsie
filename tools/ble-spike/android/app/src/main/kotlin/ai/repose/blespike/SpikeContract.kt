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

    const val PRESENCE_VERSION = 0x01
    const val WINDOW_SECONDS = 30L
    const val TAG_LEN = 8

    /**
     * Domain separation. Byte-identical on the Mac verifier -- if these two
     * strings ever differ the tag never verifies, which looks exactly like a
     * phone that is out of range.
     */
    const val PRESENCE_BEACON_LABEL = "repose-presence-v1 beacon"

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

    /** How long a pairing window stays open. Ephemerals die with it. */
    const val PAIRING_WINDOW_SECONDS = 180L
}
