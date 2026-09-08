package ai.repose.mobile.unlock.console

import android.Manifest
import android.annotation.SuppressLint
import android.bluetooth.*
import android.companion.CompanionDeviceManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import ai.repose.mobile.unlock.ConsoleBleChannel
import ai.repose.mobile.unlock.companion.canonicalBluetoothAddress
import ai.repose.mobile.unlock.companion.legacyAssociationToken
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.util.UUID

/** One foreground GATT session and one RPC; all state transitions run on main. */
@SuppressLint("MissingPermission")
internal class AndroidConsoleBleChannel(private val context: Context) : ConsoleBleChannel {
    private val handler = Handler(Looper.getMainLooper())
    private var connection: Connection? = null
    private val revoked: (String) -> Unit = { id -> handler.post { if (connection?.credential?.id == id) disconnect("revoked") } }
    init { DebugConsoleCredentials.addListener(revoked) }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "devices" -> result.success(DebugConsoleCredentials.devices())
            "disconnect" -> { disconnect("disconnected"); result.success(null) }
            "connect" -> {
                disconnect("replaced")
                val credential = call.argument<String>("deviceId")?.let(DebugConsoleCredentials::get)
                if (credential == null) { result.error("notPaired", "Pair this Mac with Phone Key first.", null); return }
                try {
                    val device = resolveDevice(credential.associationId)
                    val next = Connection(credential, result)
                    connection = next
                    next.open(device)
                } catch (_: Exception) {
                    credential.wipe()
                    if (connection != null) disconnect("bluetoothUnavailable")
                    else result.error("bluetoothUnavailable", "Bluetooth or the associated Mac is unavailable.", null)
                }
            }
            "request" -> {
                val message = call.argument<String>("message")
                val current = connection
                if (current == null || message == null) result.error("disconnected", "Connect to a paired Mac first.", null)
                else current.request(message, result)
            }
            else -> result.notImplemented()
        }
    }
    override fun close() {
        DebugConsoleCredentials.removeListener(revoked)
        disconnect("disconnected")
        DebugConsoleCredentials.clear()
    }
    private fun disconnect(reason: String) {
        val old = connection
        connection = null
        old?.close(reason)
    }
    private fun resolveDevice(token: Int): BluetoothDevice {
        check(context.checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED)
        val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter
        check(adapter != null && adapter.isEnabled)
        val manager = context.getSystemService(CompanionDeviceManager::class.java)
        check(manager != null)
        if (token >= 0) {
            check(Build.VERSION.SDK_INT >= 33)
            val association = manager.myAssociations.singleOrNull { it.id == token }
            check(association != null)
            if (Build.VERSION.SDK_INT >= 34) association.associatedDevice?.bleDevice?.device?.let { return it }
            association.deviceMacAddress?.toString()?.let { return adapter.getRemoteDevice(it) }
            if (Build.VERSION.SDK_INT >= 34) association.associatedDevice?.bluetoothDevice?.let { return it }
            error("association unavailable")
        }
        @Suppress("DEPRECATION")
        val address = manager.associations.mapNotNull(::canonicalBluetoothAddress).singleOrNull { legacyAssociationToken(it) == token }
        check(address != null)
        return adapter.getRemoteDevice(address)
    }

    private inner class Connection(val credential: ConsoleCredential, private var connectResult: MethodChannel.Result?) {
        private var gatt: BluetoothGatt? = null
        private var stage = Stage.CONNECTING
        private var mtu = 23
        private var rx: BluetoothGattCharacteristic? = null
        private var challengeCharacteristic: BluetoothGattCharacteristic? = null
        private var cipher: ConsoleBleCipher? = null
        private var requestResult: MethodChannel.Result? = null
        private var messageId = 0
        private var outgoing = emptyList<ByteArray>()
        private var writeIndex = 0
        private var writing = false
        private var response: String? = null
        private val receiver = ConsoleBleFragments.Receiver()
        private val timeout = Runnable { fail("timeout") }
        private fun armTimeout() { handler.removeCallbacks(timeout); handler.postDelayed(timeout, 30_000L) }
        private fun current(callbackGatt: BluetoothGatt): Boolean = connection === this && stage != Stage.CLOSED && gatt === callbackGatt
        fun open(device: BluetoothDevice) {
            armTimeout()
            gatt = device.connectGatt(context, false, callbacks, BluetoothDevice.TRANSPORT_LE, BluetoothDevice.PHY_LE_1M_MASK, handler)
            if (gatt == null) fail("bluetoothUnavailable")
        }
        fun request(message: String, result: MethodChannel.Result) {
            if (stage != Stage.READY) { result.error("disconnected", "Bluetooth connection is not ready.", null); return }
            if (requestResult != null) { result.error("busy", "A Bluetooth request is already pending.", null); return }
            val bytes = message.toByteArray(Charsets.UTF_8)
            if (bytes.isEmpty() || bytes.size > ConsoleBleCipher.MAX_PACKET - 58) { result.error("invalidRequest", "Request is too large.", null); return }
            requestResult = result
            try {
                check(messageId < Int.MAX_VALUE)
                messageId++
                outgoing = ConsoleBleFragments.encode(messageId, checkNotNull(cipher).encrypt(bytes), minOf(mtu - 3, 512))
                writeIndex = 0
                response = null
                receiver.reset()
                armTimeout()
                writeNext()
            } catch (_: Exception) { fail("protocolError") }
        }
        @Suppress("DEPRECATION")
        private fun writeNext() {
            if (writeIndex >= outgoing.size) { deliverIfComplete(); return }
            val link = gatt ?: return fail("disconnected")
            val characteristic = rx ?: return fail("protocolError")
            writing = true
            val ok = if (Build.VERSION.SDK_INT >= 33) {
                link.writeCharacteristic(characteristic, outgoing[writeIndex], BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT) == BluetoothStatusCodes.SUCCESS
            } else {
                characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                characteristic.value = outgoing[writeIndex]
                link.writeCharacteristic(characteristic)
            }
            if (!ok) fail("writeFailed")
        }
        private fun deliverIfComplete() {
            val value = response ?: return
            if (writing || writeIndex < outgoing.size) return
            handler.removeCallbacks(timeout)
            val callback = requestResult
            requestResult = null
            outgoing = emptyList()
            response = null
            callback?.success(value)
        }
        fun close(reason: String) {
            if (stage == Stage.CLOSED) return
            stage = Stage.CLOSED
            handler.removeCallbacks(timeout)
            val link = gatt; gatt = null
            try { link?.disconnect() } catch (_: Exception) { }
            try { link?.close() } catch (_: Exception) { }
            cipher?.close(); cipher = null; credential.wipe()
            receiver.reset(); outgoing = emptyList(); response = null
            val connect = connectResult; connectResult = null
            val request = requestResult; requestResult = null
            connect?.error(reason, "Bluetooth control connection ended. Reconnect to the paired Mac.", null)
            request?.error(reason, "Bluetooth request was interrupted and was not retried.", null)
        }
        private fun fail(reason: String) { if (connection === this) disconnect(reason) else close(reason) }

        private val callbacks = object : BluetoothGattCallback() {
            override fun onConnectionStateChange(link: BluetoothGatt, status: Int, newState: Int) {
                if (!current(link)) return
                if (status != BluetoothGatt.GATT_SUCCESS || newState == BluetoothProfile.STATE_DISCONNECTED) return fail("disconnected")
                if (newState == BluetoothProfile.STATE_CONNECTED && stage == Stage.CONNECTING) {
                    stage = Stage.MTU
                    if (!link.requestMtu(517)) fail("mtuFailed")
                }
            }
            override fun onMtuChanged(link: BluetoothGatt, value: Int, status: Int) {
                if (!current(link) || stage != Stage.MTU) return
                if (status != BluetoothGatt.GATT_SUCCESS || value < 23) return fail("mtuFailed")
                mtu = value
                stage = Stage.DISCOVERING
                if (!link.discoverServices()) fail("discoveryFailed")
            }
            @Suppress("DEPRECATION")
            override fun onServicesDiscovered(link: BluetoothGatt, status: Int) {
                if (!current(link) || stage != Stage.DISCOVERING) return
                if (status != BluetoothGatt.GATT_SUCCESS) return fail("discoveryFailed")
                val service = link.getService(SERVICE)
                rx = service?.getCharacteristic(RX)
                val tx = service?.getCharacteristic(TX)
                challengeCharacteristic = service?.getCharacteristic(CHALLENGE)
                val descriptor = tx?.getDescriptor(CCCD)
                if (rx == null || tx == null || descriptor == null || challengeCharacteristic == null ||
                    rx!!.properties and BluetoothGattCharacteristic.PROPERTY_WRITE == 0 ||
                    tx.properties and BluetoothGattCharacteristic.PROPERTY_NOTIFY == 0 ||
                    challengeCharacteristic!!.properties and BluetoothGattCharacteristic.PROPERTY_READ == 0) return fail("unsupported")
                if (!link.setCharacteristicNotification(tx, true)) return fail("subscribeFailed")
                stage = Stage.SUBSCRIBING
                val ok = if (Build.VERSION.SDK_INT >= 33) link.writeDescriptor(descriptor, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE) == BluetoothStatusCodes.SUCCESS
                else { descriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE; link.writeDescriptor(descriptor) }
                if (!ok) fail("subscribeFailed")
            }
            override fun onDescriptorWrite(link: BluetoothGatt, descriptor: BluetoothGattDescriptor, status: Int) {
                if (!current(link) || stage != Stage.SUBSCRIBING || descriptor.uuid != CCCD) return
                if (status != BluetoothGatt.GATT_SUCCESS) return fail("subscribeFailed")
                stage = Stage.CHALLENGE
                if (!link.readCharacteristic(challengeCharacteristic!!)) fail("challengeFailed")
            }
            override fun onCharacteristicRead(link: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray, status: Int) { onChallenge(link, characteristic, value, status) }
            @Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")
            override fun onCharacteristicRead(link: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) { onChallenge(link, characteristic, characteristic.value ?: byteArrayOf(), status) }
            override fun onCharacteristicWrite(link: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) {
                if (!current(link) || characteristic.uuid != RX || requestResult == null || !writing) return
                if (status != BluetoothGatt.GATT_SUCCESS) return fail("writeFailed")
                writing = false
                writeIndex++
                writeNext()
            }
            override fun onCharacteristicChanged(link: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray) { onNotification(link, characteristic, value) }
            @Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")
            override fun onCharacteristicChanged(link: BluetoothGatt, characteristic: BluetoothGattCharacteristic) { onNotification(link, characteristic, characteristic.value ?: byteArrayOf()) }
        }
        private fun onChallenge(link: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray, status: Int) {
            if (!current(link) || stage != Stage.CHALLENGE || characteristic.uuid != CHALLENGE) return
            if (status != BluetoothGatt.GATT_SUCCESS || value.size != 16) return fail("challengeFailed")
            try { cipher = ConsoleBleCipher(credential.secret, credential.session.copyOf(), value.copyOf()) }
            catch (_: Exception) { return fail("protocolError") }
            handler.removeCallbacks(timeout)
            stage = Stage.READY
            val callback = connectResult; connectResult = null
            callback?.success(null)
        }
        private fun onNotification(link: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray) {
            if (!current(link) || characteristic.uuid != TX) return
            if (stage != Stage.READY || requestResult == null || response != null) return fail("unexpectedResponse")
            try {
                val packet = receiver.accept(value, messageId) ?: return
                val plain = checkNotNull(cipher).decrypt(packet)
                response = Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(plain)).toString()
                deliverIfComplete()
            } catch (_: Exception) { fail("protocolError") }
        }
    }
    private enum class Stage { CONNECTING, MTU, DISCOVERING, SUBSCRIBING, CHALLENGE, READY, CLOSED }
    private companion object {
        fun uuid(part: String): UUID = UUID.fromString("A53E$part-7A6B-4D59-9F2E-5245504F5345")
        val SERVICE = uuid("0001"); val RX = uuid("0004"); val TX = uuid("0005"); val CHALLENGE = uuid("0006")
        val CCCD: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
    }
}
