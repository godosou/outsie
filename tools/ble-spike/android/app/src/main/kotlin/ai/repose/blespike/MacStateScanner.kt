package ai.repose.blespike

import android.annotation.SuppressLint
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.content.pm.PackageManager
import android.os.ParcelUuid
import android.util.Log

/**
 * Listens for the Mac's own beacon and verifies it before believing a word.
 *
 * The payload rides in the advertisement's local name, because CoreBluetooth on
 * macOS does not let a peripheral publish Service Data. That is a macOS
 * limitation, not a design choice, and it costs nothing: the bytes are the same
 * bytes and the tag covers them.
 *
 * Encoded base64url, not hex. A legacy advertisement has 31 bytes, and after
 * flags and the service UUID list only 22 characters are left for the name --
 * which v1's 11-byte payload filled exactly in hex. v2's mac id would have
 * pushed it to 26, and macOS simply declines to advertise rather than
 * complaining, so the Mac would look switched off. Hex is still accepted on the
 * way in for a Mac that has not been updated yet.
 *
 *   payload = version(1) ‖ keyId(1) ‖ macId(2) ‖ state(1) ‖ tag(8)
 *   msg     = "repose-macstate-v2 beacon" ‖ keyId(1) ‖ macId(2) ‖ counter(8 BE) ‖ state(1)
 *
 * WHY THE TAG MATTERS HERE TOO
 *
 * It would be tempting to treat this as cosmetic — it only drives a sentence on
 * a screen. But the sentence is「Mac 锁着，走过去按回车就能进」, and anyone with
 * a radio could otherwise broadcast "unlocked" to keep a person from walking
 * over, or "locked" to send them to a Mac that is fine. Neither is catastrophic
 * and both are the app lying on someone else's instructions, which is the thing
 * this project refuses to ship.
 */
@SuppressLint("MissingPermission")
class MacStateScanner(private val context: Context) {

    private companion object {
        const val TAG = "ReposeMacState"
        const val PAYLOAD_LEN = 5 + SpikeContract.TAG_LEN
    }

    private var scanning = false

    private val callback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            val name = result.scanRecord?.deviceName ?: return
            val payload = payloadOrNull(name) ?: return
            if (payload.size != PAYLOAD_LEN) return
            if (payload[0].toInt() and 0xFF != SpikeContract.MAC_STATE_VERSION) return

            val keyId = payload[1].toInt() and 0xFF
            // Any slot this phone holds: a Mac paired into slot 37 broadcasts
            // its state under 37, and demanding slot 1 would make that Mac
            // read as permanently unknown.
            if (!PresenceKey.activeIds(context).contains(keyId)) return
            val macId = ((payload[2].toInt() and 0xFF) shl 8) or (payload[3].toInt() and 0xFF)
            // One byte: lock state, or a calibration phase (protocol §14). A
            // value this build does not know is dropped, not guessed at.
            val beacon = MacBeaconState.of(payload[4].toInt() and 0xFF) ?: return
            val tag = payload.copyOfRange(5, PAYLOAD_LEN)

