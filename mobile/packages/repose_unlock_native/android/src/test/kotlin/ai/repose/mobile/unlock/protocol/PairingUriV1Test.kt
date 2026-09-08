package ai.repose.mobile.unlock.protocol

import java.nio.file.Path
import java.util.Base64
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class PairingUriV1Test {
    @Test
    fun `canonical URI decodes the exact RPPK v1 bytes`() {
        val frame = fixture()
        val uri = canonicalUri(frame)

        val decoded = PairingUriV1.decode(uri)

        assertArrayEquals(frame, decoded.encoded())
    }

    @Test
    fun `URI rejects noncanonical scheme authority path query and fragment`() {
        val encoded = encodedFixture()
        val mutations = listOf(
            "REPOSE://pair/v1/$encoded",
            "repose://PAIR/v1/$encoded",
            "repose:/pair/v1/$encoded",
            "repose://pair//v1/$encoded",
            "repose://pair/v1/$encoded/",
            "repose://pair/v1/$encoded?copy=true",
            "repose://pair/v1/$encoded#fragment",
            "repose://user@pair/v1/$encoded",
            "repose://pair:7/v1/$encoded",
        )

        mutations.forEach { mutation ->
            assertThrows(PairingUriFormatException::class.java) {
                PairingUriV1.decode(mutation)
            }
        }
    }

    @Test
    fun `URI rejects padding escapes alphabet changes and nonzero unused bits`() {
        val encoded = encodedFixture()
        val lastIndex = Base64UrlAlphabet.indexOf(encoded.last())
        val sameDecodedBytesButNoncanonical = encoded.dropLast(1) +
            Base64UrlAlphabet[lastIndex + 1]
        val mutations = listOf(
            "repose://pair/v1/$encoded=",
            "repose://pair/v1/${encoded.replaceFirst('-', '+')}",
            "repose://pair/v1/${encoded.replaceFirst('-', '/')}",
            "repose://pair/v1/${encoded.replaceFirst("U", "%55")}",
            "repose://pair/v1/$sameDecodedBytesButNoncanonical",
        )

        mutations.forEach { mutation ->
            assertThrows(PairingUriFormatException::class.java) {
                PairingUriV1.decode(mutation)
            }
        }
    }

    @Test
    fun `URI rejects oversized and non RPPK payloads`() {
        val nonRppk = Base64.getUrlEncoder().withoutPadding()
            .encodeToString(ByteArray(164) { 1 })
        val oversized = "A".repeat(4097)

        listOf(
            "repose://pair/v1/$nonRppk",
            "repose://pair/v1/$oversized",
        ).forEach { mutation ->
            assertThrows(PairingUriFormatException::class.java) {
                PairingUriV1.decode(mutation)
            }
        }
    }

    private fun encodedFixture(): String = Base64.getUrlEncoder().withoutPadding()
        .encodeToString(fixture())

    private fun canonicalUri(frame: ByteArray): String =
        "repose://pair/v1/${Base64.getUrlEncoder().withoutPadding().encodeToString(frame)}"

    private fun fixture(): ByteArray {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        return root.resolve("protocol/fixtures/v1/pairing-payload.bin").toFile().readBytes()
    }

    private companion object {
        const val Base64UrlAlphabet =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    }
}
