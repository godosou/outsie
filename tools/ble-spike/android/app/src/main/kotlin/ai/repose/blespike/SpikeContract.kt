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
}
