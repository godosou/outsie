package ai.repose.blespike

import android.content.Context

/**
 * The one live pairing window, if any.
 *
 * A singleton because there must never be two: two windows means two sets of
 * ephemerals and two sets of six digits, and a human comparing the wrong pair
 * would confirm a session they never saw. `start` closes any previous window
 * before opening one.
 */
object Pairing {
    private var server: PairingServer? = null

    val digits: String? get() = server?.digits
    val stage: PairingSession.Stage? get() = server?.stage
    val lastError: String? get() = server?.lastError
    val isOpen: Boolean get() = server != null && stage != null

    fun start(context: Context, onChange: () -> Unit): Boolean {
        stop()
        // Application context for the server (it outlives a screen), but the
        // permission request needs the Activity.
        (context as? android.app.Activity)?.let { act ->
            val missing = listOf(
                android.Manifest.permission.BLUETOOTH_CONNECT,
                android.Manifest.permission.BLUETOOTH_ADVERTISE,
            ).filter {
                act.checkSelfPermission(it) != android.content.pm.PackageManager.PERMISSION_GRANTED
            }
            if (missing.isNotEmpty()) {
                act.requestPermissions(missing.toTypedArray(), 8)
            }
        }
        val s = PairingServer(context.applicationContext, onChange)
        server = s
        if (!runCatching { s.start() }.getOrDefault(false)) {
            // Keep the instance so the screen can read lastError; a null server
            // would render as "never tried" and lose the reason.
            return false
        }
        return true
    }

    fun confirm(): Boolean = server?.confirmMatch() ?: false
    fun reject() { server?.reject() }

    fun stop() {
        server?.stop()
        server = null
    }
}