            if (verify(keyId, macId, beacon.byte, tag)) {
                MacState.heard(macId, beacon)
                // 「量成功过」 is learned here, from the Mac's signed verdict --
                // not on the calibration screen, which may have been rebuilt or
                // closed while you stood there not looking at it.
                if (beacon == MacBeaconState.CAL_OK) AppStore(context).markCalibrated(keyId)
                // Only from a beacon that verified. An id taken from an
                // unverified one would let anyone with a radio put a label on
                // this phone's list of Macs.
                AppStore(context).noteMacId(keyId, "%04X".format(macId))
            }
        }

        override fun onScanFailed(errorCode: Int) {
            Log.w(TAG, "scan failed: $errorCode")
            scanning = false
        }
    }

    /**
     * True if the tag was minted by the paired key for this state, in a window
     * near enough to now.
     *
     * ±1 window, same tolerance the Mac gives the phone: two clocks that agree
     * to within thirty seconds is all either side assumes.
     */
    private fun verify(keyId: Int, macId: Int, state: Int, tag: ByteArray): Boolean {
        if (!PresenceKey.has(keyId)) return false
        val c0 = System.currentTimeMillis() / 1000L / SpikeContract.WINDOW_SECONDS
        for (c in longArrayOf(c0 - 1, c0, c0 + 1)) {
            val msg = SpikeContract.MAC_STATE_LABEL.toByteArray(Charsets.US_ASCII) +
                byteArrayOf(keyId.toByte(), ((macId shr 8) and 0xFF).toByte(), (macId and 0xFF).toByte()) +
                PresenceBeacon.beLong(c) +
                byteArrayOf(state.toByte())
            val full = runCatching { PresenceKey.hmac(keyId, msg) }.getOrNull() ?: return false
            if (constantTimeEquals(full.copyOf(SpikeContract.TAG_LEN), tag)) return true
        }
        return false
    }

    fun start() {
        if (scanning) return
        // BLUETOOTH_SCAN is separate from CONNECT and ADVERTISE. Without it this
        // throws, and a crash here would take down the beacon the whole feature
        // rests on -- for the sake of a sentence on a screen.
        if (context.checkSelfPermission(android.Manifest.permission.BLUETOOTH_SCAN)
            != PackageManager.PERMISSION_GRANTED
        ) {
            Log.i(TAG, "no BLUETOOTH_SCAN permission; the Mac's state stays unknown")
            return
        }
        val scanner = context.getSystemService(BluetoothManager::class.java)
            ?.adapter?.bluetoothLeScanner ?: return
        val filter = ScanFilter.Builder()
            .setServiceUuid(ParcelUuid(SpikeContract.MAC_STATE_SERVICE_UUID))
            .build()
        // LOW_POWER, not LOW_LATENCY. This drives a sentence, not a decision:
        // being a few seconds late to say「Mac 锁着」costs nothing, and this
        // runs for the whole day next to a beacon that is already advertising.
        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_LOW_POWER)
            .build()
        runCatching { scanner.startScan(listOf(filter), settings, callback) }
            .onSuccess { scanning = true; Log.i(TAG, "listening for the Mac's state") }
            .onFailure { Log.w(TAG, "could not start scanning", it) }
    }

    fun stop() {
        if (!scanning) return
        runCatching {
            context.getSystemService(BluetoothManager::class.java)
                ?.adapter?.bluetoothLeScanner?.stopScan(callback)
        }
        scanning = false
        // A belief with nothing refreshing it is a belief that will go stale on
        // screen. Drop it now rather than let it age out looking current.
        MacState.forget()
    }

    /**
     * The local name as bytes: base64url first, hex as a fallback.
     *
     * Both, because a Mac still running the older build advertises hex, and a
     * phone that refused it would report that Mac as absent rather than as out
     * of date. The tag is checked either way, so accepting two spellings costs
     * nothing: an attacker who could forge the bytes does not need help with
     * the encoding.
     */
    private fun payloadOrNull(s: String): ByteArray? =
        base64UrlOrNull(s) ?: hexOrNull(s)

    private fun base64UrlOrNull(s: String): ByteArray? = runCatching {
        android.util.Base64.decode(
            s,
            android.util.Base64.URL_SAFE or android.util.Base64.NO_PADDING or android.util.Base64.NO_WRAP,
        )
    }.getOrNull()?.takeIf { it.size == PAYLOAD_LEN }

    private fun hexOrNull(s: String): ByteArray? {
        if (s.length % 2 != 0 || s.isEmpty()) return null
        return runCatching {
            ByteArray(s.length / 2) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
        }.getOrNull()
    }

    private fun constantTimeEquals(a: ByteArray, b: ByteArray): Boolean {
        if (a.size != b.size) return false
        var diff = 0
        for (i in a.indices) diff = diff or (a[i].toInt() xor b[i].toInt())
        return diff == 0
    }
}