package ai.repose.blespike

import java.security.SecureRandom

/**
 * The `repose-presence-v1` rotating authenticator (design §2.2).
 *
 * ```
 * counter = floor(unix_seconds / WINDOW_SECONDS)        // NEVER transmitted
 * msg     = "repose-presence-v1 beacon" ‖ keyId(1) ‖ counter(8, big-endian)
 * tag     = HMAC-SHA256(K, msg)[0 .. TAG_LEN)           // truncating an HMAC is safe
 * payload = version(1) ‖ keyId(1) ‖ tag(TAG_LEN)        // 10 bytes of service data
 * ```
 *
 * `counter` costs zero advertising bytes and is not spoofable, because the Mac derives
 * it from its own clock rather than reading it off the air.
 *
 * WHY AN UNPROVISIONED PHONE STILL ADVERTISES
 * -------------------------------------------
 * With no `K` this mints the tag from a random key it invented at startup, so the packet
 * is well-formed and cryptographically worthless. Staying silent instead would be worse
 * in two ways: the Mac could not tell "no key" from "out of range", and the
 * impersonation test could not distinguish an imposter that was rejected from one that
 * never got on the air -- a test that cannot fail for the right reason cannot pass for
 * it either. [authentic] says which case this is, and the UI shows it.
 */
class PresenceBeacon(private val keyId: Int) {

    private val decoyKey: ByteArray by lazy { ByteArray(32).also { SecureRandom().nextBytes(it) } }

    /** True when the tag is minted from a provisioned `K` rather than the decoy. */
    val authentic: Boolean get() = PresenceKey.has(keyId)

    fun currentCounter(nowSeconds: Long = System.currentTimeMillis() / 1000L): Long =
        nowSeconds / SpikeContract.WINDOW_SECONDS

    fun currentPayload(): ByteArray = payloadFor(currentCounter())

    fun payloadFor(counter: Long): ByteArray {
        val msg = beaconMessage(keyId, counter)
        val full = if (authentic) PresenceKey.hmac(keyId, msg) else hmacRaw(decoyKey, msg)
        val out = ByteArray(2 + SpikeContract.TAG_LEN)
        out[0] = SpikeContract.PRESENCE_VERSION.toByte()
        out[1] = keyId.toByte()
        full.copyInto(out, 2, 0, SpikeContract.TAG_LEN)
        return out
    }

    companion object {
        /** The pre-image the tag authenticates. Byte-identical on the Mac verifier (§3.2). */
        fun beaconMessage(keyId: Int, counter: Long): ByteArray =
            SpikeContract.PRESENCE_BEACON_LABEL.toByteArray(Charsets.US_ASCII) +
                byteArrayOf(keyId.toByte()) +
                beLong(counter)

        fun beLong(v: Long) = ByteArray(8) { ((v ushr (56 - 8 * it)) and 0xFF).toByte() }

        private fun hmacRaw(key: ByteArray, msg: ByteArray): ByteArray =
            javax.crypto.Mac.getInstance("HmacSHA256").run {
                init(javax.crypto.spec.SecretKeySpec(key, "HmacSHA256"))
                doFinal(msg)
            }
    }
}
