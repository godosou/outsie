package ai.repose.mobile.unlock.pairing

import android.annotation.SuppressLint
import android.annotation.TargetApi
import android.companion.AssociationInfo
import android.companion.CompanionDeviceManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import ai.repose.mobile.unlock.NativeRuntimeAvailability
import ai.repose.mobile.unlock.companion.canonicalBluetoothAddress
import ai.repose.mobile.unlock.companion.legacyAssociationToken

/** Foreground-only association lookup for the debug pairing client on API 31+. */
@TargetApi(31)
internal class AndroidDebugCompanionAssociationProvider(context: Context) {
    private val applicationContext = context.applicationContext
    private val preferences = applicationContext.getSharedPreferences(
        PREFERENCES_NAME,
        Context.MODE_PRIVATE,
    )

    @Synchronized
    fun currentAssociationToken(): Int? = selectedAssociation()?.token()

    @Synchronized
    fun availability(): NativeRuntimeAvailability {
        if (!applicationContext.packageManager.hasSystemFeature(
                PackageManager.FEATURE_COMPANION_DEVICE_SETUP,
            )
        ) {
            return NativeRuntimeAvailability.COMPANION_FEATURE_UNAVAILABLE
        }
        return if (selectedAssociation() == null) {
            NativeRuntimeAvailability.ASSOCIATION_NOT_CONFIGURED
        } else {
            NativeRuntimeAvailability.TRANSPORT_NOT_IMPLEMENTED
        }
    }

    @Synchronized
    fun configureAssociation(token: Int): Boolean {
        val candidate = associations().singleOrNull { it.token() == token } ?: return false
        val editor = preferences.edit().clear()
        candidate.associationId?.let { editor.putInt(PREFERRED_ASSOCIATION_ID, it) }
        candidate.deviceAddress?.let { editor.putString(PREFERRED_DEVICE_ADDRESS, it) }
        return editor.commit()
    }

    private fun selectedAssociation(): DebugCompanionAssociation? {
        if (!applicationContext.packageManager.hasSystemFeature(
                PackageManager.FEATURE_COMPANION_DEVICE_SETUP,
            )
        ) {
            return null
        }
        return DebugAssociationSelector.select(associations(), preferredAssociation())
    }

    private fun preferredAssociation(): DebugCompanionAssociation? {
        val associationId = if (preferences.contains(PREFERRED_ASSOCIATION_ID)) {
            preferences.getInt(PREFERRED_ASSOCIATION_ID, -1)
        } else {
            null
        }
        val address = canonicalBluetoothAddress(
            preferences.getString(PREFERRED_DEVICE_ADDRESS, null),
        )
        return if (associationId == null && address == null) {
            null
        } else {
            DebugCompanionAssociation(associationId, address)
        }
    }

    @Suppress("DEPRECATION")
    private fun associations(): List<DebugCompanionAssociation> {
        val manager = applicationContext.getSystemService(CompanionDeviceManager::class.java)
            ?: return emptyList()
        return try {
            if (Build.VERSION.SDK_INT >= 33) {
                Api33DebugAssociations.read(manager)
            } else {
                manager.associations.map { address ->
                    DebugCompanionAssociation(
                        associationId = null,
                        deviceAddress = canonicalBluetoothAddress(address),
                    )
                }
            }
        } catch (_: RuntimeException) {
            emptyList()
        }
    }

    private fun DebugCompanionAssociation.token(): Int? = associationId
        ?: deviceAddress?.let(::legacyAssociationToken)

    private companion object {
        const val PREFERENCES_NAME = "repose_debug_companion_association_v1"
        const val PREFERRED_ASSOCIATION_ID = "association_id"
        const val PREFERRED_DEVICE_ADDRESS = "device_address"
    }
}

@TargetApi(33)
private object Api33DebugAssociations {
    fun read(manager: CompanionDeviceManager): List<DebugCompanionAssociation> =
        manager.myAssociations.map { association ->
            DebugCompanionAssociation(
                associationId = association.id,
                deviceAddress = association.deviceAddressOrNull(),
            )
        }

    @SuppressLint("MissingPermission")
    private fun AssociationInfo.deviceAddressOrNull(): String? {
        val associatedBleAddress = if (Build.VERSION.SDK_INT >= 34) {
            try {
                canonicalBluetoothAddress(associatedDevice?.bleDevice?.device?.address)
            } catch (_: RuntimeException) {
                null
            }
        } else {
            null
        }
        if (associatedBleAddress != null) return associatedBleAddress

        val persistedAddress = try {
            canonicalBluetoothAddress(deviceMacAddress?.toString())
        } catch (_: RuntimeException) {
            null
        }
        if (persistedAddress != null) return persistedAddress

        return if (Build.VERSION.SDK_INT >= 34) {
            try {
                canonicalBluetoothAddress(associatedDevice?.bluetoothDevice?.address)
            } catch (_: RuntimeException) {
                null
            }
        } else {
            null
        }
    }
}
