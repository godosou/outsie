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

    /**
     * The human said the digits match. On success the window is OVER.
     *
     * It used not to close. PairingServer.stop() keeps the session when the
     * stage is Done -- so that the stage can still be read -- and this left
     * `server` set as well, so `isOpen` stayed true and the screen rebuilt
     * straight back into the digits, with the same two buttons. The toast said
     * 配对完成 and nothing visibly changed, so the natural thing was to press
     * again: the second press found a finished session, could derive nothing,
     * and reported 配对没有完成 for a pairing that had in fact worked on the
     * first press.
     *
     * A screen that does not change after a successful action is an invitation
     * to repeat it, and this one punished that with a false failure.
     */
    fun confirm(): Boolean {
        val ok = server?.confirmMatch() ?: false
        if (ok) stop()
        return ok
    }
    fun reject() { server?.reject() }

    fun stop() {
        server?.stop()
        server = null
    }
}
