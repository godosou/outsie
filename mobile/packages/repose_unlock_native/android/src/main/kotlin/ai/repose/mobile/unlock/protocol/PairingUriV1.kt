package ai.repose.mobile.unlock.protocol

import java.net.URI
import java.util.Base64

internal class PairingUriFormatException :
    IllegalArgumentException("non-canonical Repose pairing URI v1")

internal object PairingUriV1 {
    fun decode(input: String): PairingPayloadV1 {
        if (input.length !in MINIMUM_URI_LENGTH..MAXIMUM_URI_LENGTH) {
            throw PairingUriFormatException()
        }
        val uri = try {
            URI(input)
        } catch (_: Exception) {
            throw PairingUriFormatException()
        }
        if (
            uri.scheme != SCHEME ||
            uri.rawAuthority != AUTHORITY ||
            uri.rawQuery != null ||
            uri.rawFragment != null ||
            uri.rawUserInfo != null ||
            uri.port != -1
        ) {
            throw PairingUriFormatException()
        }
        val encoded = PATH.matchEntire(uri.rawPath ?: "")?.groupValues?.get(1)
            ?: throw PairingUriFormatException()
        val decoded = try {
            Base64.getUrlDecoder().decode(encoded)
        } catch (_: IllegalArgumentException) {
            throw PairingUriFormatException()
        }
        if (Base64.getUrlEncoder().withoutPadding().encodeToString(decoded) != encoded) {
            throw PairingUriFormatException()
        }
        return try {
            PairingProtocolV1.decode(decoded)
        } catch (_: PairingProtocolFormatException) {
            throw PairingUriFormatException()
        }
    }

    private const val SCHEME = "repose"
    private const val AUTHORITY = "pair"
    private const val MINIMUM_URI_LENGTH = 32
    private const val MAXIMUM_URI_LENGTH = 4096
    private val PATH = Regex("^/v1/([A-Za-z0-9_-]+)$")
}
