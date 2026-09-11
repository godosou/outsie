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
    private val onStateChanged: () -> Unit,
) {

    companion object {
        private const val TAG = "ReposePair"
    }

    private val handler = Handler(Looper.getMainLooper())
    private var gatt: BluetoothGattServer? = null
    private var advertising = false
    private var session: PairingSession? = null

    /** Non-null only while a window is open. */
    val digits: String? get() = session?.digits
    val stage: PairingSession.Stage? get() = session?.stage
    var lastError: String? = null
        private set

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            advertising = true
            SpikeState.event("配对窗口已打开（${SpikeContract.PAIRING_WINDOW_SECONDS} 秒）")
            onStateChanged()
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
            if (responseNeeded) {
                gatt?.sendResponse(
                    device, requestId,
                    if (ok) BluetoothGatt.GATT_SUCCESS else BluetoothGatt.GATT_FAILURE,
                    offset, null,
                )
            }
            onStateChanged()
        }

        override fun onCharacteristicReadRequest(
            device: BluetoothDevice,
            requestId: Int,
            offset: Int,
            ch: BluetoothGattCharacteristic,
        ) {
            val s = session
            val payload: ByteArray? = when {
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
            onStateChanged()
        }

        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            Log.i(TAG, "pairing conn ${device.address} status=$status newState=$newState")
        }
    }

    private val closeWindow = Runnable {
        if (session?.stage != PairingSession.Stage.Done) {
            lastError = "配对窗口超时，这次的临时密钥已作废"
            SpikeState.event(lastError!!)
        }
        stop()
    }

    /** Open a window. Returns false if the radio refused. */
    fun start(): Boolean {
        stop()
        lastError = null
        val manager = context.getSystemService(BluetoothManager::class.java)
        val adapter = manager?.adapter
        if (adapter == null || !adapter.isEnabled) {
            lastError = "蓝牙没有打开"
            return false
        }
        session = PairingSession()

        val server = manager.openGattServer(context, serverCallback)
        if (server == null) {
            lastError = "打不开 GATT 服务端"
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

        handler.postDelayed(closeWindow, SpikeContract.PAIRING_WINDOW_SECONDS * 1000)
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
            session?.complete()
            SpikeState.event("配对完成，指纹 ${PresenceKey.fingerprint(context)}")
            stop()
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
        onStateChanged()
    }
}
