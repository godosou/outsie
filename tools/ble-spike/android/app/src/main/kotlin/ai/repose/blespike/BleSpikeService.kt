package ai.repose.blespike

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
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
        /** How often, and how long, to ask an adapter that just came on for its advertiser. */
        const val ADVERTISER_RETRY_MS = 1_500L
        const val ADVERTISER_RETRIES = 6
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
         * Why a command cannot go out right now, as sentences a screen can
         * show as they are. Compare by identity to tell them apart: a screen
         * that knows a better next step for its own layout (the home screen,
         * where the switch is right there) can swap in its own wording.
         *
         * [REFUSED_KEY_OFF] is no longer what [canSend] returns for a key that
         * is switched off -- that is the [RadioPhase.OFF] sentence now, like
         * every other phase that is not on the air. It stays so a screen that
         * still compares against it compiles until it reads the phase itself.
         */
        const val REFUSED_NOT_PAIRED = "还没有配对。先在主屏添加电脑。"
        const val REFUSED_KEY_OFF = "手机钥匙关着。先去主屏打开。"

        /**
         * Whether a command posted now would actually reach the air, and if
         * not, why. Null means it would.
         *
         * No key is the first thing to say: an unauthenticated command is one
         * the Mac will refuse, and switching the key on would not change that.
         * After that the answer is the radio's own phase ([RadioFacts.phase]):
         * only [RadioPhase.onAir] carries a command. Off, Bluetooth off, still
         * starting, failed -- a byte queued in any of those is not「on its way」,
         * it is a surprise waiting for the next time the radio comes up, and
         * the phase's sentence already says what the person can do about it.
         * The design doc's 「按下之后」 row for 锁定 is explicit: 钥匙关着 / 没配对，
         * 当场说，不入队.
         *
         * `now` is [SystemClock.elapsedRealtime], the clock [RadioFacts.startingSince]
         * is written in (see [ensureRadio]).
         */
        fun canSend(context: Context): String? {
            // Any slot, not slot 1: from pair-v3 this phone's key lives wherever
            // it chose. Checking slot 1 made every command from a v3-paired
            // phone refuse itself before it was even sent.
            if (!PresenceKey.hasAny(context)) return REFUSED_NOT_PAIRED
            val radio = SpikeState.radio
            val phase = radio.phase(SystemClock.elapsedRealtime())
            return when {
                phase.onAir -> null
                // Off is the one phase whose sentence does not say what to do
                // -- the switch is right there, so this one points at it.
                phase == RadioPhase.OFF -> REFUSED_KEY_OFF
                else -> phase.sentence(radio.failure)
            }
        }

        /**
         * The set of keys changed under a running service: rebuild its beacons,
         * or stop it when nothing is left to advertise.
         *
         * Pairing already sends [ACTION_REBUILD_BEACON]; removal did not. So
         * 「移走」 deleted the key from the keystore and the service went on
         * advertising under it (logcat: `advertising started for keyId=17`
         * with key_ids already empty) -- a beacon nobody could verify, on a
         * phone whose screen said the Mac was gone. Not running: nothing to
         * do; the next start reads the keys fresh. `advertiseWanted` is left
         * alone: the person switched the key on, and pairing a new Mac should
         * find it still on.
         */
        fun keysChanged(context: Context) {
            if (!SpikeState.radio.running) return
            val intent = Intent(context, BleSpikeService::class.java)
            if (PresenceKey.hasAny(context)) {
                runCatching { context.startForegroundService(intent.setAction(ACTION_REBUILD_BEACON)) }
            } else {
                context.stopService(intent)
                SpikeState.event("没有钥匙了，停止广播")
            }
        }

        /**
         * Queue a command for the Mac. Returns false, and queues nothing, when
         * [canSend] says no -- a button that silently does nothing is worse
         * than one that says why, and a byte parked in a stopped service is
         * worse still: it would go out unasked the next time the key came on.
         * Screens should ask [canSend] first so the toast names the real reason.
         */
        fun postCommand(context: Context, cmd: Int): Boolean {
            canSend(context)?.let { reason ->
                Log.i(TAG, "command $cmd refused, not queued: $reason")
                return false
            }
            pendingSeq = AppStore(context).nextCommandSeq()
            pendingCmd = cmd
            pendingUntil = SystemClock.elapsedRealtime() + SpikeContract.COMMAND_BROADCAST_MS
            Log.i(TAG, "command $cmd queued, seq=$pendingSeq")
            SpikeState.event(
                when (cmd) {
                    SpikeContract.CMD_LOCK -> "已发出：锁定 Mac"
                    SpikeContract.CMD_CALIBRATE_NEAR -> "已发出：开始量近处"
                    SpikeContract.CMD_CALIBRATE_FAR -> "已发出：开始量远处"
                    in SpikeContract.CMD_SHORTCUT_BASE..255 -> "已发出：快捷操作 $cmd"
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
        /**
         * The system said this slot's advertising started, and nothing has
         * stopped or failed it since. The phone is on the air iff any slot is.
         */
        var live = false
    }

    private var slots: List<Slot> = emptyList()

    /**
     * Bluetooth going off and on under a running service.
     *
     * Before this existed the service noticed Bluetooth off only by crashing
     * on it, and noticed it back on never: the home screen said 「正在启动…」
     * until someone force-quit the app. Now off is a phase the screen can
     * name, and on is the radio coming back by itself -- which is what that
     * phase's sentence (「打开蓝牙，它自己接着广播」) promises.
     */
    private val bluetoothStateReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action != BluetoothAdapter.ACTION_STATE_CHANGED) return
            when (intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
                BluetoothAdapter.STATE_ON -> {
                    SpikeState.event("蓝牙开了，接着广播")
                    SpikeState.updateRadio { it.copy(bluetoothOn = true) }
                    ensureRadio()
                }
                // TURNING_OFF first, so the advertisers are stopped while the
                // adapter can still be asked; OFF again for good measure. The
                // second call finds nothing left to stop.
                BluetoothAdapter.STATE_TURNING_OFF, BluetoothAdapter.STATE_OFF -> {
                    if (SpikeState.radio.bluetoothOn) SpikeState.event("手机的蓝牙关了，广播停了")
                    teardownRadio()
                }
            }
        }
    }
    private var receiverRegistered = false

    private fun registerBluetoothReceiver() {
        if (receiverRegistered) return
        val filter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED)
        // A system broadcast still reaches a NOT_EXPORTED receiver; the flag
        // only keeps other apps from feeding it. Required from API 34 on, and
        // the flagged overload only exists from 33.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            // Exported on purpose: ACTION_STATE_CHANGED is a protected system
            // broadcast nobody else can send, and a NOT_EXPORTED receiver has
            // been seen to miss it on some stacks. Missing it is the whole bug.
            registerReceiver(bluetoothStateReceiver, filter, Context.RECEIVER_EXPORTED)
        } else {
            registerReceiver(bluetoothStateReceiver, filter)
        }
        receiverRegistered = true
    }

    private fun unregisterBluetoothReceiver() {
        if (!receiverRegistered) return
        receiverRegistered = false
        try {
            unregisterReceiver(bluetoothStateReceiver)
        } catch (e: IllegalArgumentException) {
            Log.w(TAG, "receiver was not registered: $e")
        }
    }

    private fun adapter(): BluetoothAdapter? =
        runCatching { getSystemService(BluetoothManager::class.java)?.adapter }.getOrNull()

    /** Bluetooth's own on/off, straight from the system; false if it cannot even be asked. */
    private fun bluetoothIsOn(): Boolean = runCatching { adapter()?.isEnabled == true }.getOrDefault(false)

    /**
     * Get the radio up if it can be, and record that it was asked.
     *
     * Safe to call any number of times, from anywhere in the service:
     * onCreate, every onStartCommand, Bluetooth coming back. If Bluetooth is
     * off this is [teardownRadio] instead, so those two are the only ways the
     * radio ever changes. `startingSince` is [SystemClock.elapsedRealtime]
     * (the uptime clock [MacState] already keeps), and a failure from before
     * this ask is cleared: the person, or Bluetooth, just gave the radio
     * another chance, and 「正在开始广播」 is true again until the callbacks
     * say otherwise.
     *
     * Failed slots get another go; slots already on the air are left alone,
     * because restarting a healthy advertiser costs the Mac a one-to-three
     * second hole for nothing.
     */
    private var advertiserRetries = 0
    private val retryRadio = Runnable { ensureRadio() }

    private fun ensureRadio() {
        if (!bluetoothIsOn()) {
            teardownRadio()
            return
        }
        val le = advertiser ?: runCatching { adapter()?.bluetoothLeAdvertiser }.getOrNull()
        if (le == null) {
            // Right after STATE_ON some stacks hand out no advertiser for a
            // second or two. Ask again a few times before calling the phone
            // unable; giving up at once is how a Bluetooth cycle ended in
            // 「广播没开起来」 with nothing retrying.
            if (advertiserRetries < ADVERTISER_RETRIES) {
                advertiserRetries++
                SpikeState.updateRadio { it.copy(bluetoothOn = true, startingSince = it.startingSince ?: SystemClock.elapsedRealtime()) }
                handler.removeCallbacks(retryRadio)
                handler.postDelayed(retryRadio, ADVERTISER_RETRY_MS)
                return
            }
            Log.e(TAG, "no LE advertiser (peripheral role unsupported?)")
            SpikeState.event("这部手机的蓝牙不支持广播，当不了钥匙")
            SpikeState.updateRadio {
                it.copy(bluetoothOn = true, advertising = false, failure = "这部手机的蓝牙不支持广播", startingSince = null)
            }
            return
        }
        advertiserRetries = 0
        advertiser = le
        val now = SystemClock.elapsedRealtime()
        SpikeState.updateRadio { it.copy(bluetoothOn = true, failure = null, startingSince = now) }
        slots.forEach { if (!it.live) it.advertisedCounter = Long.MIN_VALUE }
        // Only worth listening once there is a key: an unverifiable beacon
        // tells this phone nothing, and scanning for it would be battery spent
        // on a sentence that could never be shown.
        if (slots.any { it.beacon.authentic }) startScanner()
        // Straight away, not at the next check: whoever asked is watching.
        handler.removeCallbacks(rotate)
        handler.post(rotate)
    }

    /**
     * Take the radio down and say so in the facts. Idempotent: a second call
     * finds nothing to stop. Reached with Bluetooth off or going off, from
     * [ensureRadio] when it finds the adapter off, and from onDestroy. The
     * rotation stops here and only [ensureRadio] posts it again.
     */
    private fun teardownRadio() {
        handler.removeCallbacks(rotate)
        advertiser?.let { le -> slots.forEach { stopSlot(le, it) } }
        advertiser = null
        slots.forEach {
            it.live = false
            it.advertisedCounter = Long.MIN_VALUE
            it.advertisedCmd = SpikeContract.CMD_NONE
        }
        stopScanner()
        SpikeState.updateRadio { it.copy(bluetoothOn = false, advertising = false, startingSince = null) }
    }

    private fun startScanner() {
        try {
            val scanner = macStateScanner ?: MacStateScanner(this).also { macStateScanner = it }
            // Its start() is a no-op when already listening, and retries one
            // that could not happen before (no permission yet, no scanner).
            scanner.start()
        } catch (e: RuntimeException) {
            Log.w(TAG, "could not start the Mac-state scanner", e)
        }
    }

    private fun stopScanner() {
        val scanner = macStateScanner ?: return
        macStateScanner = null
        try {
            scanner.stop()
        } catch (e: RuntimeException) {
            Log.w(TAG, "could not stop the Mac-state scanner", e)
        }
    }

    private val heartbeat = object : Runnable {
        override fun run() {
            Log.i(TAG, "heartbeat adv=${SpikeState.advertising} auth=${SpikeState.authentic}")
            // Belt and braces for the receiver: if Bluetooth is on and the radio is
            // not, bring it back. A missed broadcast must cost a minute, not a day.
            if (advertiser == null && bluetoothIsOn() && SpikeState.radio.running) ensureRadio()
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
            // The one deliberate early return: no advertiser means the radio
            // was torn down (Bluetooth off, or the service stopping), and a
            // loop that kept asking a dead adapter is exactly what killed the
            // process on 2026-09-12. ensureRadio() posts this again when the
            // radio is back.
            if (advertiser == null) return
            // A scan registration can fail right after the adapter comes up;
            // nothing else would retry it until the next ensureRadio(), and a
            // phone that advertises but never listens shows every Mac as
            // 「没听到它」. Cheap: start() is a no-op while listening.
            if (slots.any { it.beacon.authentic } && macStateScanner?.isScanning != true) startScanner()
            // NOT an early return below: `return` there would leave run()
            // without rescheduling, so one moment with no slots -- between a
            // revoke and a pairing, say -- would stop the rotation permanently
            // and the phone would go quiet until the service was restarted.
            val first = slots.firstOrNull()
            val c = first?.beacon?.currentCounter() ?: 0L
            // A command has to go out now, not at the next 30s window boundary,
            // and it has to stop going out when it expires. Both are changes to
            // what the payload should say, so both force a refresh.
            val cmdNow = liveCommand()
            // Any slot out of date refreshes them all: they share a window and
            // a command, so they are never legitimately out of step.
            if (first != null &&
                slots.any { it.advertisedCounter != c || it.advertisedCmd != cmdNow }
            ) {
                refreshBeacon(c)
            }
            // Poll faster while something is pending, so the press-to-air delay
            // is not itself mistaken for the radio being slow.
            val pending = pendingCmd != SpikeContract.CMD_NONE &&
                SystemClock.elapsedRealtime() < pendingUntil
            // The refresh may have found the adapter gone and torn the radio
            // down; then this loop ends here too.
            if (advertiser == null) return
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
     *
     * These two callbacks are where `advertising` is decided. Asking the radio
     * to start is not being on the air: the fact becomes true only when the
     * system says so, and until then the screen honestly shows 「正在开始广播」.
     * Both arrive on the main thread, like everything else in here.
     */
    private fun callbackFor(keyId: Int) = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings) {
            slotFor(keyId)?.live = true
            SpikeState.beaconsSent++
            SpikeState.updateRadio { it.copy(advertising = true, failure = null) }
            SpikeState.event(
                "钥匙 $keyId 的信标上天了，窗口 ${SpikeState.beaconCounter}" +
                    if (SpikeState.authentic) "" else "（还没配对 —— 这个 tag 没有意义）",
            )
            Log.i(TAG, "advertising started for keyId=$keyId: $settingsInEffect")
        }

        override fun onStartFailure(errorCode: Int) {
            // Not live, and forgotten as advertised, so the next rotate tick
            // tries again instead of parking in FAILED for a whole window.
            slotFor(keyId)?.let { it.live = false; it.advertisedCounter = Long.MIN_VALUE }
            val why = advertiseFailureWords(errorCode)
            // The code goes to logcat; the person gets words.
            Log.e(TAG, "advertising failed for keyId=$keyId, code=$errorCode")
            SpikeState.event("钥匙 $keyId 的信标发不出去：$why")
            noteRadioFailure(why)
        }
    }

    private fun slotFor(keyId: Int): Slot? = slots.firstOrNull { it.beacon.keyId == keyId }

    /**
     * A slot just failed. Not a global failure while another slot is fine:
     * with two Macs paired and one advertiser refusing, the phone IS being
     * seen, and saying otherwise would be the screen reporting something that
     * is not true. Only when no slot is on the air does the failure reach it.
     */
    private fun noteRadioFailure(why: String) {
        if (slots.any { it.live }) return
        SpikeState.updateRadio { it.copy(advertising = false, failure = why) }
    }

    /**
     * [AdvertiseCallback] error codes, in the person's words. The phrase lands
     * inside 「广播没开起来：…。关掉再开一次。」, so: short, no full stop, no
     * number. The number is in logcat for whoever is debugging.
     */
    private fun advertiseFailureWords(errorCode: Int): String = when (errorCode) {
        AdvertiseCallback.ADVERTISE_FAILED_DATA_TOO_LARGE -> "要广播的内容太长"
        AdvertiseCallback.ADVERTISE_FAILED_TOO_MANY_ADVERTISERS -> "手机上在广播的东西太多"
        AdvertiseCallback.ADVERTISE_FAILED_ALREADY_STARTED -> "上一次广播还没停下"
        AdvertiseCallback.ADVERTISE_FAILED_FEATURE_UNSUPPORTED -> "这部手机的蓝牙不支持广播"
        else -> "手机的蓝牙出了点问题"
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "BLE spike", NotificationManager.IMPORTANCE_LOW),
        )
        startForeground(NOTIFICATION_ID, buildNotification())
        SpikeState.addListener(refreshNotification)
        SpikeState.startedAtUptime = SystemClock.elapsedRealtime()
        // One fresh set of facts: whatever the last run left behind is not
        // this run's radio.
        SpikeState.updateRadio {
            RadioFacts(
                running = true,
                hasKeys = PresenceKey.hasAny(this),
                bluetoothOn = bluetoothIsOn(),
                advertising = false,
                failure = null,
                startingSince = null,
            )
        }
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

        // Bluetooth off is not a reason to give up here -- it is a phase the
        // screen names, and the receiver is what makes that sentence true.
        // Registered before ensureRadio() so a switch flipped in between is
        // not missed.
        registerBluetoothReceiver()
        ensureRadio()
        handler.postDelayed(heartbeat, HEARTBEAT_MS)
    }

    /** Re-mint and re-advertise every slot. */
    private fun refreshBeacon(counter: Long) {
        val le = advertiser ?: return
        val cmd = liveCommand()
        for (slot in slots) {
            refreshSlot(le, slot, counter, cmd)
            // A slot that found the adapter gone tore the radio down; the
            // rest would only find the same thing.
            if (advertiser == null) return
        }
        SpikeState.beaconCounter = counter
        SpikeState.authentic = slots.all { it.beacon.authentic } && slots.isNotEmpty()
    }

    /**
     * Stop and restart one slot's advertiser with the payload for this window.
     * Returns false if the radio was not asked.
     *
     * Whether the phone is on the air is NOT decided here: `startAdvertising`
     * returning is the request being accepted, and the answer comes back on
     * the slot's callback. Deciding it here is how the screen once said 「开着」
     * over a radio that had refused.
     *
     * Every call into the adapter is guarded. On 2026-09-12 the person
     * switched Bluetooth off while this ran; `startAdvertising` on the dead
     * adapter threw, nothing caught it, and the process died -- taking the
     * key, the Mac-state scanner and any chance of noticing Bluetooth coming
     * back with it. A radio that cannot be asked is a phase, not a crash.
     */
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
            stopSlot(le, slot)
            SpikeState.event("钥匙 ${slot.beacon.keyId} 签不出 tag（${e.message}），这一路停了")
            Log.e(TAG, "beacon mint failed for keyId=${slot.beacon.keyId}", e)
            noteRadioFailure("手机里的钥匙用不了")
            return false
        }

        stopSlot(le, slot)
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
        try {
            le.startAdvertising(settings, data, slot.callback)
        } catch (e: RuntimeException) {
            // IllegalStateException when the adapter is off, SecurityException
            // without BLUETOOTH_ADVERTISE, IllegalArgumentException for a
            // payload the stack will not take. None of them may end the process.
            Log.e(TAG, "startAdvertising threw for keyId=${slot.beacon.keyId}", e)
            if (!bluetoothIsOn()) {
                // Not a failure: Bluetooth went off under us and the broadcast
                // has not arrived yet. Tear down now; the receiver brings the
                // radio back when Bluetooth does.
                SpikeState.event("手机的蓝牙关了，广播停了")
                teardownRadio()
            } else {
                val why = if (e is SecurityException) "没拿到蓝牙权限" else "手机的蓝牙出了点问题"
                SpikeState.event("钥匙 ${slot.beacon.keyId} 的信标发不出去：$why")
                noteRadioFailure(why)
            }
            return false
        }

        slot.advertisedCounter = counter
        slot.advertisedCmd = cmd
        return true
    }

    /** Stop one slot's advertiser; the slot is off the air whether or not the adapter agreed. */
    private fun stopSlot(le: BluetoothLeAdvertiser, slot: Slot) {
        try {
            le.stopAdvertising(slot.callback)
        } catch (e: RuntimeException) {
            Log.w(TAG, "stopAdvertising threw for keyId=${slot.beacon.keyId}: $e")
        }
        slot.live = false
        slot.advertisedCounter = Long.MIN_VALUE
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Pairing writes a key into a NEW slot, and the beacon object was built
        // in onCreate against whichever slot existed then. Android delivers this
        // to onStartCommand, not onCreate, for a service that is already
        // running -- so without this, a phone that had just paired went on
        // advertising under its previous key id, and the Mac it had just been
        // introduced to heard nothing it could verify.
        //
        // A start with no action -- the home switch, the boot receiver, the
        // sticky restart's null intent -- takes the same path. Whoever sent it
        // wants the key on the air. If the radio is already there this costs
        // nothing; if it is not (a failed start, an adapter that went away),
        // it is the recovery the person just asked for by tapping the switch.
        rebuildBeacon()
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
        // The fact the screen keys on first, whether or not the slots change.
        SpikeState.updateRadio { it.copy(hasKeys = ids.isNotEmpty()) }
        if (ids == slots.map { it.beacon.keyId }) return
        advertiser?.let { le -> slots.forEach { stopSlot(le, it) } }
        slots = ids.map { Slot(PresenceBeacon(it), callbackFor(it)) }
        // Nothing is on the air until the new slots' callbacks say so.
        SpikeState.updateRadio { it.copy(advertising = false) }
        SpikeState.authentic = slots.isNotEmpty() && slots.all { it.beacon.authentic }
        SpikeState.fingerprint = PresenceKey.fingerprint(this)
        if (slots.isNotEmpty()) {
            SpikeState.event("广播用的钥匙编号：${ids.joinToString("、")}")
        }
    }

    /** Point the beacons at the slots this phone now holds, and make sure the radio is up. */
    private fun rebuildBeacon() {
        buildSlots()
        // ensureRadio() posts the rotation straight away, rather than at the
        // next check: the window between pairing and the first beacon is
        // exactly when someone is standing at the Mac waiting to see it work.
        ensureRadio()
    }

    override fun onDestroy() {
        SpikeState.removeListener(refreshNotification)
        handler.removeCallbacks(heartbeat)
        unregisterBluetoothReceiver()
        teardownRadio()
        slots = emptyList()
        // Off is off whatever Bluetooth is doing; the adapter's answer is kept
        // only so the facts do not claim Bluetooth went away when it did not.
        SpikeState.updateRadio {
            it.copy(running = false, bluetoothOn = bluetoothIsOn(), advertising = false, failure = null, startingSince = null)
        }
        SpikeState.startedAtUptime = 0L
        SpikeState.event("service destroyed")
        Log.w(TAG, "service destroyed")
        super.onDestroy()
    }

    /** The same sentence the home screen shows, so the shade never says something the app does not. */
    private val refreshNotification: () -> Unit = {
        runCatching { getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification()) }
    }

    private fun buildNotification(): Notification =
        Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("${Brand.NAME} 手机钥匙")
            .setContentText(SpikeState.radio.let { it.phase(SystemClock.elapsedRealtime()).sentence(it.failure) })
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .build()
}
