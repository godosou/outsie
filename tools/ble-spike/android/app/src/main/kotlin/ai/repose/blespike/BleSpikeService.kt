package ai.repose.blespike

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.content.Intent
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.ParcelUuid
import android.os.SystemClock
import android.util.Log

/**
 * Foreground service holding the GATT server and the LE advertiser.
 * The whole point of the foreground service is to find out whether realme UI keeps
 * advertising alive once the screen goes off.
 */
@SuppressLint("MissingPermission")
class BleSpikeService : Service() {

    companion object {
        const val TAG = "BleSpike"
        private const val CHANNEL_ID = "ble_spike"
        private const val NOTIFICATION_ID = 41
        private const val HEARTBEAT_MS = 30_000L
    }

    private val payload = SpikeContract.PAYLOAD.toByteArray(Charsets.US_ASCII)
    private val handler = Handler(Looper.getMainLooper())
    private var gattServer: BluetoothGattServer? = null
    private var advertiser: BluetoothLeAdvertiser? = null

    private val heartbeat = object : Runnable {
        override fun run() {
            Log.i(TAG, "heartbeat adv=${SpikeState.advertising} live=${SpikeState.liveConnections}")
            SpikeState.event("heartbeat (still alive)")
            handler.postDelayed(this, HEARTBEAT_MS)
        }
    }

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            SpikeState.advertising = true
            SpikeState.event("advertising started")
            Log.i(TAG, "advertising started: $settingsInEffect")
        }

        override fun onStartFailure(errorCode: Int) {
            SpikeState.advertising = false
            SpikeState.event("advertising FAILED code=$errorCode")
            Log.e(TAG, "advertising failed, code=$errorCode")
        }
    }

    private val gattCallback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            if (newState == BluetoothProfile.STATE_CONNECTED) {
                SpikeState.liveConnections++
                SpikeState.totalConnections++
                SpikeState.event("connected ${device.address}")
            } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                SpikeState.liveConnections = (SpikeState.liveConnections - 1).coerceAtLeast(0)
                SpikeState.event("disconnected ${device.address} status=$status")
                // Android's legacy advertiser stops once a peripheral connection is
                // established and never resumes on its own. Without this the phone goes
                // silent after the Mac's first read, which looks exactly like the OS
                // killing BLE in Doze. That false negative is the one result this whole
                // experiment cannot afford, so advertising is restarted on every
                // disconnect.
                restartAdvertising()
            }
            Log.i(TAG, "conn ${device.address} status=$status newState=$newState")
            updateNotification()
        }

        override fun onCharacteristicReadRequest(
            device: BluetoothDevice,
            requestId: Int,
            offset: Int,
            characteristic: BluetoothGattCharacteristic,
        ) {
            val server = gattServer ?: return
            if (characteristic.uuid != SpikeContract.CHARACTERISTIC_UUID) {
                server.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
                return
            }
            if (offset > payload.size) {
                server.sendResponse(device, requestId, BluetoothGatt.GATT_INVALID_OFFSET, offset, null)
                return
            }
            val slice = payload.copyOfRange(offset, payload.size)
            server.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, slice)
            SpikeState.reads++
            SpikeState.event("read served to ${device.address} (offset=$offset)")
            Log.i(TAG, "read served to ${device.address}")
            updateNotification()
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "BLE spike", NotificationManager.IMPORTANCE_LOW),
        )
        startForeground(NOTIFICATION_ID, buildNotification())
        SpikeState.serviceRunning = true
        SpikeState.startedAtUptime = SystemClock.elapsedRealtime()
        SpikeState.event("service started")

        val manager = getSystemService(BluetoothManager::class.java)
        val adapter = manager?.adapter
        if (adapter == null || !adapter.isEnabled) {
            SpikeState.event("Bluetooth is OFF - enable it and restart")
            return
        }
        if (!adapter.isMultipleAdvertisementSupported) {
            SpikeState.event("warning: chipset reports no multi-advertisement support")
        }

        openGattServer(manager)
        startAdvertising(adapter.bluetoothLeAdvertiser)
        handler.postDelayed(heartbeat, HEARTBEAT_MS)
        SpikeState.notifyListeners()
    }

    private fun openGattServer(manager: BluetoothManager) {
        val server = manager.openGattServer(this, gattCallback)
        if (server == null) {
            SpikeState.event("openGattServer returned null")
            return
        }
        val characteristic = BluetoothGattCharacteristic(
            SpikeContract.CHARACTERISTIC_UUID,
            BluetoothGattCharacteristic.PROPERTY_READ,
            BluetoothGattCharacteristic.PERMISSION_READ,
        )
        val service = BluetoothGattService(
            SpikeContract.SERVICE_UUID,
            BluetoothGattService.SERVICE_TYPE_PRIMARY,
        ).apply { addCharacteristic(characteristic) }
        server.addService(service)
        gattServer = server
        SpikeState.gattOpen = true
        SpikeState.event("gatt server open")
    }

    /**
     * Resume advertising after a peripheral connection ends. Posted to the main thread
     * because this is called from a binder callback, with a short delay to let the
     * stack finish tearing the connection down before a new advertising set is opened.
     */
    private fun restartAdvertising() {
        handler.postDelayed({
            val adapter = getSystemService(BluetoothManager::class.java)?.adapter
            if (adapter == null || !adapter.isEnabled) {
                SpikeState.advertising = false
                SpikeState.event("cannot resume advertising: Bluetooth is off")
                SpikeState.notifyListeners()
                return@postDelayed
            }
            // Stopping first keeps a second start from failing with ALREADY_STARTED if
            // the stack happened to leave the previous set running.
            runCatching { advertiser?.stopAdvertising(advertiseCallback) }
            SpikeState.advertising = false
            startAdvertising(adapter.bluetoothLeAdvertiser)
            SpikeState.event("advertising restart requested after disconnect")
            SpikeState.notifyListeners()
        }, 250L)
    }

    private fun startAdvertising(leAdvertiser: BluetoothLeAdvertiser?) {
        if (leAdvertiser == null) {
            SpikeState.event("no LE advertiser (peripheral role unsupported?)")
            return
        }
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .setConnectable(true)
            .setTimeout(0)
            .build()
        // A 128-bit UUID eats 18 of the 31 advertising bytes, so the name goes in the
        // scan response instead of the primary packet.
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .addServiceUuid(ParcelUuid(SpikeContract.SERVICE_UUID))
            .build()
        val scanResponse = AdvertiseData.Builder().setIncludeDeviceName(true).build()
        leAdvertiser.startAdvertising(settings, data, scanResponse, advertiseCallback)
        advertiser = leAdvertiser
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_STICKY

    override fun onDestroy() {
        handler.removeCallbacks(heartbeat)
        runCatching { advertiser?.stopAdvertising(advertiseCallback) }
        runCatching { gattServer?.close() }
        advertiser = null
        gattServer = null
        SpikeState.serviceRunning = false
        SpikeState.advertising = false
        SpikeState.gattOpen = false
        SpikeState.liveConnections = 0
        SpikeState.startedAtUptime = 0L
        SpikeState.event("service destroyed")
        Log.w(TAG, "service destroyed")
        super.onDestroy()
    }

    private fun buildNotification(): Notification =
        Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("Repose BLE spike")
            .setContentText("adv=${SpikeState.advertising} live=${SpikeState.liveConnections}")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .build()

    private fun updateNotification() =
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
}
