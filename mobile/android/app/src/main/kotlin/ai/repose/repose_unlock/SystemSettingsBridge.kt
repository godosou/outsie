package ai.repose.repose_unlock

import android.Manifest
import android.app.Activity
import android.app.ActivityManager
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel

/** Settings UX only. This does not start BLE, associate a device or enable unlock. */
internal class SystemSettingsBridge(private val activity: Activity, messenger: BinaryMessenger) {
    private val channel = MethodChannel(messenger, "ai.repose/system_settings")
    private val preferences = activity.getSharedPreferences("repose_permission_prompts", Context.MODE_PRIVATE)
    private var pending: MethodChannel.Result? = null
    private val nearbyPermissions = listOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT)
    private val notificationPermissions = if (Build.VERSION.SDK_INT >= 33) listOf(Manifest.permission.POST_NOTIFICATIONS) else emptyList()

    init {
        channel.setMethodCallHandler { call, result ->
            try {
                when (call.method) {
                    "getStatus" -> result.success(snapshot())
                    "openAppSettings" -> {
                        activity.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                            Uri.parse("package:${activity.packageName}")))
                        result.success(true)
                    }
                    "requestNearby" -> request(nearbyPermissions, result)
                    "requestNotifications" -> request(notificationPermissions, result)
                    else -> result.notImplemented()
                }
            } catch (_: Exception) {
                if (pending === result) pending = null
                result.error("settingsUnavailable", "Open this app's system settings manually.", null)
            }
        }
    }

    @Suppress("DEPRECATION")
    private fun declared(): Set<String> = activity.packageManager
        .getPackageInfo(activity.packageName, PackageManager.GET_PERMISSIONS)
        .requestedPermissions?.toSet().orEmpty()

    private fun access(permissions: List<String>): String {
        val required = permissions.filter { it in declared() }
        if (required.isEmpty()) return "notRequired"
        val denied = required.filter { activity.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        return permissionStatus(
            declared = true,
            granted = denied.isEmpty(),
            requested = denied.any { preferences.getBoolean(it, false) && !activity.shouldShowRequestPermissionRationale(it) },
            rationale = denied.all { activity.shouldShowRequestPermissionRationale(it) },
        )
    }

    private fun snapshot(): Map<String, Any> {
        val power = activity.getSystemService(PowerManager::class.java)
        val manager = activity.getSystemService(ActivityManager::class.java)
        val background = when {
            manager?.isBackgroundRestricted == true -> "restricted"
            power == null || manager == null -> "unknown"
            power.isIgnoringBatteryOptimizations(activity.packageName) -> "unrestricted"
            else -> "optimized"
        }
        val notificationAccess = access(notificationPermissions)
        return mapOf(
            "platform" to "android",
            "manufacturer" to Build.MANUFACTURER,
            "nearby" to access(nearbyPermissions),
            "notifications" to if (notificationAccess == "granted" &&
                activity.getSystemService(NotificationManager::class.java)?.areNotificationsEnabled() == false)
                "permanentlyDenied" else notificationAccess,
            "background" to background,
            "powerSaver" to (power?.isPowerSaveMode == true),
        )
    }

    private fun request(permissions: List<String>, result: MethodChannel.Result) {
        if (pending != null) { result.error("busy", "A permission prompt is already open.", null); return }
        // Only ask for permissions the installed feature actually declares.
        val required = permissions.filter { it in declared() &&
            activity.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (required.isEmpty()) { result.success(true); return }
        if (access(permissions) == "permanentlyDenied") {
            result.error("settingsRequired", "Allow this permission in system settings.", null)
            return
        }
        pending = result
        activity.requestPermissions(required.toTypedArray(), REQUEST_CODE)
        preferences.edit().apply { required.forEach { putBoolean(it, true) } }.apply()
    }

    fun onPermissionResult(requestCode: Int) {
        if (requestCode != REQUEST_CODE) return
        // Completion is not a grant. Flutter always re-reads the OS state.
        val result = pending
        pending = null
        result?.success(true)
    }

    fun close() {
        channel.setMethodCallHandler(null)
        pending?.error("cancelled", "The permission screen closed.", null)
        pending = null
    }

    companion object { private const val REQUEST_CODE = 4901 }
}
