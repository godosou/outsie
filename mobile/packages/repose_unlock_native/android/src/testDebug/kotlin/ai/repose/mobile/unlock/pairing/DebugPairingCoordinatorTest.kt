package ai.repose.mobile.unlock.pairing

import ai.repose.mobile.unlock.protocol.PairingProtocolV1
import java.nio.charset.StandardCharsets
import java.nio.file.Path
import java.util.Base64
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class DebugPairingCoordinatorTest {
    private var now = 1_700_000_000_000uL
    private var associationId: Int? = 91
    private val transport = RecordingTransport()
    private val coordinator = DebugPairingCoordinator(
        transport = transport,
        nowEpochMillis = { now },
        associationId = { associationId },
        phoneIdentity = DebugPhoneIdentity("android_0123456789abcdef", "realme GT5 Pro"),
    )

    @Test
    fun `begin decodes canonical payload and connects the associated device with exact control frame`() {
        val session = coordinator.begin(canonicalUri())

        assertEquals("a0a1a2a3a4a5a6a7a8a9aaabacadaeaf", session.sessionId)
        assertEquals("Repose MacBook Pro", session.deviceName)
        assertEquals(1_800_000_000_000L, session.expiresAtEpochMillis)
        assertEquals(91, transport.requests.single().associationId)
        assertArrayEquals(
            "RPD1|a0a1a2a3a4a5a6a7a8a9aaabacadaeaf|android_0123456789abcdef|realme GT5 Pro"
                .toByteArray(StandardCharsets.UTF_8),
            transport.requests.single().controlValue,
        )
        assertArrayEquals(
            "ACCEPTED|a0a1a2a3a4a5a6a7a8a9aaabacadaeaf"
                .toByteArray(StandardCharsets.US_ASCII),
            transport.requests.single().expectedAcceptedStatus,
        )
        assertEquals(session, coordinator.snapshot().pending)
        assertTrue(coordinator.snapshot().devices.isEmpty())
    }

    @Test
    fun `expiry and missing association fail before opening bluetooth`() {
        now = 1_800_000_000_000uL
        assertReason(DebugPairingFailure.QR_EXPIRED) { coordinator.begin(canonicalUri()) }
        assertTrue(transport.requests.isEmpty())

        now = 1_700_000_000_000uL
        associationId = null
        assertReason(DebugPairingFailure.ASSOCIATION_UNAVAILABLE) {
            coordinator.begin(canonicalUri())
        }
        assertTrue(transport.requests.isEmpty())
    }

    @Test
    fun `only an exact accepted status permits confirmation`() {
        coordinator.begin(canonicalUri())

        assertReason(DebugPairingFailure.NOT_ACCEPTED) {
            coordinator.confirm("a0a1a2a3a4a5a6a7a8a9aaabacadaeaf")
        }
        transport.observers.single().onStatus(
            "WAITING|a0a1a2a3a4a5a6a7a8a9aaabacadaeaf".toByteArray(StandardCharsets.US_ASCII),
        )
        assertReason(DebugPairingFailure.NOT_ACCEPTED) {
            coordinator.confirm("a0a1a2a3a4a5a6a7a8a9aaabacadaeaf")
        }
        transport.observers.single().onStatus(
            "ACCEPTED|a0a1a2a3a4a5a6a7a8a9aaabacadaeaf".toByteArray(StandardCharsets.US_ASCII),
        )

        coordinator.confirm("a0a1a2a3a4a5a6a7a8a9aaabacadaeaf")

        assertNull(coordinator.snapshot().pending)
        assertEquals(
            DebugPairedDevice("000102030405060708090a0b0c0d0e0f", "Repose MacBook Pro"),
            coordinator.snapshot().devices.single(),
        )
        assertEquals(1, transport.connections.single().closeCalls)
    }

    @Test
    fun `mismatched malformed and non ascii statuses fail closed`() {
        listOf(
            "ACCEPTED|ffeeddccbbaa99887766554433221100".toByteArray(),
            "accepted|a0a1a2a3a4a5a6a7a8a9aaabacadaeaf".toByteArray(),
            "ACCEPTED|a0a1a2a3a4a5a6a7a8a9aaabacadaeaf\u0080".toByteArray(),
        ).forEach { status ->
            val localTransport = RecordingTransport()
            val local = coordinator(transport = localTransport)
            local.begin(canonicalUri())

            localTransport.observers.single().onStatus(status)

            assertNull(local.snapshot().pending)
            assertTrue(local.snapshot().devices.isEmpty())
            assertEquals(1, localTransport.connections.single().closeCalls)
        }
    }

    @Test
    fun `disconnect and transport failure clear pending authority`() {
        listOf<(DebugGattPairingObserver) -> Unit>(
            { it.onDisconnected() },
            { it.onFailure() },
        ).forEach { terminate ->
            val localTransport = RecordingTransport()
            val local = coordinator(transport = localTransport)
            local.begin(canonicalUri())

            terminate(localTransport.observers.single())

            assertNull(local.snapshot().pending)
            assertReason(DebugPairingFailure.SESSION_MISMATCH) {
                local.confirm("a0a1a2a3a4a5a6a7a8a9aaabacadaeaf")
            }
        }
    }

    @Test
    fun `stale callbacks cannot authorize a later transaction`() {
        val firstUri = canonicalUri()
        coordinator.begin(firstUri)
        val stale = transport.observers.single()
        coordinator.disconnect()
        coordinator.begin(uriWithSession(0x44))

        stale.onStatus(
            "ACCEPTED|a0a1a2a3a4a5a6a7a8a9aaabacadaeaf".toByteArray(StandardCharsets.US_ASCII),
        )

        assertReason(DebugPairingFailure.NOT_ACCEPTED) {
            coordinator.confirm("44444444444444444444444444444444")
        }
        transport.observers.last().onStatus(
            "ACCEPTED|44444444444444444444444444444444"
                .toByteArray(StandardCharsets.US_ASCII),
        )
        coordinator.confirm("44444444444444444444444444444444")
        assertEquals(1, coordinator.snapshot().devices.size)
    }

    @Test
    fun `confirmed session is one time and revocation is exact`() {
        coordinator.begin(canonicalUri())
        transport.observers.single().onStatus(
            "ACCEPTED|a0a1a2a3a4a5a6a7a8a9aaabacadaeaf".toByteArray(),
        )
        coordinator.confirm("a0a1a2a3a4a5a6a7a8a9aaabacadaeaf")

        assertReason(DebugPairingFailure.QR_ALREADY_USED) { coordinator.begin(canonicalUri()) }
        assertReason(DebugPairingFailure.DEVICE_NOT_FOUND) { coordinator.revoke("missing") }
        coordinator.revoke("000102030405060708090a0b0c0d0e0f")
        assertTrue(coordinator.snapshot().devices.isEmpty())
    }

    @Test
    fun `confirmation rechecks session and expiry`() {
        coordinator.begin(canonicalUri())
        transport.observers.single().onStatus(
            "ACCEPTED|a0a1a2a3a4a5a6a7a8a9aaabacadaeaf".toByteArray(),
        )

        assertReason(DebugPairingFailure.SESSION_MISMATCH) { coordinator.confirm("wrong") }
        now = 1_800_000_000_000uL
        assertReason(DebugPairingFailure.QR_EXPIRED) {
            coordinator.confirm("a0a1a2a3a4a5a6a7a8a9aaabacadaeaf")
        }
        assertNull(coordinator.snapshot().pending)
        assertTrue(coordinator.snapshot().devices.isEmpty())
    }

    @Test
    fun `phone identity rejects fields the Mac control parser cannot accept`() {
        listOf("", "contains space", "pipe|id", "a".repeat(65)).forEach { invalid ->
            assertThrows(IllegalArgumentException::class.java) {
                DebugPhoneIdentity(invalid, "phone")
            }
        }
        listOf("", "bad|name", "bad\nname", "x".repeat(81)).forEach { invalid ->
            assertThrows(IllegalArgumentException::class.java) {
                DebugPhoneIdentity("phone", invalid)
            }
        }
    }

    private fun coordinator(
        transport: RecordingTransport,
    ) = DebugPairingCoordinator(
        transport = transport,
        nowEpochMillis = { now },
        associationId = { associationId },
        phoneIdentity = DebugPhoneIdentity("phone", "Android phone"),
    )

    private fun canonicalUri(): String = uri(fixture())

    private fun uriWithSession(value: Int): String {
        val payload = ai.repose.mobile.unlock.protocol.PairingProtocolV1.decode(fixture())
        return uri(
            PairingProtocolV1.encode(
                sessionId = ByteArray(16) { value.toByte() },
                expiresAtEpochMillis = payload.expiresAtEpochMillis,
                macId = payload.macId,
                macIdentityPublicKey = payload.macIdentityPublicKey,
                pairingSecret = payload.pairingSecret,
                macName = payload.macName,
            ),
        )
    }

    private fun uri(frame: ByteArray): String =
        "repose://pair/v1/${Base64.getUrlEncoder().withoutPadding().encodeToString(frame)}"

    private fun fixture(): ByteArray = Path.of(
        requireNotNull(System.getProperty("repose.workspaceRoot")),
        "protocol/fixtures/v1/pairing-payload.bin",
    ).toFile().readBytes()

    private fun assertReason(reason: DebugPairingFailure, operation: () -> Unit) {
        val error = assertThrows(DebugPairingException::class.java, operation)
        assertEquals(reason, error.reason)
    }

    private class RecordingTransport : DebugGattPairingTransport {
        val requests = mutableListOf<DebugGattPairingRequest>()
        val observers = mutableListOf<DebugGattPairingObserver>()
        val connections = mutableListOf<RecordingConnection>()

        override fun connect(
            request: DebugGattPairingRequest,
            observer: DebugGattPairingObserver,
        ): DebugGattPairingConnection {
            requests += request.copy(
                controlValue = request.controlValue.copyOf(),
                expectedAcceptedStatus = request.expectedAcceptedStatus.copyOf(),
            )
            observers += observer
            return RecordingConnection().also(connections::add)
        }
    }

    private class RecordingConnection : DebugGattPairingConnection {
        var closeCalls = 0

        override fun close() {
            closeCalls += 1
        }
    }
}
