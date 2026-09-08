package ai.repose.mobile.unlock.companion

import android.Manifest
import android.annotation.TargetApi
import android.app.Activity
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.companion.AssociationInfo
import android.companion.AssociationRequest
import android.companion.BluetoothLeDeviceFilter
import android.companion.CompanionDeviceManager
import android.content.Intent
import android.content.IntentSender
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import android.os.Parcelable
import java.util.UUID

internal class AndroidCompanionAssociationDriver(
    private val activity: Activity,
    private val configureRuntimeAssociation: (
        Int,
        (AssociationConfiguration) -> Unit,
    ) -> Unit,
    private val mainHandler: Handler = Handler(Looper.getMainLooper()),
) : CompanionAssociationDriver {
    private val manager: CompanionDeviceManager?
        get() = activity.getSystemService(CompanionDeviceManager::class.java)

    override fun missingBluetoothPermissions(): List<String> = BluetoothPermissions.filter {
        activity.checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
    }

    override fun requestBluetoothPermissions(permissions: List<String>, requestCode: Int) {
        activity.requestPermissions(permissions.toTypedArray(), requestCode)
    }

    override fun discoverReposeMac(
        serviceUuid: String,
        events: AssociationDiscoveryEvents,
    ) {
        val bluetooth = activity.getSystemService(BluetoothManager::class.java)?.adapter
        if (bluetooth == null || !bluetooth.isEnabled) {
            events.onFailure("Bluetooth is unavailable.")
            return
        }
        val companionManager = manager
        if (companionManager == null) {
            events.onFailure("Companion device setup is unavailable.")
            return
        }

        val service = ParcelUuid(UUID.fromString(serviceUuid))
        val scanFilter = ScanFilter.Builder()
            .setServiceUuid(service)
            .build()
        val deviceFilter = BluetoothLeDeviceFilter.Builder()
            .setScanFilter(scanFilter)
            .build()
        val request = AssociationRequest.Builder()
            .addDeviceFilter(deviceFilter)
            .setSingleDevice(false)
            .build()
        val callback = callback(events)

        if (Build.VERSION.SDK_INT >= 33) {
            companionManager.associate(request, activity.mainExecutor, callback)
        } else {
            @Suppress("DEPRECATION")
            companionManager.associate(request, callback, mainHandler)
        }
    }

    override fun launchAssociationPrompt(prompt: AssociationPrompt, requestCode: Int) {
        val intentSender = (prompt as? AndroidAssociationPrompt)?.intentSender
            ?: throw IllegalArgumentException("Unknown association prompt type.")
        activity.startIntentSenderForResult(intentSender, requestCode, null, 0, 0, 0)
    }

    override fun configureAssociation(
        associationId: Int,
        completion: (AssociationConfiguration) -> Unit,
    ) {
        configureRuntimeAssociation(associationId) { configuration ->
            mainHandler.post { completion(configuration) }
        }
    }

    @Suppress("DEPRECATION")
    fun associationIdFromResult(data: Intent?): Int? {
        if (data == null) return null
        return try {
            if (Build.VERSION.SDK_INT >= 33) {
                Api33.associationIdFromResult(data)
            } else {
                val associatedDevice = data.getParcelableExtra<Parcelable>(
                    CompanionDeviceManager.EXTRA_DEVICE,
                )
                val address = when (associatedDevice) {
                    is ScanResult -> associatedDevice.device.address
                    is BluetoothDevice -> associatedDevice.address
                    else -> null
                }
                address?.let(::legacyAssociationToken)
            }
        } catch (_: RuntimeException) {
            null
        }
    }

    private fun callback(events: AssociationDiscoveryEvents): CompanionDeviceManager.Callback =
        if (Build.VERSION.SDK_INT >= 33) {
            Api33Callback(events)
        } else {
            LegacyCallback(events)
        }

    private open class LegacyCallback(
        protected val events: AssociationDiscoveryEvents,
    ) : CompanionDeviceManager.Callback() {
        @Suppress("OVERRIDE_DEPRECATION")
        override fun onDeviceFound(chooserLauncher: IntentSender) {
            events.onAssociationPending(AndroidAssociationPrompt(chooserLauncher))
        }

        override fun onFailure(errorMessage: CharSequence?) {
            events.onFailure(errorMessage?.toString())
        }
    }

    @TargetApi(33)
    private class Api33Callback(
        events: AssociationDiscoveryEvents,
    ) : LegacyCallback(events) {
        override fun onAssociationPending(chooserLauncher: IntentSender) {
            events.onAssociationPending(AndroidAssociationPrompt(chooserLauncher))
        }

        override fun onAssociationCreated(associationInfo: AssociationInfo) {
            events.onAssociationCreated(associationInfo.id)
        }

        override fun onFailure(errorCode: Int, errorMessage: CharSequence?) {
            events.onFailure(errorMessage?.toString())
        }
    }

    @TargetApi(33)
    private object Api33 {
        fun associationIdFromResult(data: Intent): Int? = data.getParcelableExtra(
            CompanionDeviceManager.EXTRA_ASSOCIATION,
            AssociationInfo::class.java,
        )?.id
    }

    private data class AndroidAssociationPrompt(
        val intentSender: IntentSender,
    ) : AssociationPrompt

    private companion object {
        val BluetoothPermissions = listOf(
            Manifest.permission.BLUETOOTH_SCAN,
            Manifest.permission.BLUETOOTH_CONNECT,
        )
    }
}
