package ai.repose.mobile.unlock.bluetooth

import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class BleTransportReducerTest {
    private val reducer = BleTransportReducer(
        RetryPolicy(maxOpenAttempts = 3, cooldownMillis = 1_000),
    )
    private val binding = TransportBinding(
        AssociationId(81),
        PairingGeneration(7uL),
        RuntimeToken(101),
    )

    @Test
    fun `all callbacks require association generation runtime operation and link tokens`() {
        val operation = operation(1)
        val started = reducer.reduce(
            TransportState.Stopped,
            TransportEvent.RuntimeStarted(binding, operation.operationToken, bluetoothAvailable = true),
        )
        val opening = started.state
        assertTrue(opening is TransportState.Opening)

        val staleBindings = listOf(
            binding.copy(associationId = AssociationId(82)),
            binding.copy(pairingGeneration = PairingGeneration(8uL)),
            binding.copy(runtimeToken = RuntimeToken(102)),
        )
        staleBindings.forEach { stale ->
            val ignored = reducer.reduce(
                opening,
                TransportEvent.LinkEstablished(
                    operation.copy(binding = stale),
                    LinkToken(201),
                ),
            )
            assertSame(opening, ignored.state)
            assertTrue(ignored.commands.isEmpty())
        }
        val wrongOperation = reducer.reduce(
            opening,
            TransportEvent.LinkEstablished(operation(2), LinkToken(201)),
        )
        assertSame(opening, wrongOperation.state)

        val connected = reducer.reduce(
            opening,
            TransportEvent.LinkEstablished(operation, LinkToken(201)),
        ).state
        val wrongLink = reducer.reduce(
            connected,
            TransportEvent.FragmentReceived(
                operation,
                LinkToken(202),
                byteArrayOf(1),
                MonotonicMillis(0),
            ),
        )
        assertSame(connected, wrongLink.state)
        assertTrue(wrongLink.commands.isEmpty())
    }

    @Test
    fun `process recreation accepts only the new runtime token`() {
        val oldBinding = binding.copy(runtimeToken = RuntimeToken(100))
        val newReducer = BleTransportReducer()
        val started = newReducer.reduce(
            TransportState.Stopped,
            TransportEvent.RuntimeStarted(binding, OperationToken(1), bluetoothAvailable = true),
        )

        val staleCallback = newReducer.reduce(
            started.state,
            TransportEvent.LinkEstablished(
                TransportOperation(oldBinding, OperationToken(1)),
                LinkToken(1),
            ),
        )

        assertSame(started.state, staleCallback.state)
        assertTrue(staleCallback.commands.isEmpty())
    }

    @Test
    fun `all unsigned pairing generation bits participate in callback binding`() {
        listOf(0uL, ULong.MAX_VALUE).forEachIndexed { index, generation ->
            val unsignedBinding = binding.copy(
                pairingGeneration = PairingGeneration(generation),
            )
            val operation = TransportOperation(
                unsignedBinding,
                OperationToken(index + 1L),
            )
            val started = reducer.reduce(
                TransportState.Stopped,
                TransportEvent.RuntimeStarted(
                    unsignedBinding,
                    operation.operationToken,
                    bluetoothAvailable = true,
                ),
            )

            val accepted = reducer.reduce(
                started.state,
                TransportEvent.LinkEstablished(operation, LinkToken(index + 1L)),
            )

            assertTrue(accepted.state is TransportState.Connected)
        }
    }

    @Test
    fun `bluetooth off cancels or closes once and resumes only with a fresh operation`() {
        val operation = operation(1)
        val opening = start(operation).state
        val offWhileOpening = reducer.reduce(
            opening,
            TransportEvent.BluetoothUnavailable(binding),
        )
        assertTrue(offWhileOpening.state is TransportState.BluetoothOff)
        assertEquals(listOf(TransportCommand.CancelOpen(operation)), offWhileOpening.commands)

        val duplicateOff = reducer.reduce(
            offWhileOpening.state,
            TransportEvent.BluetoothUnavailable(binding),
        )
        assertSame(offWhileOpening.state, duplicateOff.state)
        assertTrue(duplicateOff.commands.isEmpty())

        val earlyReuse = reducer.reduce(
            offWhileOpening.state,
            TransportEvent.BluetoothAvailable(binding, operation.operationToken),
        )
        assertSame(offWhileOpening.state, earlyReuse.state)

        val operation2 = operation(2)
        val resumed = reducer.reduce(
            offWhileOpening.state,
            TransportEvent.BluetoothAvailable(binding, operation2.operationToken),
        )
        assertEquals(TransportState.Opening(operation2, 1), resumed.state)
        assertEquals(listOf(TransportCommand.OpenLink(operation2)), resumed.commands)

        val session = TransportSession(operation2, LinkToken(2))
        val connected = reducer.reduce(
            resumed.state,
            TransportEvent.LinkEstablished(operation2, session.linkToken),
        ).state
        val offWhileConnected = reducer.reduce(
            connected,
            TransportEvent.BluetoothUnavailable(binding),
        )
        assertEquals(listOf(TransportCommand.CloseLink(session)), offWhileConnected.commands)
    }

    @Test
    fun `bluetooth off discards partial bytes before the next operation`() {
        val operation1 = operation(1)
        val link1 = LinkToken(1)
        var state = reducer.reduce(
            start(operation1).state,
            TransportEvent.LinkEstablished(operation1, link1),
        ).state
        val fragments = GattFragmentCodec.encodeWithPayloadSizes(
            BluetoothTestFixtures.challenge(),
            listOf(100, 149),
        )
        state = reducer.reduce(
            state,
            TransportEvent.FragmentReceived(
                operation1,
                link1,
                fragments.first(),
                MonotonicMillis(100),
            ),
        ).state
        state = reducer.reduce(
            state,
            TransportEvent.BluetoothUnavailable(binding),
        ).state
        val operation2 = operation(2)
        state = reducer.reduce(
            state,
            TransportEvent.BluetoothAvailable(binding, operation2.operationToken),
        ).state
        val link2 = LinkToken(2)
        state = reducer.reduce(
            state,
            TransportEvent.LinkEstablished(operation2, link2),
        ).state

        val cannotCompleteOldFrame = reducer.reduce(
            state,
            TransportEvent.FragmentReceived(
                operation2,
                link2,
                fragments.last(),
                MonotonicMillis(200),
            ),
        )

        assertTrue(cannotCompleteOldFrame.state is TransportState.CoolingDown)
        assertTrue(
            cannotCompleteOldFrame.commands.none { it is TransportCommand.ChallengeReady },
        )
    }

    @Test
    fun `trusted runtime stop clears partial bytes before association replacement`() {
        val operation1 = operation(1)
        val link1 = LinkToken(1)
        var state = reducer.reduce(
            start(operation1).state,
            TransportEvent.LinkEstablished(operation1, link1),
        ).state
        val fragments = GattFragmentCodec.encodeWithPayloadSizes(
            BluetoothTestFixtures.challenge(),
            listOf(100, 149),
        )
        state = reducer.reduce(
            state,
            TransportEvent.FragmentReceived(
                operation1,
                link1,
                fragments.first(),
                MonotonicMillis(100),
            ),
        ).state
        state = reducer.reduce(
            state,
            TransportEvent.RuntimeStopped(binding),
        ).state

        val replacementBinding = TransportBinding(
            AssociationId(82),
            PairingGeneration(ULong.MAX_VALUE),
            RuntimeToken(102),
        )
        val replacement = TransportOperation(replacementBinding, OperationToken(2))
        state = reducer.reduce(
            state,
            TransportEvent.RuntimeStarted(
                replacementBinding,
                replacement.operationToken,
                bluetoothAvailable = true,
            ),
        ).state
        val replacementLink = LinkToken(2)
        state = reducer.reduce(
            state,
            TransportEvent.LinkEstablished(replacement, replacementLink),
        ).state

        val cannotCompleteOldFrame = reducer.reduce(
            state,
            TransportEvent.FragmentReceived(
                replacement,
                replacementLink,
                fragments.last(),
                MonotonicMillis(200),
            ),
        )

        assertTrue(cannotCompleteOldFrame.state is TransportState.CoolingDown)
        assertTrue(
            cannotCompleteOldFrame.commands.none { it is TransportCommand.ChallengeReady },
        )
    }

    @Test
    fun `trusted binding replacement atomically closes old partial link and opens fresh`() {
        val operation1 = operation(1)
        val link1 = LinkToken(1)
        var state = reducer.reduce(
            start(operation1).state,
            TransportEvent.LinkEstablished(operation1, link1),
        ).state
        val fragments = GattFragmentCodec.encodeWithPayloadSizes(
            BluetoothTestFixtures.challenge(),
            listOf(100, 149),
        )
        state = reducer.reduce(
            state,
            TransportEvent.FragmentReceived(
                operation1,
                link1,
                fragments.first(),
                MonotonicMillis(100),
            ),
        ).state
        val replacementBinding = binding.copy(
            pairingGeneration = PairingGeneration(8uL),
            runtimeToken = RuntimeToken(102),
        )
        val replacement = TransportOperation(replacementBinding, OperationToken(2))

        val replaced = reducer.reduce(
            state,
            TransportEvent.RuntimeStarted(
                replacementBinding,
                replacement.operationToken,
                bluetoothAvailable = true,
            ),
        )

        assertEquals(TransportState.Opening(replacement, attemptsUsed = 1), replaced.state)
        assertEquals(2, replaced.commands.size)
        assertEquals(TransportCommand.CloseLink(TransportSession(operation1, link1)), replaced.commands[0])
        assertEquals(TransportCommand.OpenLink(replacement), replaced.commands[1])
    }

    @Test
    fun `link loss reconnects only after monotonic cooldown without sleeping`() {
        val operation1 = operation(1)
        val link1 = LinkToken(11)
        val connected = reducer.reduce(
            start(operation1).state,
            TransportEvent.LinkEstablished(operation1, link1),
        ).state

        val lost = reducer.reduce(
            connected,
            TransportEvent.LinkLost(operation1, link1, MonotonicMillis(5_000)),
        )
        assertEquals(
            TransportState.CoolingDown(operation1, attemptsUsed = 1, dueAt = MonotonicMillis(6_000)),
            lost.state,
        )
        assertEquals(1, lost.commands.filterIsInstance<TransportCommand.ScheduleRetry>().size)

        val operation2 = operation(2)
        val tooEarly = reducer.reduce(
            lost.state,
            TransportEvent.RetryElapsed(
                operation1,
                operation2.operationToken,
                MonotonicMillis(5_999),
            ),
        )
        assertSame(lost.state, tooEarly.state)
        val reopened = reducer.reduce(
            lost.state,
            TransportEvent.RetryElapsed(
                operation1,
                operation2.operationToken,
                MonotonicMillis(6_000),
            ),
        )
        assertEquals(TransportState.Opening(operation2, attemptsUsed = 2), reopened.state)
        assertEquals(listOf(TransportCommand.OpenLink(operation2)), reopened.commands)
    }

    @Test
    fun `retry count is bounded and duplicate failure callbacks are idempotent`() {
        var state: TransportState = start(operation(1)).state
        var scheduled = 0

        for (attempt in 1..3) {
            val current = operation(attempt.toLong())
            val failed = reducer.reduce(
                state,
                TransportEvent.LinkOpenFailed(current, MonotonicMillis(attempt * 10_000L)),
            )
            scheduled += failed.commands.filterIsInstance<TransportCommand.ScheduleRetry>().size
            val duplicate = reducer.reduce(
                failed.state,
                TransportEvent.LinkOpenFailed(current, MonotonicMillis(attempt * 10_000L)),
            )
            assertSame(failed.state, duplicate.state)
            assertTrue(duplicate.commands.isEmpty())
            state = failed.state

            if (attempt < 3) {
                val next = operation((attempt + 1).toLong())
                state = reducer.reduce(
                    state,
                    TransportEvent.RetryElapsed(
                        current,
                        next.operationToken,
                        MonotonicMillis(attempt * 10_000L + 1_000),
                    ),
                ).state
            }
        }

        assertEquals(2, scheduled)
        assertEquals(TransportState.Exhausted(operation(3), attemptsUsed = 3), state)
    }

    @Test
    fun `operation tokens strictly advance within one binding and cannot wrap at signed max`() {
        val operation2 = operation(2)
        val cooldown = reducer.reduce(
            start(operation2).state,
            TransportEvent.LinkOpenFailed(operation2, MonotonicMillis(1_000)),
        ).state

        listOf(OperationToken(1), OperationToken(2)).forEach { reused ->
            val rejected = reducer.reduce(
                cooldown,
                TransportEvent.RetryElapsed(
                    operation2,
                    reused,
                    MonotonicMillis(2_000),
                ),
            )
            assertSame(cooldown, rejected.state)
            assertTrue(rejected.commands.isEmpty())
        }

        val maxOperation = operation(Long.MAX_VALUE)
        val maxCooldown = reducer.reduce(
            start(maxOperation).state,
            TransportEvent.LinkOpenFailed(maxOperation, MonotonicMillis(1_000)),
        ).state
        val cannotWrap = reducer.reduce(
            maxCooldown,
            TransportEvent.RetryElapsed(
                maxOperation,
                OperationToken(1),
                MonotonicMillis(2_000),
            ),
        )
        assertSame(maxCooldown, cannotWrap.state)

        val off = reducer.reduce(
            start(operation2).state,
            TransportEvent.BluetoothUnavailable(binding),
        ).state
        val oldTokenResume = reducer.reduce(
            off,
            TransportEvent.BluetoothAvailable(binding, OperationToken(1)),
        )
        assertSame(off, oldTokenResume.state)
        assertTrue(oldTokenResume.commands.isEmpty())
    }

    @Test
    fun `invalid or duplicate fragments fail closed and schedule one retry`() {
        val operation = operation(1)
        val link = LinkToken(5)
        val connected = reducer.reduce(
            start(operation).state,
            TransportEvent.LinkEstablished(operation, link),
        ).state
        val fragment = GattFragmentCodec.encodeWithPayloadSizes(
            BluetoothTestFixtures.challenge(),
            listOf(100, 149),
        ).first()
        val partial = reducer.reduce(
            connected,
            TransportEvent.FragmentReceived(operation, link, fragment, MonotonicMillis(1_000)),
        ).state

        val rejected = reducer.reduce(
            partial,
            TransportEvent.FragmentReceived(operation, link, fragment, MonotonicMillis(1_001)),
        )

        assertTrue(rejected.state is TransportState.CoolingDown)
        assertEquals(1, rejected.commands.filterIsInstance<TransportCommand.CloseLink>().size)
        assertEquals(1, rejected.commands.filterIsInstance<TransportCommand.ScheduleRetry>().size)
        val duplicate = reducer.reduce(
            rejected.state,
            TransportEvent.FragmentReceived(operation, link, fragment, MonotonicMillis(1_002)),
        )
        assertSame(rejected.state, duplicate.state)
        assertTrue(duplicate.commands.isEmpty())
    }

    @Test
    fun `disconnect resets partial assembly and old timer cannot affect a new link`() {
        val operation1 = operation(1)
        val link1 = LinkToken(5)
        val connected1 = reducer.reduce(
            start(operation1).state,
            TransportEvent.LinkEstablished(operation1, link1),
        ).state
        val fragments = GattFragmentCodec.encodeWithPayloadSizes(
            BluetoothTestFixtures.challenge(),
            listOf(100, 149),
        )
        val partial = reducer.reduce(
            connected1,
            TransportEvent.FragmentReceived(
                operation1,
                link1,
                fragments.first(),
                MonotonicMillis(1_000),
            ),
        ).state
        val coolingDown = reducer.reduce(
            partial,
            TransportEvent.LinkLost(operation1, link1, MonotonicMillis(2_000)),
        ).state
        val operation2 = operation(2)
        val opening2 = reducer.reduce(
            coolingDown,
            TransportEvent.RetryElapsed(
                operation1,
                operation2.operationToken,
                MonotonicMillis(3_000),
            ),
        ).state
        val link2 = LinkToken(6)
        val connected2 = reducer.reduce(
            opening2,
            TransportEvent.LinkEstablished(operation2, link2),
        ).state

        val staleOldTimer = reducer.reduce(
            connected2,
            TransportEvent.RetryElapsed(
                operation1,
                OperationToken(3),
                MonotonicMillis(9_000),
            ),
        )
        assertSame(connected2, staleOldTimer.state)

        val trailingOldFragment = reducer.reduce(
            connected2,
            TransportEvent.FragmentReceived(
                operation2,
                link2,
                fragments.last(),
                MonotonicMillis(3_001),
            ),
        )
        assertTrue(trailingOldFragment.state is TransportState.CoolingDown)
        assertTrue(
            trailingOldFragment.commands.none { it is TransportCommand.ChallengeReady },
        )
    }

    @Test
    fun `transport hands only exact Challenge frames to the responder boundary`() {
        val operation = operation(1)
        val link = LinkToken(5)
        var state = reducer.reduce(
            start(operation).state,
            TransportEvent.LinkEstablished(operation, link),
        ).state

        GattFragmentCodec.encode(
            BluetoothTestFixtures.response(),
            maximumAttributeValueBytes = 23,
        ).forEach { fragment ->
            state = reducer.reduce(
                state,
                TransportEvent.FragmentReceived(
                    operation,
                    link,
                    fragment,
                    MonotonicMillis(1_000),
                ),
            ).state
        }

        assertTrue(state is TransportState.CoolingDown)
    }

    private fun start(operation: TransportOperation): TransportTransition = reducer.reduce(
        TransportState.Stopped,
        TransportEvent.RuntimeStarted(
            operation.binding,
            operation.operationToken,
            bluetoothAvailable = true,
        ),
    )

    private fun operation(value: Long): TransportOperation =
        TransportOperation(binding, OperationToken(value))
}
