package ai.repose.blespike

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import android.os.SystemClock
import android.util.Log

/**
 * Receives the list of buttons this phone may show, from a Mac it has paired with.
 *
 * WHY THIS IS A SEPARATE, SHORT-LIVED, CONNECTABLE SERVICE
 *
 * The presence beacon is non-connectable on purpose: nothing can reach into this
 * phone through it. Fetching a catalogue needs the opposite, so it is fenced —
 * the window opens only when the user asks for it, closes on its own after
 * [WINDOW_MS], and is a different service UUID from pairing so a scanner looking
 * for one never has to reason about the other.
 *
 * WHY THE CATALOGUE IS SIGNED
 *
 * A forged catalogue does not have to do anything clever: it mislabels the
 * buttons. One reading 「锁屏」 sends a byte that means something else in the
 * Mac's own configuration, and the person presses one thing believing it is
 * another. So the payload carries an HMAC under the catalogue key from pairing —
 * a key that can sign a button list and could never mint a presence beacon.
 *
 * An unsigned or wrongly signed catalogue is DROPPED, and the previous one is
 * kept. Showing unverified buttons "just this once" is the whole attack.
 *
 *   wire  = chunk*                       each: idx(2 BE) ‖ total(2 BE) ‖ bytes
 *   whole = version(1) ‖ revision(4 BE) ‖ jsonUtf8 ‖ tag(16)
 *   tag   = HMAC(K_cat, "repose-console-v1 catalogue" ‖ version ‖ revision ‖ json)[0..16)
 */
@SuppressLint("MissingPermission")
class ConsoleServer(private val context: Context) {

    companion object {
        const val TAG = "ReposeConsole"

        /**
         * How long the window stays open.
         *
         * Long enough for a Mac to notice the request in its beacon (the command
         * rides for 25 seconds), connect and write a few kilobytes; short enough
         * that a phone left on a desk is not quietly connectable all day.
         */
        const val WINDOW_MS = 60_000L
    }

    var lastError: String? = null
        private set

    /** Set once a catalogue has been received and verified. */
    var received: ConsoleCatalogue? = null
        private set

    /**
     * Whether a sync is in flight right now.
     *
     * An observable fact -- the receiving window is literally open -- so a
     * screen can say 「正在同步」 without inventing anything. Without it the
     * only place that knew was a line of text the next state event overwrote,
     * which made pressing 「同步一下」 look like pressing nothing at all.
     */
    val syncing: Boolean
        get() = awaiting

    /** True from asking until a catalogue lands or the window closes. */
    private var awaiting = false

