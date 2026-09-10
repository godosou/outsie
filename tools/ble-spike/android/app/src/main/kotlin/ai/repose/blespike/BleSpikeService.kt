package ai.repose.blespike

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.BluetoothManager
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
 * Foreground service that broadcasts the `repose-presence-v1` beacon.
 *
 * WHAT CHANGED FROM THE B-SPIKE, AND WHY THE GATT SERVER IS GONE
 * --------------------------------------------------------------
 * The B-spike advertised a fixed 128-bit UUID and served a fixed string over GATT.
 * Both constants are published in this repository, so they identified nothing: any
 * device rebroadcasting them counted as "my phone" (E13). Identity now lives in the
 * advertisement itself, as a rotating HMAC over the current time window that only a
 * device holding the paired key can mint.
 *
 * The beacon is **non-connectable**, which reclaims the 3-byte Flags tax, lowers
 * power, and stops strangers connecting. That also makes a GATT server unreachable,
 * so it is not opened at all. A component that cannot be connected to, kept only to
 * feed a status indicator, is the kind of thing this project has learned to delete
 * rather than leave looking alive. The one result the GATT path produced -- real BLE
 * works between this phone and the Mac -- is recorded in
 * docs/validation/2026-09-09-b2-real-ble.md and does not need to keep running.
 */
@SuppressLint("MissingPermission")
class BleSpikeService : Service() {

    companion object {
        const val TAG = "BleSpike"
        private const val CHANNEL_ID = "ble_spike"
        private const val NOTIFICATION_ID = 41
        private const val HEARTBEAT_MS = 30_000L

        /**
         * How often we check whether the window counter has rolled. Well under
         * WINDOW_SECONDS: a beacon that goes stale is a phone that appears to have
         * left, so the check must never be the thing that is late.
         */
        private const val ROTATE_CHECK_MS = 2_000L
    }

    private val handler = Handler(Looper.getMainLooper())
    private var advertiser: BluetoothLeAdvertiser? = null
    private lateinit var beacon: PresenceBeacon
    private var advertisedCounter = Long.MIN_VALUE

    private val heartbeat = object : Runnable {
        override fun run() {
            Log.i(TAG, "heartbeat adv=${SpikeState.advertising} auth=${SpikeState.authentic}")
            SpikeState.event("heartbeat (still alive)")
            handler.postDelayed(this, HEARTBEAT_MS)
        }
    }

    /**
     * Re-mints the tag when the window rolls. Android's legacy advertiser has no way to
     * swap the payload in place, so this stops and restarts the set; the Mac's ±1 window
     * tolerance covers the sub-second gap. The restart also draws a fresh RPA, which no
     * longer matters -- identity is the key, not the address.
     */
    private val rotate = object : Runnable {
        override fun run() {
            val c = beacon.currentCounter()
            if (c != advertisedCounter) refreshBeacon(c)
            handler.postDelayed(this, ROTATE_CHECK_MS)
        }
    }

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            SpikeState.advertising = true
            SpikeState.beaconsSent++
            SpikeState.event(
                "beacon on air, window ${SpikeState.beaconCounter}" +
                    if (SpikeState.authentic) "" else " (UNPROVISIONED — tag is worthless)",
            )
            Log.i(TAG, "advertising started: $settingsInEffect")
        }

        override fun onStartFailure(errorCode: Int) {
            SpikeState.advertising = false
            SpikeState.event("advertising FAILED code=$errorCode")
            Log.e(TAG, "advertising failed, code=$errorCode")
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

        // Before anything goes on the air, take a key if one was pushed for us.
        PresenceKey.ingestProvisionedKey(this, SpikeContract.PRESENCE_KEY_ID)
            ?.let { SpikeState.event(it) }

        beacon = PresenceBeacon(SpikeContract.PRESENCE_KEY_ID)
        SpikeState.authentic = beacon.authentic
        SpikeState.fingerprint = PresenceKey.fingerprint(this)
        if (!beacon.authentic) {
            SpikeState.event(
                "no presence key: broadcasting an invalid tag. The Mac will see this " +
                    "device and refuse it.",
            )
        }

        val adapter = getSystemService(BluetoothManager::class.java)?.adapter
        if (adapter == null || !adapter.isEnabled) {
            SpikeState.event("Bluetooth is OFF - enable it and restart")
            return
        }
        advertiser = adapter.bluetoothLeAdvertiser
        if (advertiser == null) SpikeState.event("no LE advertiser (peripheral role unsupported?)")

        handler.post(rotate)
        handler.postDelayed(heartbeat, HEARTBEAT_MS)
        SpikeState.notifyListeners()
    }

    private fun refreshBeacon(counter: Long) {
        val le = advertiser ?: return
        val payload = runCatching { beacon.payloadFor(counter) }.getOrElse { e ->
            // A Keystore that will not sign is a phone that cannot prove who it is.
            // Falling back to an unsigned packet here would quietly turn the imposter
            // and the real phone back into the same thing, so it goes silent instead.
            SpikeState.advertising = false
            runCatching { le.stopAdvertising(advertiseCallback) }
            SpikeState.event("cannot mint a tag (${e.message}); stopped advertising")
            Log.e(TAG, "beacon mint failed", e)
            return
        }

        runCatching { le.stopAdvertising(advertiseCallback) }
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .setConnectable(false)
            .setTimeout(0)
            .build()
        // The UUID list AD is what CoreBluetooth's `withServices:` filter matches on --
        // service data alone does not satisfy it. Both fit easily now that the UUID is
        // 16-bit: 4 + 4 + 10 = 18 of 31 bytes, with no Flags tax when non-connectable.
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .addServiceUuid(ParcelUuid(SpikeContract.PRESENCE_SERVICE_UUID))
            .addServiceData(ParcelUuid(SpikeContract.PRESENCE_SERVICE_UUID), payload)
            .build()
        le.startAdvertising(settings, data, advertiseCallback)

        advertisedCounter = counter
        SpikeState.beaconCounter = counter
        SpikeState.authentic = beacon.authentic
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_STICKY

    override fun onDestroy() {
        handler.removeCallbacks(heartbeat)
        handler.removeCallbacks(rotate)
        runCatching { advertiser?.stopAdvertising(advertiseCallback) }
        advertiser = null
        SpikeState.serviceRunning = false
        SpikeState.advertising = false
        SpikeState.startedAtUptime = 0L
        SpikeState.event("service destroyed")
        Log.w(TAG, "service destroyed")
        super.onDestroy()
    }

    private fun buildNotification(): Notification =
        Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("Repose 在场信标")
            .setContentText(
                if (SpikeState.authentic) "广播中 · 已配置密钥" else "广播中 · 未配置密钥（无效标签）",
            )
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .build()
}
