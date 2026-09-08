package ai.repose.mobile.unlock

import ai.repose.mobile.unlock.generated.FlutterError
import ai.repose.mobile.unlock.generated.NativeCompanionCapability
import ai.repose.mobile.unlock.pairing.DebugGattPairingConnection
import ai.repose.mobile.unlock.pairing.DebugGattPairingObserver
import ai.repose.mobile.unlock.pairing.DebugGattPairingRequest
import ai.repose.mobile.unlock.pairing.DebugGattPairingTransport
import ai.repose.mobile.unlock.pairing.DebugPairingCoordinator
import ai.repose.mobile.unlock.pairing.DebugPhoneIdentity
import java.nio.charset.StandardCharsets
import java.nio.file.Path
import java.util.Base64
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class DebugReposeUnlockHostApiTest {
    private val transport = RecordingTransport()
    private var availability = NativeRuntimeAvailability.TRANSPORT_NOT_IMPLEMENTED
    private var bluetoothReady = true
    private val api = DebugReposeUnlockHostApi(
        coordinator = DebugPairingCoordinator(
            transport = transport,
            nowEpochMillis = { 1_700_000_000_000uL },
            associationId = { 9 },
            phoneIdentity = DebugPhoneIdentity("phone", "Android phone"),
        ),
        requestCompanionAssociation = { callback -> callback(Result.success(Unit)) },
        availability = { availability },
        bluetoothReady = { bluetoothReady },
    )

    @Test
    fun `debug transport is ready only for a configured association with bluetooth`() {
        assertEquals(NativeCompanionCapability.READY, api.getSnapshot().capability)

        bluetoothReady = false
        assertEquals(
            NativeCompanionCapability.BLUETOOTH_UNAVAILABLE,
            api.getSnapshot().capability,
        )

        bluetoothReady = true
        availability = NativeRuntimeAvailability.ASSOCIATION_NOT_CONFIGURED
        assertEquals(
            NativeCompanionCapability.ASSOCIATION_NOT_CONFIGURED,
            api.getSnapshot().capability,
        )
    }

    @Test
    fun `host exposes authoritative pending and paired snapshots only after Mac acceptance`() {
        val session = api.beginPairing(canonicalUri())
        assertEquals(session, api.getSnapshot().pendingPairing)

        val early = assertThrows(FlutterError::class.java) {
            api.confirmPairing(session.sessionId)
        }
        assertEquals("pairingNotAccepted", early.code)
        assertTrue(api.getSnapshot().devices.isEmpty())

        transport.observer.onStatus(
            "ACCEPTED|${session.sessionId}".toByteArray(StandardCharsets.US_ASCII),
        )
        api.confirmPairing(session.sessionId)

        val paired = api.getSnapshot()
        assertNull(paired.pendingPairing)
        assertEquals("000102030405060708090a0b0c0d0e0f", paired.devices.single().id)
        assertEquals("Repose MacBook Pro", paired.devices.single().displayName)
    }

    @Test
    fun `host maps malformed and expired QR without exposing payload contents`() {
        val invalid = assertThrows(FlutterError::class.java) {
            api.beginPairing("repose://pair/v1/not-a-frame")
        }
        assertEquals("invalidPairingCode", invalid.code)
        assertTrue(invalid.message?.contains("not-a-frame") == false)

        val expiredApi = DebugReposeUnlockHostApi(
            coordinator = DebugPairingCoordinator(
                transport = transport,
                nowEpochMillis = { 1_800_000_000_000uL },
                associationId = { 9 },
                phoneIdentity = DebugPhoneIdentity("phone", "Android phone"),
            ),
            requestCompanionAssociation = {},
            availability = { NativeRuntimeAvailability.TRANSPORT_NOT_IMPLEMENTED },
            bluetoothReady = { true },
        )
        val expired = assertThrows(FlutterError::class.java) {
            expiredApi.beginPairing(canonicalUri())
        }
        assertEquals("qrExpired", expired.code)
    }

    private fun canonicalUri(): String {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val frame = root.resolve("protocol/fixtures/v1/pairing-payload.bin")
            .toFile()
            .readBytes()
        return "repose://pair/v1/" +
            Base64.getUrlEncoder().withoutPadding().encodeToString(frame)
    }

    private class RecordingTransport : DebugGattPairingTransport {
        lateinit var observer: DebugGattPairingObserver

        override fun connect(
            request: DebugGattPairingRequest,
            observer: DebugGattPairingObserver,
        ): DebugGattPairingConnection {
            this.observer = observer
            return DebugGattPairingConnection {}
        }
    }
}