    private var gatt: BluetoothGattServer? = null
    private var openUntil = 0L
    private val handler = Handler(Looper.getMainLooper())
    private val chunks = HashMap<Int, ByteArray>()
    private var expected = 0

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartFailure(errorCode: Int) {
            lastError = "打不开取列表的通道（错误码 $errorCode）"
            Log.e(TAG, "advertise failed $errorCode")
        }
    }

    private val serverCallback = object : BluetoothGattServerCallback() {
        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            characteristic: BluetoothGattCharacteristic,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray,
        ) {
            val ok = characteristic.uuid == SpikeContract.CONSOLE_CHAR_CATALOGUE && accept(value)
            if (responseNeeded) {
                gatt?.sendResponse(
                    device,
                    requestId,
                    if (ok) android.bluetooth.BluetoothGatt.GATT_SUCCESS
                    else android.bluetooth.BluetoothGatt.GATT_FAILURE,
                    offset,
                    null,
                )
            }
        }
    }

    /** One chunk. True if it was well-formed; the catalogue lands once all arrive. */
    private fun accept(chunk: ByteArray): Boolean {
        if (chunk.size < 4) return false
        val idx = ((chunk[0].toInt() and 0xFF) shl 8) or (chunk[1].toInt() and 0xFF)
        val total = ((chunk[2].toInt() and 0xFF) shl 8) or (chunk[3].toInt() and 0xFF)
        if (total == 0 || total > 512 || idx >= total) return false
        // A changed total means a different transfer. Keeping the old parts
        // would splice two catalogues into one nobody sent.
        if (total != expected) {
            chunks.clear()
            expected = total
        }
        chunks[idx] = chunk.copyOfRange(4, chunk.size)
        touch()
        if (chunks.size < total) return true

        val whole = ByteArray(chunks.values.sumOf { it.size })
        var at = 0
        for (i in 0 until total) {
            val part = chunks[i] ?: return false
            part.copyInto(whole, at)
            at += part.size
        }
        chunks.clear()
        expected = 0

        val parsed = ConsoleCatalogue.verify(context, whole)
        if (parsed == null) {
            // Dropped, and the previous catalogue is kept. This is the one place
            // where "show it anyway" would undo the entire point of signing it.
            lastError = "收到的东西签名对不上，已经丢掉了——它不是你配对的那台 Mac 发的。"
            SpikeState.event(lastError!!)
            Log.w(TAG, "catalogue failed verification")
            return false
        }
        received = parsed
        lastError = null
        awaiting = false
        // Kept, so the buttons are on screen before the Mac is in range. The
        // signature was checked when it arrived; storing it does not re-open
        // that question, and re-checking on every read would mean keeping the
        // key warm for no gain.
        ConsoleCatalogue.save(
            context,
            parsed.revision,
            String(whole, 5, whole.size - 5 - ConsoleCatalogue.TAG_LEN, Charsets.UTF_8),
            parsed.keyId,
        )
        // The display preferences are a separate store and survive this — a
        // sync that wiped someone's ordering would look like the ordering never
        // saved. What it does drop is records for actions the Mac no longer
        // has: a freed cmd byte gets reused eventually, and a leftover
        // 「hidden」 would make a brand-new action invisible the day it arrives.
        ConsoleArrangement.save(
            context,
            ConsoleArrangement.prune(parsed, ConsoleArrangement.load(context)),
        )
        SpikeState.event("收到 ${parsed.apps.sumOf { it.actions.size }} 个操作")
        SpikeState.notifyListeners()
        stop()
        return true
    }

    /** Open the window and ask the Mac for its list. */
    fun request(): Boolean {
        stop()
        lastError = null
        for (p in listOf(
            android.Manifest.permission.BLUETOOTH_CONNECT,
            android.Manifest.permission.BLUETOOTH_ADVERTISE,
        )) {
            if (context.checkSelfPermission(p) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                lastError = "还没有蓝牙权限。"
                return false
            }
        }
        val manager = context.getSystemService(BluetoothManager::class.java)
        val adapter = manager?.adapter
        if (adapter == null || !adapter.isEnabled) {
            lastError = "蓝牙没有打开"
            return false
        }
        if (!PresenceKey.hasAny(context)) {
            lastError = "还没有配对，Mac 不会理会这个请求。"
            return false
        }

        awaiting = true
        val server = runCatching { manager.openGattServer(context, serverCallback) }
            .getOrElse { e -> lastError = "打不开通道：${e.message}"; null } ?: return false

        val service = BluetoothGattService(
            SpikeContract.CONSOLE_SERVICE_UUID,
            BluetoothGattService.SERVICE_TYPE_PRIMARY,
        )
        service.addCharacteristic(
            BluetoothGattCharacteristic(
                SpikeContract.CONSOLE_CHAR_CATALOGUE,
                BluetoothGattCharacteristic.PROPERTY_WRITE,
                BluetoothGattCharacteristic.PERMISSION_WRITE,
            ),
        )
        server.addService(service)
        gatt = server

        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
            .setConnectable(true)
            .setTimeout(0)
            .build()
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .addServiceUuid(ParcelUuid(SpikeContract.CONSOLE_SERVICE_UUID))
            .build()
        adapter.bluetoothLeAdvertiser
            ?.startAdvertising(settings, data, AdvertiseData.Builder().setIncludeDeviceName(true).build(), advertiseCallback)
            ?: run { lastError = "这台设备不能做外围"; return false }

        // And tell the Mac to look. The request rides the beacon it already
        // trusts, so a Mac that is not paired with this phone ignores it.
        BleSpikeService.postCommand(context, SpikeContract.CMD_REQUEST_CATALOGUE)
        touch()
        return true
    }

    private fun touch() {
        openUntil = SystemClock.elapsedRealtime() + WINDOW_MS
        handler.removeCallbacks(closer)
        handler.postDelayed(closer, WINDOW_MS)
    }

    private val closer = Runnable {
        if (SystemClock.elapsedRealtime() >= openUntil) {
            // 「awaiting」, not 「received == null」: the second sync of a session
            // would otherwise fail in silence, because the FIRST one's
            // catalogue is still sitting there looking like success.
            if (awaiting && lastError == null) {
                lastError = "没同步成。Mac 要在附近，而且电脑上开着 Outsie。"
                SpikeState.event(lastError!!)
                SpikeState.notifyListeners()
            }
            stop()
        }
    }

    /** Close the window. Safe to call when nothing is open. */
    fun stop() {
        awaiting = false
        handler.removeCallbacks(closer)
        chunks.clear()
        expected = 0
        val manager = context.getSystemService(BluetoothManager::class.java)
        runCatching {
            manager?.adapter?.bluetoothLeAdvertiser?.stopAdvertising(advertiseCallback)
        }
        runCatching { gatt?.close() }
        gatt = null
    }
}
