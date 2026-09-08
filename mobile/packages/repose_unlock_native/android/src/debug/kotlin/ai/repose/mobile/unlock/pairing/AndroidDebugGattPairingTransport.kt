package ai.repose.mobile.unlock.pairing

import android.Manifest
import android.annotation.SuppressLint
import android.annotation.TargetApi
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.companion.AssociationInfo
import android.companion.CompanionDeviceManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import ai.repose.mobile.unlock.companion.canonicalBluetoothAddress
import ai.repose.mobile.unlock.companion.legacyAssociationToken
import ai.repose.mobile.unlock.protocol.PairingProtocolV1
import java.util.UUID

@TargetApi(31)
internal class AndroidDebugGattPairingTransport(
    context: Context,
    private val timeoutScheduler: DebugGattTimeoutScheduler = HandlerDebugGattTimeoutScheduler(),
) : DebugGattPairingTransport {
    private val applicationContext = context.applicationContext

    @SuppressLint("MissingPermission")
    override fun connect(
        request: DebugGattPairingRequest,
        observer: DebugGattPairingObserver,
    ): DebugGattPairingConnection {
        if (
            request.controlValue.isEmpty() ||
            request.controlValue.size > MAX_ATTRIBUTE_VALUE ||
            request.expectedAcceptedStatus.isEmpty() ||
            request.expectedAcceptedStatus.size > MAX_ATTRIBUTE_VALUE
        ) {
            throw DebugGattUnavailableException()
        }
        if (applicationContext.checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            throw DebugGattUnavailableException()
        }
        val adapter = applicationContext
            .getSystemService(BluetoothManager::class.java)
            ?.adapter
            ?.takeIf { it.isEnabled }
            ?: throw DebugGattUnavailableException()
        val manager = applicationContext.getSystemService(CompanionDeviceManager::class.java)
            ?: throw DebugGattUnavailableException()
        val device = resolveAssociatedDevice(manager, adapter, request.associationId)
            ?: throw DebugGattUnavailableException()
        return GattConnection(
            applicationContext,
            device,
            request.controlValue.copyOf(),
            request.expectedAcceptedStatus.copyOf(),
            observer,
            timeoutScheduler,
        ).also(GattConnection::open)
    }

    @SuppressLint("MissingPermission")
    private fun resolveAssociatedDevice(
        manager: CompanionDeviceManager,
        adapter: android.bluetooth.BluetoothAdapter,
        associationToken: Int,
    ): BluetoothDevice? {
        if (associationToken >= 0) {
            if (Build.VERSION.SDK_INT < 33) return null
            return Api33DebugAssociationDeviceResolver.resolve(
                manager,
                adapter,
                associationToken,
            )
        }

        @Suppress("DEPRECATION")
        val matchingAddress = try {
            manager.associations
                .mapNotNull(::canonicalBluetoothAddress)
                .singleOrNull { legacyAssociationToken(it) == associationToken }
        } catch (_: RuntimeException) {
            null
        } ?: return null
        return try {
            adapter.getRemoteDevice(matchingAddress)
        } catch (_: RuntimeException) {
            null
        }
    }

    private class GattConnection(
        private val context: Context,
        private val device: BluetoothDevice,
        controlValue: ByteArray,
        expectedAcceptedStatus: ByteArray,
        private val observer: DebugGattPairingObserver,
        timeoutScheduler: DebugGattTimeoutScheduler,
    ) : DebugGattPairingConnection {
        private val monitor = Any()
        private val controlValue = controlValue.copyOf()
        private val handshakeDeadline = DebugGattHandshakeDeadline(
            scheduler = timeoutScheduler,
            delayMillis = HANDSHAKE_TIMEOUT_MILLIS,
            expectedAcceptedStatus = expectedAcceptedStatus,
            onTimeout = {
                closeLink(notifyFailure = true, notifyDisconnected = false)
            },
        )
        private val callback = Callback()
        private var gatt: BluetoothGatt? = null
        private var stage = Stage.CONNECTING
        private var statusCharacteristic: BluetoothGattCharacteristic? = null

        @SuppressLint("MissingPermission")
        fun open() {
            try {
                handshakeDeadline.arm()
            } catch (_: RuntimeException) {
                throw DebugGattUnavailableException()
            }
            val opened = try {
                device.connectGatt(context, false, callback, BluetoothDevice.TRANSPORT_LE)
            } catch (_: RuntimeException) {
                null
            }
            if (opened == null) {
                handshakeDeadline.close()
                throw DebugGattUnavailableException()
            }
            synchronized(monitor) {
                if (stage == Stage.CLOSED) {
                    opened.closeSafely()
                } else {
                    gatt = opened
                }
            }
        }

        override fun close() {
            closeLink(notifyFailure = false, notifyDisconnected = false)
        }

        private inner class Callback : BluetoothGattCallback() {
            @SuppressLint("MissingPermission")
            override fun onConnectionStateChange(
                callbackGatt: BluetoothGatt,
                status: Int,
                newState: Int,
            ) {
                if (!isCurrent(callbackGatt)) return
                if (status != BluetoothGatt.GATT_SUCCESS) {
                    fail(callbackGatt)
                    return
                }
                when (newState) {
                    BluetoothProfile.STATE_CONNECTED -> synchronized(monitor) {
                        if (stage != Stage.CONNECTING) return
                        stage = Stage.NEGOTIATING_MTU
                        if (!callbackGatt.requestMtu(TARGET_MTU)) fail(callbackGatt)
                    }
                    BluetoothProfile.STATE_DISCONNECTED -> disconnect(callbackGatt)
                }
            }

            @SuppressLint("MissingPermission")
            override fun onMtuChanged(callbackGatt: BluetoothGatt, mtu: Int, status: Int) {
                synchronized(monitor) {
                    if (!isCurrentLocked(callbackGatt) || stage != Stage.NEGOTIATING_MTU) return
                    if (
                        status != BluetoothGatt.GATT_SUCCESS ||
                        mtu < controlValue.size + ATT_WRITE_OVERHEAD
                    ) {
                        fail(callbackGatt)
                        return
                    }
                    stage = Stage.DISCOVERING_SERVICES
                    if (!callbackGatt.discoverServices()) fail(callbackGatt)
                }
            }

            @SuppressLint("MissingPermission")
            override fun onServicesDiscovered(callbackGatt: BluetoothGatt, status: Int) {
                synchronized(monitor) {
                    if (!isCurrentLocked(callbackGatt) || stage != Stage.DISCOVERING_SERVICES) {
                        return
                    }
                    if (status != BluetoothGatt.GATT_SUCCESS) {
                        fail(callbackGatt)
                        return
                    }
                    val service = callbackGatt.getService(SERVICE_UUID)
                    val control = service?.getCharacteristic(CONTROL_UUID)
                    val statusValue = service?.getCharacteristic(STATUS_UUID)
                    val descriptor = statusValue?.getDescriptor(CLIENT_CONFIGURATION_UUID)
                    if (control == null || statusValue == null || descriptor == null) {
                        fail(callbackGatt)
                        return
                    }
                    val validProperties =
                        control.properties.and(BluetoothGattCharacteristic.PROPERTY_WRITE) != 0 &&
                            statusValue.properties.and(
                                BluetoothGattCharacteristic.PROPERTY_READ,
                            ) != 0 &&
                            statusValue.properties.and(
                                BluetoothGattCharacteristic.PROPERTY_NOTIFY,
                            ) != 0
                    if (!validProperties) {
                        fail(callbackGatt)
                        return
                    }
                    statusCharacteristic = statusValue
                    if (!callbackGatt.setCharacteristicNotification(statusValue, true)) {
                        fail(callbackGatt)
                        return
                    }
                    stage = Stage.ENABLING_NOTIFICATIONS
                    if (!writeDescriptor(callbackGatt, descriptor)) fail(callbackGatt)
                }
            }

            @SuppressLint("MissingPermission")
            override fun onDescriptorWrite(
                callbackGatt: BluetoothGatt,
                descriptor: BluetoothGattDescriptor,
                status: Int,
            ) {
                synchronized(monitor) {
                    if (
                        !isCurrentLocked(callbackGatt) ||
                        stage != Stage.ENABLING_NOTIFICATIONS ||
                        descriptor.uuid != CLIENT_CONFIGURATION_UUID
                    ) {
                        return
                    }
                    if (status != BluetoothGatt.GATT_SUCCESS) {
                        fail(callbackGatt)
                        return
                    }
                    val control = callbackGatt.getService(SERVICE_UUID)
                        ?.getCharacteristic(CONTROL_UUID)
                    if (control == null) {
                        fail(callbackGatt)
                        return
                    }
                    stage = Stage.WRITING_CONTROL
                    if (!writeControl(callbackGatt, control)) fail(callbackGatt)
                }
            }

            @SuppressLint("MissingPermission")
            override fun onCharacteristicWrite(
                callbackGatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
                status: Int,
            ) {
                synchronized(monitor) {
                    if (
                        !isCurrentLocked(callbackGatt) ||
                        stage != Stage.WRITING_CONTROL ||
                        characteristic.uuid != CONTROL_UUID
                    ) {
                        return
                    }
                    if (status != BluetoothGatt.GATT_SUCCESS) {
                        fail(callbackGatt)
                        return
                    }
                    val statusValue = statusCharacteristic
                    if (statusValue == null) {
                        fail(callbackGatt)
                        return
                    }
                    stage = Stage.READING_STATUS
                    if (!callbackGatt.readCharacteristic(statusValue)) fail(callbackGatt)
                }
            }

            override fun onCharacteristicChanged(
                callbackGatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
                value: ByteArray,
            ) {
                deliverStatus(callbackGatt, characteristic, value)
            }

            @Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")
            override fun onCharacteristicChanged(
                callbackGatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
            ) {
                deliverStatus(callbackGatt, characteristic, characteristic.value ?: return)
            }

            override fun onCharacteristicRead(
                callbackGatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
                value: ByteArray,
                status: Int,
            ) {
                readStatus(callbackGatt, characteristic, value, status)
            }

            @Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")
            override fun onCharacteristicRead(
                callbackGatt: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
                status: Int,
            ) {
                readStatus(callbackGatt, characteristic, characteristic.value ?: return, status)
            }
        }

        private fun readStatus(
            callbackGatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
            status: Int,
        ) {
            synchronized(monitor) {
                if (
                    !isCurrentLocked(callbackGatt) ||
                    stage != Stage.READING_STATUS ||
                    characteristic.uuid != STATUS_UUID
                ) {
                    return
                }
                if (status != BluetoothGatt.GATT_SUCCESS) {
                    fail(callbackGatt)
                    return
                }
                stage = Stage.READY
            }
            val snapshot = value.copyOf()
            if (handshakeDeadline.permitStatus(snapshot)) observer.onStatus(snapshot)
        }

        private fun deliverStatus(
            callbackGatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
        ) {
            val deliver = synchronized(monitor) {
                isCurrentLocked(callbackGatt) &&
                    characteristic.uuid == STATUS_UUID &&
                    stage in STATUS_DELIVERY_STAGES
            }
            if (deliver) {
                val snapshot = value.copyOf()
                if (handshakeDeadline.permitStatus(snapshot)) observer.onStatus(snapshot)
            }
        }

        private fun isCurrent(callbackGatt: BluetoothGatt): Boolean = synchronized(monitor) {
            isCurrentLocked(callbackGatt)
        }

        private fun isCurrentLocked(callbackGatt: BluetoothGatt): Boolean =
            stage != Stage.CLOSED && (gatt == null || gatt === callbackGatt)

        private fun fail(callbackGatt: BluetoothGatt) {
            if (isCurrent(callbackGatt)) {
                closeLink(notifyFailure = true, notifyDisconnected = false)
            }
        }

        private fun disconnect(callbackGatt: BluetoothGatt) {
            if (isCurrent(callbackGatt)) {
                closeLink(notifyFailure = false, notifyDisconnected = true)
            }
        }

        @SuppressLint("MissingPermission")
        private fun closeLink(notifyFailure: Boolean, notifyDisconnected: Boolean) {
            val closing = synchronized(monitor) {
                if (stage == Stage.CLOSED) return
                stage = Stage.CLOSED
                statusCharacteristic = null
                gatt.also { gatt = null }
            }
            closing?.disconnectSafely()
            closing?.closeSafely()
            handshakeDeadline.close()
            controlValue.fill(0)
            when {
                notifyFailure -> CALLBACK_HANDLER.post(observer::onFailure)
                notifyDisconnected -> CALLBACK_HANDLER.post(observer::onDisconnected)
            }
        }

        @Suppress("DEPRECATION")
        @SuppressLint("MissingPermission")
        private fun writeDescriptor(
            callbackGatt: BluetoothGatt,
            descriptor: BluetoothGattDescriptor,
        ): Boolean = if (Build.VERSION.SDK_INT >= 33) {
            callbackGatt.writeDescriptor(
                descriptor,
                BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE,
            ) == BluetoothStatusCodes.SUCCESS
        } else {
            descriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
            callbackGatt.writeDescriptor(descriptor)
        }

        @Suppress("DEPRECATION")
        @SuppressLint("MissingPermission")
        private fun writeControl(
            callbackGatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
        ): Boolean = if (Build.VERSION.SDK_INT >= 33) {
            callbackGatt.writeCharacteristic(
                characteristic,
                controlValue,
                BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT,
            ) == BluetoothStatusCodes.SUCCESS
        } else {
            characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
            characteristic.value = controlValue
            callbackGatt.writeCharacteristic(characteristic)
        }

        private enum class Stage {
            CONNECTING,
            NEGOTIATING_MTU,
            DISCOVERING_SERVICES,
            ENABLING_NOTIFICATIONS,
            WRITING_CONTROL,
            READING_STATUS,
            READY,
            CLOSED,
        }

        private companion object {
            val STATUS_DELIVERY_STAGES = setOf(
                Stage.WRITING_CONTROL,
                Stage.READING_STATUS,
                Stage.READY,
            )
        }
    }

    private companion object {
        const val TARGET_MTU = 247
        const val HANDSHAKE_TIMEOUT_MILLIS = 15_000L
        const val ATT_WRITE_OVERHEAD = 3
        const val MAX_ATTRIBUTE_VALUE = 512
        val CALLBACK_HANDLER = Handler(Looper.getMainLooper())
        val SERVICE_UUID: UUID = UUID.fromString(PairingProtocolV1.bleServiceUuid)
        val CONTROL_UUID: UUID = UUID.fromString(
            PairingProtocolV1.bleControlCharacteristicUuid,
        )
        val STATUS_UUID: UUID = UUID.fromString(
            PairingProtocolV1.bleStatusCharacteristicUuid,
        )
        val CLIENT_CONFIGURATION_UUID: UUID = UUID.fromString(
            "00002902-0000-1000-8000-00805f9b34fb",
        )
    }
}

