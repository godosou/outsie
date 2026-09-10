package ai.repose.blespike

/**
 * Computes the `repose-presence-v1` rotating authenticator (§2.2) and hands the raw
 * service-data bytes to the advertiser.
 *
 * ```
 * counter = floor(unix_time_seconds / WINDOW)          // NEVER transmitted
 * msg     = "repose-presence-v1 beacon" ‖ keyId(1) ‖ counter(8, big-endian)
 * tag     = HMAC-SHA256(K, msg)[0 .. TAG_LEN)          // truncation of an HMAC is safe
 * payload = version(1) ‖ keyId(1) ‖ tag(TAG_LEN)
 * ```
 *
 * The HMAC is taken *through* the non-exportable Keystore key ([PresenceKey.hmac]); the raw
 * `K` never appears here. The caller re-invokes [currentPayload] and re-advertises at least
 * once per WINDOW so the tag stays current across counter rollover and RPA rotation (§2.4).
 */
class PresenceBeacon(private val keyId: Int) {

    /** The window counter the Mac will independently recompute from its own clock. */
    fun currentCounter(nowSeconds: Long = System.currentTimeMillis() / 1000L): Long =
        nowSeconds / SpikeContract.WINDOW_SECONDS

    /** The service-data payload for the current window. */
    fun currentPayload(): ByteArray = payloadFor(currentCounter())

    fun payloadFor(counter: Long): ByteArray {
        val tag = PresenceKey.hmac(keyId, beaconMessage(keyId, counter)).copyOf(SpikeContract.TAG_LEN)
        val out = ByteArray(2 + SpikeContract.TAG_LEN)
        out[0] = SpikeContract.PRESENCE_VERSION.toByte()
        out[1] = keyId.toByte()
        tag.copyInto(out, 2)
        return out
    }

    companion object {
        /** The pre-image the tag authenticates. Shared verbatim with the Mac verifier (§3.2). */
        fun beaconMessage(keyId: Int, counter: Long): ByteArray =
            PairingCrypto.ascii(SpikeContract.PRESENCE_BEACON_LABEL) +
                byteArrayOf(keyId.toByte()) +
                PairingCrypto.longToBe(counter)
    }
}
