package ai.repose.blespike

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
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
import android.util.Log

/**
 * The radio half of `repose-pair-v2` on the phone: a GATT server, open only
 * while the user is deliberately pairing.
 *
 * WHY THIS IS A MODE AND NOT A FEATURE THAT IS ALWAYS ON
 *
 * The presence beacon is non-connectable on purpose -- nothing can dial into it.
 * Pairing needs the opposite, so entering pairing STOPS the beacon and
 * advertises a connectable set instead, and leaving pairing puts the beacon
 * back. Presence is not needed during the two minutes someone is pairing, and
 * leaving a connectable surface up for the rest of the time would hand every
 * passer-by something to talk to.
 *
 * The window is short and self-closing. Ephemerals, nonce and SAS all die with
 * it: a session that is abandoned halfway cannot be finished later, which is
 * what stops a captured half-exchange from being useful.
 */
@SuppressLint("MissingPermission")
class PairingServer(
    private val context: Context,
    onStateChanged: () -> Unit,
) {

    /**
     * Every notification goes to the main thread.
     *
     * GATT callbacks arrive on a binder thread. Calling back straight from there
     * meant the screen rebuilt its views off the UI thread -- which does not
     * throw and does not log; it just produces a view tree that never draws. The
     * first live pairing run showed a blank phone while the Mac sat waiting at
     * the six digits, which is the worst moment to have nothing on screen.
     */
    private val notify: () -> Unit = {
        Handler(Looper.getMainLooper()).post(onStateChanged)
    }

    companion object {
        private const val TAG = "ReposePair"
    }

    private val handler = Handler(Looper.getMainLooper())
    private var gatt: BluetoothGattServer? = null
    private var advertising = false
    private var session: PairingSession? = null

    /** Whatever the Mac called itself, if it said. Cosmetic — see the contract. */
    var macName: String? = null
        private set

    /** Non-null only while a window is open. */
    val digits: String? get() = session?.digits
    val stage: PairingSession.Stage? get() = session?.stage
    var lastError: String? = null
        private set

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            advertising = true
            SpikeState.event("配对窗口已打开（${SpikeContract.PAIRING_WINDOW_SECONDS} 秒）")
            notify()
        }
        override fun onStartFailure(errorCode: Int) {
            lastError = "配对广播失败 code=$errorCode"
            SpikeState.event(lastError!!)
            stop()
        }
    }

    private val serverCallback = object : BluetoothGattServerCallback() {
        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            ch: BluetoothGattCharacteristic,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray,
        ) {
            val s = session
            val ok = when {
                ch.uuid == SpikeContract.PAIR_CHAR_NAME -> {
                    // Cosmetic, and accepted at any stage -- it is not part of
                    // the protocol and must not be able to disturb it. Bounded,
                    // because this arrives from a stranger over a radio.
                    macName = value.decodeToString().take(60).trim().ifEmpty { null }
                    true
                }
                s == null -> false
                ch.uuid == SpikeContract.PAIR_CHAR_PKM -> s.receiveMacKey(value)
                ch.uuid == SpikeContract.PAIR_CHAR_NM -> s.receiveMacNonce(value)
                else -> false
            }
            if (!ok) {
                // GATT_FAILURE rather than a silent empty success: a Mac whose
                // messages arrive out of order has to be told, or it will go on
                // to compare digits that were never computed.
                lastError = "配对消息被拒绝（顺序或内容不对）"
                SpikeState.event(lastError!!)
            }
            touchWindow()
            if (responseNeeded) {
                gatt?.sendResponse(
                    device, requestId,
                    if (ok) BluetoothGatt.GATT_SUCCESS else BluetoothGatt.GATT_FAILURE,
                    offset, null,
                )
            }
            notify()
        }

        override fun onCharacteristicReadRequest(
            device: BluetoothDevice,
            requestId: Int,
            offset: Int,
            ch: BluetoothGattCharacteristic,
        ) {
            val s = session
            val payload: ByteArray? = when {
                ch.uuid == SpikeContract.PAIR_CHAR_NAME -> phoneDisplayName().encodeToByteArray()
                s == null -> null
                ch.uuid == SpikeContract.PAIR_CHAR_PKP -> s.keyAndCommitment()
                ch.uuid == SpikeContract.PAIR_CHAR_NP -> s.revealNonce()
                else -> null
            }
            if (payload == null) {
                gatt?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
                return
            }
            // Long reads arrive as repeated requests with a growing offset; the
            // 97-byte P1 payload does not fit one default-MTU packet.
            if (offset > payload.size) {
                gatt?.sendResponse(device, requestId, BluetoothGatt.GATT_INVALID_OFFSET, offset, null)
                return
            }
            gatt?.sendResponse(
                device, requestId, BluetoothGatt.GATT_SUCCESS, offset,
                payload.copyOfRange(offset, payload.size),
            )
            touchWindow()
            notify()
        }

        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            Log.i(TAG, "pairing conn ${device.address} status=$status newState=$newState")
        }
    }

    /**
     * Push the deadline back, because something happened.
     *
     * The window used to be a fixed three minutes from the moment somebody
     * tapped 开始配对 -- and then they had to walk to the Mac, click 配对手机,
     * wait for it to find the phone, compare six digits, click there, and type
     * an administrator password. Three minutes is easy to exceed doing that.
     * The phone's window closed mid-exchange and dropped its session, so the
     * Mac derived a key and wrote it while the phone reported 配对没有完成.
     * One side paired, the other not, and nothing on either screen explained it.
     *
     * The window is an IDLE timeout now: it means "nothing has happened for
     * three minutes", which is what it was always meant to mean. Every protocol
     * step pushes it back, and so does the digits appearing -- the human is the
     * slowest step and must not be the one that runs out of time.
     */
    private fun touchWindow() {
        handler.removeCallbacks(closeWindow)
        handler.postDelayed(closeWindow, SpikeContract.PAIRING_WINDOW_SECONDS * 1000)
    }

    private val closeWindow = Runnable {
        if (session?.stage != PairingSession.Stage.Done) {
            // Says which kind of nothing happened. "Timed out" on a screen
            // showing six digits reads as a bug; the user was mid-decision and
            // deserves to know the clock was on them.
            lastError = if (session?.digits != null) {
                "太久没有确认，这次配对作废了。两边重新开始一次。"
            } else {
                "三分钟没等到 Mac，配对窗口已关闭。重新开始即可。"
            }
            SpikeState.event(lastError!!)
        }
        stop()
    }

    /** Open a window. Returns false if it could not start, with lastError saying why. */
    fun start(): Boolean {
        stop()
        lastError = null

        // Ask before touching the radio.
        //
        // A fresh install has no Bluetooth permission, and openGattServer throws
        // SecurityException rather than returning null -- so tapping 开始配对 on a
        // newly installed app killed the process outright. From the user's side
        // the app simply vanished, with nothing to read and nothing to fix.
        for (p in listOf(
            android.Manifest.permission.BLUETOOTH_CONNECT,
            android.Manifest.permission.BLUETOOTH_ADVERTISE,
        )) {
            if (context.checkSelfPermission(p) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                lastError = "还没有蓝牙权限。请在弹出的窗口里允许，然后再试一次。"
                return false
            }
        }

        val manager = context.getSystemService(BluetoothManager::class.java)
        val adapter = manager?.adapter
        if (adapter == null || !adapter.isEnabled) {
            lastError = "蓝牙没有打开"
            return false
        }
        session = PairingSession()

        // Even with permission granted, the stack can refuse. Nothing the radio
        // does should be able to close the app while someone is halfway through
        // pairing.
        val server = runCatching { manager.openGattServer(context, serverCallback) }
            .getOrElse { e ->
                lastError = "打不开配对通道：${e.message}"
                Log.e(TAG, "openGattServer failed", e)
                null
            }
        if (server == null) {
            if (lastError == null) lastError = "打不开配对通道"
            return false
        }
        val service = BluetoothGattService(
            SpikeContract.PAIRING_SERVICE_UUID,
            BluetoothGattService.SERVICE_TYPE_PRIMARY,
        )
        service.addCharacteristic(
            BluetoothGattCharacteristic(
                SpikeContract.PAIR_CHAR_PKM,
                BluetoothGattCharacteristic.PROPERTY_WRITE,
                BluetoothGattCharacteristic.PERMISSION_WRITE,
            ),
        )
        service.addCharacteristic(
            BluetoothGattCharacteristic(
                SpikeContract.PAIR_CHAR_PKP,
                BluetoothGattCharacteristic.PROPERTY_READ,
                BluetoothGattCharacteristic.PERMISSION_READ,
            ),
        )
        service.addCharacteristic(
            BluetoothGattCharacteristic(
                SpikeContract.PAIR_CHAR_NM,
                BluetoothGattCharacteristic.PROPERTY_WRITE,
                BluetoothGattCharacteristic.PERMISSION_WRITE,
            ),
        )
        service.addCharacteristic(
            BluetoothGattCharacteristic(
                SpikeContract.PAIR_CHAR_NP,
                BluetoothGattCharacteristic.PROPERTY_READ,
                BluetoothGattCharacteristic.PERMISSION_READ,
            ),
        )
        service.addCharacteristic(
            BluetoothGattCharacteristic(
                SpikeContract.PAIR_CHAR_NAME,
                BluetoothGattCharacteristic.PROPERTY_READ or
                    BluetoothGattCharacteristic.PROPERTY_WRITE,
                BluetoothGattCharacteristic.PERMISSION_READ or
                    BluetoothGattCharacteristic.PERMISSION_WRITE,
            ),
        )
        server.addService(service)
        gatt = server

        // Connectable, unlike the beacon. Name included so a human choosing
        // between two phones on the Mac's side has something to choose by.
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
            .setConnectable(true)
            .setTimeout(0)
            .build()
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .addServiceUuid(ParcelUuid(SpikeContract.PAIRING_SERVICE_UUID))
            .build()
        val scanResponse = AdvertiseData.Builder().setIncludeDeviceName(true).build()
        adapter.bluetoothLeAdvertiser?.startAdvertising(settings, data, scanResponse, advertiseCallback)
            ?: run { lastError = "这台设备不能做外围"; return false }

        touchWindow()
        return true
    }

    /**
     * Confirm the digits match and keep the key.
     *
     * This is the only path that writes K. Everything before it is arithmetic
     * that can be thrown away, which is what makes "不一致" a real answer rather
     * than a polite one.
     */
    fun confirmMatch(): Boolean {
        val k = session?.deriveKey() ?: return false
        return runCatching {
            PresenceKey.importKey(context, SpikeContract.PRESENCE_KEY_ID, k)
            k.fill(0)
            // Only kept once the digits matched. A name captured from a session
            // the human rejected would be the attacker's name, sitting on the
            // screen next to the words 已配对.
            AppStore(context).pairedMac = macName
            session?.complete()
            SpikeState.event("配对完成，指纹 ${PresenceKey.fingerprint(context)}")
            stop()
            // Pairing takes the beacon down to advertise connectably; finishing
            // has to put it back. Otherwise a successful pairing hands you a
            // phone that is paired and silent, with the home screen reporting
            // 已关闭 -- which reads as the pairing having failed.
            runCatching {
                context.startForegroundService(
                    android.content.Intent(context, BleSpikeService::class.java),
                )
            }
            true
        }.getOrElse {
            lastError = "写入密钥失败：${it.message}"
            false
        }
    }

    /** The human said the digits differ. Throw everything away. */
    fun reject() {
        session?.fail()
        lastError = "两边数字不一致，已中止。不要重试同一次会话。"
        SpikeState.event(lastError!!)
        stop()
    }

    /**
     * What to call this phone on the Mac's screen.
     *
     * The user's own Bluetooth name first — that is the one they recognise —
     * falling back to the model, which is at least true.
     */
    private fun phoneDisplayName(): String {
        val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter
        val bt = runCatching { adapter?.name }.getOrNull()?.trim()
        if (!bt.isNullOrEmpty()) return bt.take(60)
        return "${android.os.Build.MANUFACTURER} ${android.os.Build.MODEL}".trim().take(60)
    }

    fun stop() {
        handler.removeCallbacks(closeWindow)
        val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter
        if (advertising) {
            runCatching { adapter?.bluetoothLeAdvertiser?.stopAdvertising(advertiseCallback) }
            advertising = false
        }
        runCatching { gatt?.close() }
        gatt = null
        if (session?.stage != PairingSession.Stage.Done) session = null
        notify()
    }
}
