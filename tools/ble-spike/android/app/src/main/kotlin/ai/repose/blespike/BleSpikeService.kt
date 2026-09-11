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

        /** Sent after a successful pairing, which lands the key in a new slot. */
        const val ACTION_REBUILD_BEACON = "ai.repose.blespike.REBUILD_BEACON"
        private const val CHANNEL_ID = "ble_spike"
        private const val NOTIFICATION_ID = 41
        private const val HEARTBEAT_MS = 30_000L

        /**
         * How often we check whether the window counter has rolled. Well under
         * WINDOW_SECONDS: a beacon that goes stale is a phone that appears to have
         * left, so the check must never be the thing that is late.
         */
        private const val ROTATE_CHECK_MS = 2_000L

        /**
         * How soon after a button press the new payload goes on the air.
         *
         * The ordinary rotate check is fine for a window that turns over every
         * 30s, but someone who just pressed 锁定 Mac is watching their screen.
         * Two seconds of nothing reads as a button that did not work.
         */
        private const val COMMAND_CHECK_MS = 400L

        /**
         * The command the beacon is currently carrying, and when it expires.
         *
         * A command is not a message that gets delivered once — it rides in
         * every advertisement for [SpikeContract.COMMAND_BROADCAST_MS] and then
         * stops. That is deliberate: the beacon is one-way, so there is no
         * acknowledgement to wait for, and a single packet is easily the one
         * that lands in a scan gap (measured p99 ~7s between sightings of a
         * single advertiser — see docs/validation/2026-09-10-scan-cadence.md).
         * Repeating for a few seconds is the only delivery guarantee available.
         *
         * Static because the UI posts commands while the service is the thing
         * that advertises them, and there is exactly one of each.
         */
        @Volatile private var pendingCmd = SpikeContract.CMD_NONE
        @Volatile private var pendingSeq = 0L
        @Volatile private var pendingUntil = 0L

        /**
         * Queue a command for the Mac. Returns false if the phone has no key,
         * because an unauthenticated command is one the Mac will refuse — and
         * a button that silently does nothing is worse than one that says why.
         */
        fun postCommand(context: android.content.Context, cmd: Int): Boolean {
            if (!PresenceKey.has(SpikeContract.PRESENCE_KEY_ID)) return false
            pendingSeq = AppStore(context).nextCommandSeq()
            pendingCmd = cmd
            pendingUntil = SystemClock.elapsedRealtime() + SpikeContract.COMMAND_BROADCAST_MS
            SpikeState.event(
                when (cmd) {
                    SpikeContract.CMD_LOCK -> "已发出：锁定 Mac"
                    else -> "已发出指令 $cmd"
                },
            )
            return true
        }
    }

    private val handler = Handler(Looper.getMainLooper())
    private var advertiser: BluetoothLeAdvertiser? = null
    /** Listens for the Mac's own beacon. Silent without a key or the permission. */
    private var macStateScanner: MacStateScanner? = null
    /**
     * One advertiser per paired Mac.
     *
     * Each Mac derives its own key (pair-v3), so a phone paired with two of them
     * holds two keys and has to prove itself to both. Android allows several
     * concurrent advertising instances, so they run side by side rather than
     * taking turns -- alternating would multiply every Mac's time-to-notice by
     * the number of Macs, and the presence decision already has a hard enough
     * time with the scan-window tail (see the note on `rotate`).
     *
     * With one Mac this is exactly what it was: one beacon, one callback.
     */
    private class Slot(val beacon: PresenceBeacon, val callback: AdvertiseCallback) {
        var advertisedCounter = Long.MIN_VALUE
        var advertisedCmd = SpikeContract.CMD_NONE
    }

    private var slots: List<Slot> = emptyList()

    private val heartbeat = object : Runnable {
        override fun run() {
            Log.i(TAG, "heartbeat adv=${SpikeState.advertising} auth=${SpikeState.authentic}")
            SpikeState.event("heartbeat (still alive)")
            handler.postDelayed(this, HEARTBEAT_MS)
        }
    }

    /**
     * Re-mints the tag when the window rolls, by stopping and restarting advertising.
     * The Mac's ±1 window tolerance covers the 1-3s gap that costs. The restart also
     * draws a fresh private address, which no longer matters for identity -- that is
     * the key -- and is mildly good for privacy, since rotating every 30s is harder to
     * follow than the system's own ~15 minute schedule.
     *
     * TRIED AND REVERTED: startAdvertisingSet + setAdvertisingData, swapping the ten
     * payload bytes in place so the instance and its address survive. The theory was
     * that churning the advertising instance caused the multi-second holes the Mac
     * sees. Measured over 150s each, phone motionless 1m away:
     *
     *   stop/start, MEDIUM   p50 0.27  p90 2.98  p99 6.29  max 9.29   n=146   6 addrs
     *   in-place,   MEDIUM   p50 1.23  p90 3.93  p99 7.49  max 8.29   n= 89   1 addr
     *   in-place,   HIGH     p50 1.23  p90 4.23  p99 7.82  max 9.29   n= 89   1 addr
     *
     * The holes did not move. Raising tx power did not move them either (it only
     * lifted median RSSI from -67 to -62). What did change was throughput: in-place
     * cost 39% of the sightings, and pinning one address is slightly worse for
     * privacy than forcing a rotation every window.
     *
     * So the holes are not this phone's doing. During the same capture the Mac saw
     * 3177 advertisements from 230 other devices with a worst-case gap of 0.57s --
     * its radio never went quiet. A single advertiser is limited by how often its
     * packets land inside a scan window, and that tail is macOS's to set, not ours.
     * No phone-side knob reaches it, which is why the presence decision must not be
     * built on packet recency alone. See docs/validation/2026-09-10-scan-cadence.md.
     */
    private val rotate = object : Runnable {
        override fun run() {
            val first = slots.firstOrNull() ?: return
            val c = first.beacon.currentCounter()
            // A command has to go out now, not at the next 30s window boundary,
            // and it has to stop going out when it expires. Both are changes to
            // what the payload should say, so both force a refresh.
            val cmdNow = liveCommand()
            // Any slot out of date refreshes them all: they share a window and
            // a command, so they are never legitimately out of step.
            if (slots.any { it.advertisedCounter != c || it.advertisedCmd != cmdNow }) refreshBeacon(c)
            // Poll faster while something is pending, so the press-to-air delay
            // is not itself mistaken for the radio being slow.
            val pending = pendingCmd != SpikeContract.CMD_NONE &&
                SystemClock.elapsedRealtime() < pendingUntil
            handler.postDelayed(this, if (pending) COMMAND_CHECK_MS else ROTATE_CHECK_MS)
        }
    }

    /** The command that should be on air right now, or CMD_NONE once it expires. */
    private fun liveCommand(): Int =
        if (pendingCmd != SpikeContract.CMD_NONE &&
            SystemClock.elapsedRealtime() < pendingUntil
        ) {
            pendingCmd
        } else {
            SpikeContract.CMD_NONE
        }

    /**
     * One per slot, because stopAdvertising takes the callback as its handle --
     * sharing one across instances would make it impossible to stop a single
     * advertiser.
     */
    private fun callbackFor(keyId: Int) = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            SpikeState.advertising = true
            SpikeState.beaconsSent++
            SpikeState.event(
                "钥匙 $keyId 的信标上天了，窗口 ${SpikeState.beaconCounter}" +
                    if (SpikeState.authentic) "" else "（还没配对 —— 这个 tag 没有意义）",
            )
            Log.i(TAG, "advertising started for keyId=$keyId: $settingsInEffect")
        }

        override fun onStartFailure(errorCode: Int) {
            // Not a global "advertising off": another slot may be fine, and
            // saying the phone is silent when one of two Macs can still see it
            // would be the screen reporting something that is not true.
            SpikeState.event("钥匙 $keyId 的信标发不出去，错误码 $errorCode")
            Log.e(TAG, "advertising failed for keyId=$keyId, code=$errorCode")
            if (slots.none { it.advertisedCounter != Long.MIN_VALUE }) {
                SpikeState.advertising = false
            }
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

        buildSlots()
        SpikeState.fingerprint = PresenceKey.fingerprint(this)
        if (slots.none { it.beacon.authentic }) {
            SpikeState.event(
                "还没有配对：Mac 会看见这台手机，但认不出是你的。",
            )
        }

        val adapter = getSystemService(BluetoothManager::class.java)?.adapter
        if (adapter == null || !adapter.isEnabled) {
            SpikeState.event("Bluetooth is OFF - enable it and restart")
            return
        }
        advertiser = adapter.bluetoothLeAdvertiser
        if (advertiser == null) SpikeState.event("no LE advertiser (peripheral role unsupported?)")

        // Only worth listening once there is a key: an unverifiable beacon
        // tells this phone nothing, and scanning for it would be battery spent
        // on a sentence that could never be shown.
        if (slots.any { it.beacon.authentic }) {
            macStateScanner = MacStateScanner(this).also { it.start() }
        }

        handler.post(rotate)
        handler.postDelayed(heartbeat, HEARTBEAT_MS)
        SpikeState.notifyListeners()
    }

    /** Re-mint and re-advertise every slot. */
    private fun refreshBeacon(counter: Long) {
        val le = advertiser ?: return
        val cmd = liveCommand()
        var any = false
        for (slot in slots) {
            if (refreshSlot(le, slot, counter, cmd)) any = true
        }
        // The screen's one-line answer. It is about the phone, not about a
        // particular Mac: with two paired and one advertiser failing, the phone
        // IS being seen, and saying otherwise would be worse than saying less.
        SpikeState.advertising = any
        SpikeState.beaconCounter = counter
        SpikeState.authentic = slots.all { it.beacon.authentic } && slots.isNotEmpty()
    }

    private fun refreshSlot(
        le: BluetoothLeAdvertiser,
        slot: Slot,
        counter: Long,
        cmd: Int,
    ): Boolean {
        val payload = runCatching { slot.beacon.payloadFor(counter, cmd, pendingSeq) }.getOrElse { e ->
            // A Keystore that will not sign is a phone that cannot prove who it is.
            // Falling back to an unsigned packet here would quietly turn the imposter
            // and the real phone back into the same thing, so it goes silent instead.
            runCatching { le.stopAdvertising(slot.callback) }
            slot.advertisedCounter = Long.MIN_VALUE
            SpikeState.event("钥匙 ${slot.beacon.keyId} 签不出 tag（${e.message}），这一路停了")
            Log.e(TAG, "beacon mint failed for keyId=${slot.beacon.keyId}", e)
            return false
        }

        runCatching { le.stopAdvertising(slot.callback) }
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
        le.startAdvertising(settings, data, slot.callback)

        slot.advertisedCounter = counter
        slot.advertisedCmd = cmd
        return true
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Pairing writes a key into a NEW slot, and the beacon object was built
        // in onCreate against whichever slot existed then. Android delivers this
        // to onStartCommand, not onCreate, for a service that is already
        // running -- so without this, a phone that had just paired went on
        // advertising under its previous key id, and the Mac it had just been
        // introduced to heard nothing it could verify.
        if (intent?.action == ACTION_REBUILD_BEACON) rebuildBeacon()
        return START_STICKY
    }

    /**
     * One advertiser per key this phone holds, replacing whatever was running.
     *
     * Called at startup and after pairing, which adds a slot. Stopping the old
     * ones first matters: an advertiser left running under a revoked key would
     * keep telling a Mac the phone is there, using a key the Mac no longer has
     * -- harmless on the air, but it burns an advertising instance and the
     * phone would eventually run out of them.
     */
    private fun buildSlots() {
        val ids = PresenceKey.activeIds(this)
        if (ids.map { it } == slots.map { it.beacon.keyId }) return
        advertiser?.let { le -> slots.forEach { runCatching { le.stopAdvertising(it.callback) } } }
        slots = ids.map { Slot(PresenceBeacon(it), callbackFor(it)) }
        SpikeState.authentic = slots.isNotEmpty() && slots.all { it.beacon.authentic }
        SpikeState.fingerprint = PresenceKey.fingerprint(this)
        if (slots.isNotEmpty()) {
            SpikeState.event("广播用的钥匙编号：${ids.joinToString("、")}")
        }
    }

    /** Point the beacons at the slots this phone now holds. */
    private fun rebuildBeacon() {
        buildSlots()
        // Straight away, rather than at the next rotation: the window between
        // pairing and the first beacon is exactly when someone is standing at
        // the Mac waiting to see it work.
        handler.removeCallbacks(rotate)
        handler.post(rotate)
        if (slots.any { it.beacon.authentic } && macStateScanner == null) {
            macStateScanner = MacStateScanner(this).also { it.start() }
        }
    }

    override fun onDestroy() {
        handler.removeCallbacks(heartbeat)
        handler.removeCallbacks(rotate)
        advertiser?.let { le -> slots.forEach { runCatching { le.stopAdvertising(it.callback) } } }
        slots = emptyList()
        advertiser = null
        macStateScanner?.stop()
        macStateScanner = null
        SpikeState.serviceRunning = false
        SpikeState.advertising = false
        SpikeState.startedAtUptime = 0L
        SpikeState.event("service destroyed")
        Log.w(TAG, "service destroyed")
        super.onDestroy()
    }

    private fun buildNotification(): Notification =
        Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("${Brand.NAME} 手机钥匙")
            .setContentText(
                if (SpikeState.authentic) "正在让你的 Mac 认出这台手机" else "还没有配对，Mac 认不出这台手机",
            )
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .build()
}