@TargetApi(33)
private object Api33DebugAssociationDeviceResolver {
    @SuppressLint("MissingPermission")
    fun resolve(
        manager: CompanionDeviceManager,
        adapter: android.bluetooth.BluetoothAdapter,
        associationToken: Int,
    ): BluetoothDevice? {
        val association = try {
            manager.myAssociations.singleOrNull { it.id == associationToken }
        } catch (_: RuntimeException) {
            null
        } ?: return null
        return resolve(association, adapter)
    }

    @SuppressLint("MissingPermission")
    private fun resolve(
        association: AssociationInfo,
        adapter: android.bluetooth.BluetoothAdapter,
    ): BluetoothDevice? = try {
        val associatedBle = if (Build.VERSION.SDK_INT >= 34) {
            association.associatedDevice?.bleDevice?.device
        } else {
            null
        }
        val associatedBluetooth = if (Build.VERSION.SDK_INT >= 34) {
            association.associatedDevice?.bluetoothDevice
        } else {
            null
        }
        associatedBle
            ?: association.deviceMacAddress?.toString()?.let(adapter::getRemoteDevice)
            ?: associatedBluetooth
    } catch (_: RuntimeException) {
        null
    }
}

private class HandlerDebugGattTimeoutScheduler(
    private val handler: Handler = Handler(Looper.getMainLooper()),
) : DebugGattTimeoutScheduler {
    override fun schedule(
        delayMillis: Long,
        callback: () -> Unit,
    ): DebugGattTimeoutTicket {
        val runnable = Runnable(callback)
        if (!handler.postDelayed(runnable, delayMillis)) throw DebugGattUnavailableException()
        return DebugGattTimeoutTicket { handler.removeCallbacks(runnable) }
    }
}

private class DebugGattUnavailableException : IllegalStateException(
    "The debug GATT pairing transport is unavailable.",
)

@SuppressLint("MissingPermission")
private fun BluetoothGatt.disconnectSafely() {
    try {
        disconnect()
    } catch (_: RuntimeException) {
        // Authority is already removed before transport cleanup.
    }
}

private fun BluetoothGatt.closeSafely() {
    try {
        close()
    } catch (_: RuntimeException) {
        // Authority is already removed before transport cleanup.
    }
}
