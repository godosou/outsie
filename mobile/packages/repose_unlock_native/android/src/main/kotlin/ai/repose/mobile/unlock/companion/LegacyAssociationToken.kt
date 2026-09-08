package ai.repose.mobile.unlock.companion

import java.util.Locale

internal fun canonicalBluetoothAddress(value: String?): String? {
    val address = value?.uppercase(Locale.ROOT) ?: return null
    return address.takeIf { BLUETOOTH_ADDRESS.matches(it) }
}

internal fun legacyAssociationToken(address: String): Int? {
    val canonical = canonicalBluetoothAddress(address) ?: return null
    var hash = 17
    canonical.forEach { character -> hash = 31 * hash + character.code }
    return -((hash and 0x3fff_ffff) + 2)
}

private val BLUETOOTH_ADDRESS = Regex("(?:[0-9A-F]{2}:){5}[0-9A-F]{2}")
