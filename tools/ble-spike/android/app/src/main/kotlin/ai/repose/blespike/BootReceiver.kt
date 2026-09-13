package ai.repose.blespike

import android.Manifest
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.util.Log

/**
 * Put the key back after a restart.
 *
 * Without this, rebooting the phone retires it as a key and says nothing: the
 * Mac goes back to asking for a password, which is safe, correct, and the exact
 * shape of a failure nobody notices for a week. The person is left thinking the
 * feature is unreliable rather than that their phone restarted.
 *
 * Only if the user had it on, and only if the permissions are still granted.
 * Starting a foreground service at boot for someone who switched the feature
 * off would be the app deciding for them; asking for permissions from a
 * broadcast receiver is not possible anyway, and failing quietly here is right
 * because the app's own screen will show the toggle as off, which is true.
 */
class BootReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED &&
            intent.action != Intent.ACTION_MY_PACKAGE_REPLACED
        ) {
            return
        }
        if (!AppStore(context).advertiseWanted) return

        val needed = listOf(
            Manifest.permission.BLUETOOTH_ADVERTISE,
            Manifest.permission.BLUETOOTH_CONNECT,
        )
        if (needed.any { context.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }) {
            Log.i(TAG, "was on before the restart, but a permission is gone; staying off")
            return
        }

        Log.i(TAG, "restarting the beacon after ${intent.action}")
        context.startForegroundService(Intent(context, BleSpikeService::class.java))
    }

    private companion object {
        const val TAG = "ReposeBoot"
    }
}
