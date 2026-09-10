package ai.repose.blespike

import android.os.SystemClock
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Process-wide observable state so the Activity can show what the service is doing.
 * Deliberately dumb: a few counters plus a rolling event log.
 */
object SpikeState {

    private const val LOG_LIMIT = 14
    private val clock = SimpleDateFormat("HH:mm:ss", Locale.US)
    private val listeners = mutableListOf<() -> Unit>()
    private val log = ArrayDeque<String>()

    @Volatile var serviceRunning = false
    @Volatile var advertising = false
    @Volatile var startedAtUptime = 0L

    /** True only when the tag is minted from a provisioned key. See [PresenceBeacon]. */
    @Volatile var authentic = false
    @Volatile var fingerprint: String? = null
    @Volatile var beaconCounter = 0L
    @Volatile var beaconsSent = 0

    @Synchronized fun addListener(listener: () -> Unit) { listeners += listener }

    @Synchronized fun removeListener(listener: () -> Unit) { listeners -= listener }

    @Synchronized
    fun event(message: String) {
        log.addFirst("${clock.format(Date())}  $message")
        while (log.size > LOG_LIMIT) log.removeLast()
        notifyListeners()
    }

    @Synchronized
    fun notifyListeners() {
        listeners.toList().forEach { it() }
    }

    @Synchronized
    fun render(): String {
        val s = if (startedAtUptime == 0L) 0 else (SystemClock.elapsedRealtime() - startedAtUptime) / 1000
        val uptime = "${s / 3600}h ${(s % 3600) / 60}m ${s % 60}s"
        return """
            service:      ${if (serviceRunning) "RUNNING" else "stopped"}
            advertising:  ${if (advertising) "YES" else "no"}
            presence key: ${if (authentic) "provisioned ${fingerprint ?: "?"}" else "NONE — tag is worthless"}
            window:       $beaconCounter ($beaconsSent beacons sent)
            uptime:       $uptime

            service uuid: ${SpikeContract.PRESENCE_SERVICE_UUID}  (16-bit FFF0)
            payload:      version(1) ‖ keyId(1) ‖ HMAC tag(${SpikeContract.TAG_LEN})
            window size:  ${SpikeContract.WINDOW_SECONDS}s

            --- events (newest first) ---
        """.trimIndent() + "\n" + log.joinToString("\n")
    }
}
