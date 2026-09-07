package ai.repose.mobile.unlock.bluetooth

import java.nio.file.Path
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class BleRoleStrategyTest {
    private val binding = TransportBinding(
        AssociationId(91),
        PairingGeneration(7uL),
        RuntimeToken(400),
    )

    @Test
    fun `both role adapters deliver a challenge for every possible GATT split`() {
        val challenge = BluetoothTestFixtures.challenge()

        roleFactories().forEach { factory ->
            for (split in 1 until challenge.size) {
                val harness = factory.create()
                val operation = TransportOperation(binding, OperationToken(split.toLong()))
                val link = LinkToken((split + 1_000).toLong())
                harness.strategy.handle(
                    TransportEvent.RuntimeStarted(
                        binding,
                        operation.operationToken,
                        bluetoothAvailable = true,
                    ),
                )
                harness.strategy.handle(TransportEvent.LinkEstablished(operation, link))
                GattFragmentCodec.encodeWithPayloadSizes(
                    challenge,
                    listOf(split, challenge.size - split),
                ).forEach { fragment ->
                    harness.strategy.handle(
                        TransportEvent.FragmentReceived(
                            operation,
                            link,
                            fragment,
                            MonotonicMillis(10),
                        ),
                    )
                }

                assertEquals(factory.expectedOpenAction, harness.recorder.actions.first())
                assertEquals(1, harness.recorder.challenges.size)
                assertArrayEquals(challenge, harness.recorder.challenges.single())
                assertTrue(harness.strategy.currentState is TransportState.FrameDelivered)
            }
        }
    }

    @Test
    fun `both adapters ignore stale and duplicate callbacks and reconnect through reducer`() {
        roleFactories().forEach { factory ->
            val harness = factory.create()
            val operation1 = TransportOperation(binding, OperationToken(1))
            val session1 = TransportSession(operation1, LinkToken(10))
            harness.strategy.handle(
                TransportEvent.RuntimeStarted(binding, operation1.operationToken, true),
            )
            harness.strategy.handle(
                TransportEvent.LinkEstablished(
                    operation1.copy(
                        binding = binding.copy(pairingGeneration = PairingGeneration(6uL)),
                    ),
                    session1.linkToken,
                ),
            )
            val connected = harness.strategy.handle(
                TransportEvent.LinkEstablished(operation1, session1.linkToken),
            )
            val actionCount = harness.recorder.actions.size
            val duplicate = harness.strategy.handle(
                TransportEvent.LinkEstablished(operation1, session1.linkToken),
            )
            assertSame(connected, duplicate)
            assertEquals(actionCount, harness.recorder.actions.size)

            harness.strategy.handle(
                TransportEvent.LinkLost(
                    operation1,
                    session1.linkToken,
                    MonotonicMillis(1_000),
                ),
            )
            harness.strategy.handle(
                TransportEvent.RetryElapsed(
                    operation1,
                    OperationToken(2),
                    MonotonicMillis(2_000),
                ),
            )
            assertEquals(1, harness.recorder.retryTickets.size)
            assertEquals(2, harness.recorder.openCount)
        }
    }

    @Test
    fun `production role remains disabled without GT5 evidence`() {
        assertEquals(BleRole.Disabled, BleRoleSelector.productionRole())
        assertTrue(BleRoleSelector.productionStrategy() is DisabledBleRoleStrategy)
    }

    @Test
    fun `Pigeon does not expose BLE role or raw GATT operations`() {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val pigeon = root.resolve(
            "mobile/packages/repose_unlock_native/pigeons/repose_unlock_api.dart",
        ).toFile().readText()

        listOf("Gatt", "gatt", "BleRole", "bleRole", "rawFrame", "writeCharacteristic")
            .forEach { forbidden ->
                assertTrue("Pigeon unexpectedly exposes $forbidden", forbidden !in pigeon)
            }
    }

    @Test
    fun `transport reducer contains no clock read sleep or blocking delay`() {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val reducerSource = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/bluetooth/BleTransportReducer.kt",
        ).toFile().readText()

        listOf("Thread.sleep", "System.currentTimeMillis", "elapsedRealtime", "delay(")
            .forEach { forbidden ->
                assertTrue("reducer unexpectedly uses $forbidden", forbidden !in reducerSource)
            }
    }

    private fun roleFactories(): List<RoleFactory> = listOf(
        RoleFactory("peripheral-open") {
            val recorder = RecordingPort()
            RoleHarness(MacCentralPhonePeripheralStrategy(recorder), recorder)
        },
        RoleFactory("central-open") {
            val recorder = RecordingPort()
            RoleHarness(MacPeripheralPhoneCentralStrategy(recorder), recorder)
        },
    )

    private data class RoleFactory(
        val expectedOpenAction: String,
        val create: () -> RoleHarness,
    )

    private data class RoleHarness(
        val strategy: BleRoleStrategy,
        val recorder: RecordingPort,
    )

    private class RecordingPort : PhonePeripheralPlatformPort, PhoneCentralPlatformPort {
        val actions = mutableListOf<String>()
        val challenges = mutableListOf<ByteArray>()
        val retryTickets = mutableListOf<RetryTicket>()
        var openCount = 0

        override fun beginGattServerAndAdvertising(operation: TransportOperation) {
            actions += "peripheral-open"
            openCount += 1
        }

        override fun stopGattServerAndAdvertising(operation: TransportOperation) {
            actions += "peripheral-cancel"
        }

        override fun disconnectCentral(session: TransportSession) {
            actions += "peripheral-close"
        }

        override fun beginScanAndConnect(operation: TransportOperation) {
            actions += "central-open"
            openCount += 1
        }

        override fun cancelScanAndConnect(operation: TransportOperation) {
            actions += "central-cancel"
        }

        override fun disconnectPeripheral(session: TransportSession) {
            actions += "central-close"
        }

        override fun scheduleRetry(ticket: RetryTicket) {
            retryTickets += ticket
        }

        override fun cancelRetry(ticket: RetryTicket) {
            actions += "retry-cancel"
        }

        override fun onChallengeFrame(session: TransportSession, challengeFrame: ByteArray) {
            challenges += challengeFrame.copyOf()
        }
    }
}
