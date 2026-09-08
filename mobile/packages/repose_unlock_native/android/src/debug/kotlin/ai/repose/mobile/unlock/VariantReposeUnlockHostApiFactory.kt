package ai.repose.mobile.unlock

import android.Manifest
import android.bluetooth.BluetoothManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings
import ai.repose.mobile.unlock.companion.AssociationConfiguration
import ai.repose.mobile.unlock.pairing.AndroidDebugCompanionAssociationProvider
import ai.repose.mobile.unlock.pairing.AndroidDebugGattPairingTransport
import ai.repose.mobile.unlock.pairing.DebugPairingCoordinator
import ai.repose.mobile.unlock.pairing.DebugPhoneIdentity
import java.nio.charset.StandardCharsets
import java.security.MessageDigest

internal object VariantReposeUnlockHostApiFactory {
    fun create(
        context: Context,
        requestCompanionAssociation: ((Result<Unit>) -> Unit) -> Unit,
    ): LifecycleReposeUnlockHostApi {
        val applicationContext = context.applicationContext
        val associationProvider = AndroidDebugCompanionAssociationProvider(applicationContext)
        return DebugReposeUnlockHostApi(
            coordinator = DebugPairingCoordinator(
                transport = AndroidDebugGattPairingTransport(applicationContext),
                nowEpochMillis = { System.currentTimeMillis().coerceAtLeast(0).toULong() },
                associationId = associationProvider::currentAssociationToken,
                phoneIdentity = debugPhoneIdentity(applicationContext),
            ),
            requestCompanionAssociation = requestCompanionAssociation,
            availability = associationProvider::availability,
            bluetoothReady = { bluetoothReady(applicationContext) },
        )
    }

    fun configureAssociation(
        context: Context,
        associationId: Int,
        completion: (AssociationConfiguration) -> Unit,
    ) {
        val configured = AndroidDebugCompanionAssociationProvider(context)
            .configureAssociation(associationId)
        completion(
            if (configured) {
                AssociationConfiguration.CONFIGURED
            } else {
                AssociationConfiguration.FAILED
            },
        )
    }

    private fun bluetoothReady(context: Context): Boolean =
        context.checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) ==
            PackageManager.PERMISSION_GRANTED &&
            context.getSystemService(BluetoothManager::class.java)?.adapter?.isEnabled == true

    private fun debugPhoneIdentity(context: Context): DebugPhoneIdentity {
        val androidId = Settings.Secure.getString(
            context.contentResolver,
            Settings.Secure.ANDROID_ID,
        ) ?: Build.FINGERPRINT
        val digest = MessageDigest.getInstance("SHA-256").run {
            update("repose-debug-phone-id-v1\u0000".toByteArray(StandardCharsets.US_ASCII))
            digest(androidId.toByteArray(StandardCharsets.UTF_8))
        }
        val deviceId = "android_" + digest.copyOfRange(0, 16).toLowerHex()
        val requestedName = listOf(Build.MANUFACTURER, Build.MODEL)
            .joinToString(" ")
            .replace(Regex("[|\\p{Cc}\\p{Cf}]+"), " ")
            .trim()
            .ifEmpty { "Android phone" }
        return DebugPhoneIdentity(deviceId, requestedName.utf8Prefix(80))
    }
}

private fun ByteArray.toLowerHex(): String = joinToString(separator = "") { byte ->
    "%02x".format(byte.toInt() and 0xff)
}

private fun String.utf8Prefix(maxBytes: Int): String {
    val result = StringBuilder()
    var index = 0
    var size = 0
    while (index < length) {
        val codePoint = codePointAt(index)
        val text = String(Character.toChars(codePoint))
        val encodedSize = text.toByteArray(StandardCharsets.UTF_8).size
        if (size + encodedSize > maxBytes) break
        result.append(text)
        size += encodedSize
        index += Character.charCount(codePoint)
    }
    return result.toString().ifEmpty { "Android phone" }
}
