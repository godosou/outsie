package ai.repose.mobile.unlock.bluetooth

import ai.repose.mobile.unlock.protocol.ProtocolV1

@JvmInline
internal value class AssociationId(val value: Int) {
    init {
        require(value > 0) { "association id must be positive" }
    }
}

@JvmInline
internal value class PairingGeneration(val bits: ULong)

@JvmInline
internal value class RuntimeToken(val value: Long) {
    init {
        require(value > 0) { "runtime token must be positive" }
    }
}

/** Strictly increases within one [TransportBinding]; it carries no protocol authority. */
@JvmInline
internal value class OperationToken(val value: Long) {
    init {
        require(value > 0) { "operation token must be positive" }
    }
}

@JvmInline
internal value class LinkToken(val value: Long) {
    init {
        require(value > 0) { "link token must be positive" }
    }
}

@JvmInline
internal value class MonotonicMillis(val value: Long) : Comparable<MonotonicMillis> {
    init {
        require(value >= 0) { "monotonic time must not be negative" }
    }

    override fun compareTo(other: MonotonicMillis): Int = value.compareTo(other.value)

    fun plusSaturated(deltaMillis: Long): MonotonicMillis {
        require(deltaMillis >= 0)
        return if (value > Long.MAX_VALUE - deltaMillis) {
            MonotonicMillis(Long.MAX_VALUE)
        } else {
            MonotonicMillis(value + deltaMillis)
        }
    }
}

internal data class TransportBinding(
    val associationId: AssociationId,
    val pairingGeneration: PairingGeneration,
    val runtimeToken: RuntimeToken,
)

internal data class TransportOperation(
    val binding: TransportBinding,
    val operationToken: OperationToken,
)

internal data class TransportSession(
    val operation: TransportOperation,
    val linkToken: LinkToken,
)

internal data class RetryPolicy(
    val maxOpenAttempts: Int = 3,
    val cooldownMillis: Long = 1_000,
) {
    init {
        require(maxOpenAttempts in 1..5) { "retry count must remain tightly bounded" }
        require(cooldownMillis in 1..60_000) { "cooldown must be bounded" }
    }
}

internal data class RetryTicket(
    val failedOperation: TransportOperation,
    val dueAt: MonotonicMillis,
)

internal sealed interface TransportState {
    data object Stopped : TransportState

    data class Opening(
        val operation: TransportOperation,
        val attemptsUsed: Int,
    ) : TransportState

    class Connected internal constructor(
        val session: TransportSession,
        val attemptsUsed: Int,
        val reassembly: GattReassembly,
    ) : TransportState

    data class FrameDelivered(
        val session: TransportSession,
        val attemptsUsed: Int,
    ) : TransportState

    data class CoolingDown(
        val failedOperation: TransportOperation,
        val attemptsUsed: Int,
        val dueAt: MonotonicMillis,
    ) : TransportState

    data class BluetoothOff(
        val binding: TransportBinding,
        val previousOperationToken: OperationToken,
    ) : TransportState

    data class Exhausted(
        val failedOperation: TransportOperation,
        val attemptsUsed: Int,
    ) : TransportState
}

internal sealed interface TransportEvent {
    /** Emitted only by the trusted, serialized native lifecycle owner. */
    data class RuntimeStarted(
        val binding: TransportBinding,
        val initialOperationToken: OperationToken,
        val bluetoothAvailable: Boolean,
    ) : TransportEvent

    data class LinkEstablished(
        val operation: TransportOperation,
        val linkToken: LinkToken,
    ) : TransportEvent

    data class LinkOpenFailed(
        val operation: TransportOperation,
        val observedAt: MonotonicMillis,
    ) : TransportEvent

    data class LinkLost(
        val operation: TransportOperation,
        val linkToken: LinkToken,
        val observedAt: MonotonicMillis,
    ) : TransportEvent

    class FragmentReceived(
        val operation: TransportOperation,
        val linkToken: LinkToken,
        packet: ByteArray,
        val observedAt: MonotonicMillis,
    ) : TransportEvent {
        private val packet: ByteArray = packet.copyOf()

        internal fun copyPacketForReducer(): ByteArray = packet.copyOf()
    }

    data class RetryElapsed(
        val failedOperation: TransportOperation,
        val nextOperationToken: OperationToken,
        val observedAt: MonotonicMillis,
    ) : TransportEvent

    data class BluetoothUnavailable(val binding: TransportBinding) : TransportEvent

    data class BluetoothAvailable(
        val binding: TransportBinding,
        val nextOperationToken: OperationToken,
    ) : TransportEvent

    data class RuntimeStopped(val binding: TransportBinding) : TransportEvent
}

internal sealed interface TransportCommand {
    data class OpenLink(val operation: TransportOperation) : TransportCommand
    data class CancelOpen(val operation: TransportOperation) : TransportCommand
    data class CloseLink(val session: TransportSession) : TransportCommand
    data class ScheduleRetry(val ticket: RetryTicket) : TransportCommand
    data class CancelRetry(val ticket: RetryTicket) : TransportCommand

    class ChallengeReady internal constructor(
        val session: TransportSession,
        challengeFrame: ByteArray,
    ) : TransportCommand {
        private val challengeFrame: ByteArray = challengeFrame.copyOf()

        internal fun copyChallengeForNativeConsumer(): ByteArray = challengeFrame.copyOf()
    }
}

internal data class TransportTransition(
    val state: TransportState,
    val commands: List<TransportCommand> = emptyList(),
)

/** Pure state transition logic. It never reads a clock, starts a thread, or sleeps. */
internal class BleTransportReducer(
    private val retryPolicy: RetryPolicy = RetryPolicy(),
) {
    fun reduce(state: TransportState, event: TransportEvent): TransportTransition = when (event) {
        is TransportEvent.RuntimeStarted -> start(state, event)
        is TransportEvent.LinkEstablished -> linkEstablished(state, event)
        is TransportEvent.LinkOpenFailed -> openFailed(state, event)
        is TransportEvent.LinkLost -> linkLost(state, event)
        is TransportEvent.FragmentReceived -> fragmentReceived(state, event)
        is TransportEvent.RetryElapsed -> retryElapsed(state, event)
        is TransportEvent.BluetoothUnavailable -> bluetoothUnavailable(state, event)
        is TransportEvent.BluetoothAvailable -> bluetoothAvailable(state, event)
        is TransportEvent.RuntimeStopped -> stop(state, event)
    }

    private fun start(
        state: TransportState,
        event: TransportEvent.RuntimeStarted,
    ): TransportTransition {
        if (state !== TransportState.Stopped) {
            if (bindingOf(state) == event.binding) return unchanged(state)
            val replacement = start(TransportState.Stopped, event)
            val oldCancellation = cancellationCommands(state)
            return replacement.copy(commands = oldCancellation + replacement.commands)
        }
        val operation = TransportOperation(event.binding, event.initialOperationToken)
        return if (event.bluetoothAvailable) {
            TransportTransition(
                TransportState.Opening(operation, attemptsUsed = 1),
                listOf(TransportCommand.OpenLink(operation)),
            )
        } else {
            TransportTransition(
                TransportState.BluetoothOff(event.binding, event.initialOperationToken),
            )
        }
    }

    private fun linkEstablished(
        state: TransportState,
        event: TransportEvent.LinkEstablished,
    ): TransportTransition {
        val opening = state as? TransportState.Opening ?: return unchanged(state)
        if (event.operation != opening.operation) return unchanged(state)
        return TransportTransition(
            TransportState.Connected(
                TransportSession(event.operation, event.linkToken),
                opening.attemptsUsed,
                GattReassembly.Empty,
            ),
        )
    }

    private fun openFailed(
        state: TransportState,
        event: TransportEvent.LinkOpenFailed,
    ): TransportTransition {
        val opening = state as? TransportState.Opening ?: return unchanged(state)
        if (event.operation != opening.operation) return unchanged(state)
        return failure(opening.operation, opening.attemptsUsed, event.observedAt)
    }

    private fun linkLost(
        state: TransportState,
        event: TransportEvent.LinkLost,
    ): TransportTransition {
        val sessionAndAttempts = activeSession(state) ?: return unchanged(state)
        val (session, attemptsUsed) = sessionAndAttempts
        if (event.operation != session.operation || event.linkToken != session.linkToken) {
            return unchanged(state)
        }
        return failure(session.operation, attemptsUsed, event.observedAt)
    }

    private fun fragmentReceived(
        state: TransportState,
        event: TransportEvent.FragmentReceived,
    ): TransportTransition {
        val connected = state as? TransportState.Connected ?: return unchanged(state)
        if (
            event.operation != connected.session.operation ||
            event.linkToken != connected.session.linkToken
        ) {
            return unchanged(state)
        }

        val nextReassembly = try {
            GattFragmentCodec.accept(connected.reassembly, event.copyPacketForReducer())
        } catch (_: FragmentFormatException) {
            return malformedFrameFailure(connected, event.observedAt)
        }
        if (nextReassembly !is GattReassembly.Complete) {
            return TransportTransition(
                TransportState.Connected(
                    connected.session,
                    connected.attemptsUsed,
                    nextReassembly,
                ),
            )
        }

        val challenge = try {
            GattFragmentCodec.finish(nextReassembly).also(ProtocolV1::decodeChallenge)
        } catch (_: IllegalArgumentException) {
            return malformedFrameFailure(connected, event.observedAt)
        }
        return TransportTransition(
            TransportState.FrameDelivered(connected.session, connected.attemptsUsed),
            listOf(TransportCommand.ChallengeReady(connected.session, challenge)),
        )
    }

    private fun malformedFrameFailure(
        connected: TransportState.Connected,
        observedAt: MonotonicMillis,
    ): TransportTransition {
        val failed = failure(
            connected.session.operation,
            connected.attemptsUsed,
            observedAt,
        )
        return failed.copy(
            commands = listOf(TransportCommand.CloseLink(connected.session)) + failed.commands,
        )
    }

    private fun retryElapsed(
        state: TransportState,
        event: TransportEvent.RetryElapsed,
    ): TransportTransition {
        val cooldown = state as? TransportState.CoolingDown ?: return unchanged(state)
        if (
            event.failedOperation != cooldown.failedOperation ||
            event.nextOperationToken.value <= cooldown.failedOperation.operationToken.value ||
            event.observedAt < cooldown.dueAt
        ) {
            return unchanged(state)
        }
        val nextOperation = TransportOperation(
            cooldown.failedOperation.binding,
            event.nextOperationToken,
        )
        return TransportTransition(
            TransportState.Opening(nextOperation, cooldown.attemptsUsed + 1),
            listOf(TransportCommand.OpenLink(nextOperation)),
        )
    }

    private fun bluetoothUnavailable(
        state: TransportState,
        event: TransportEvent.BluetoothUnavailable,
    ): TransportTransition {
        val binding = bindingOf(state) ?: return unchanged(state)
        if (event.binding != binding || state is TransportState.BluetoothOff) {
            return unchanged(state)
        }
        val operation = operationOf(state) ?: return unchanged(state)
        val cancellation = cancellationCommands(state)
        return TransportTransition(
            TransportState.BluetoothOff(binding, operation.operationToken),
            cancellation,
        )
    }

    private fun bluetoothAvailable(
        state: TransportState,
        event: TransportEvent.BluetoothAvailable,
    ): TransportTransition {
        val off = state as? TransportState.BluetoothOff ?: return unchanged(state)
        if (
            event.binding != off.binding ||
            event.nextOperationToken.value <= off.previousOperationToken.value
        ) {
            return unchanged(state)
        }
        val operation = TransportOperation(off.binding, event.nextOperationToken)
        return TransportTransition(
            TransportState.Opening(operation, attemptsUsed = 1),
            listOf(TransportCommand.OpenLink(operation)),
        )
    }

    private fun stop(
        state: TransportState,
        event: TransportEvent.RuntimeStopped,
    ): TransportTransition {
        if (bindingOf(state) != event.binding) return unchanged(state)
        val cancellations = cancellationCommands(state)
        return TransportTransition(TransportState.Stopped, cancellations)
    }

    private fun cancellationCommands(state: TransportState): List<TransportCommand> = when (state) {
        is TransportState.Opening -> listOf(TransportCommand.CancelOpen(state.operation))
        is TransportState.Connected -> listOf(TransportCommand.CloseLink(state.session))
        is TransportState.FrameDelivered -> listOf(TransportCommand.CloseLink(state.session))
        is TransportState.CoolingDown -> listOf(
            TransportCommand.CancelRetry(RetryTicket(state.failedOperation, state.dueAt)),
        )

        is TransportState.BluetoothOff,
        is TransportState.Exhausted,
        TransportState.Stopped,
        -> emptyList()
    }

    private fun failure(
        failedOperation: TransportOperation,
        attemptsUsed: Int,
        observedAt: MonotonicMillis,
    ): TransportTransition {
        if (attemptsUsed >= retryPolicy.maxOpenAttempts) {
            return TransportTransition(
                TransportState.Exhausted(failedOperation, attemptsUsed),
            )
        }
        val dueAt = observedAt.plusSaturated(retryPolicy.cooldownMillis)
        val ticket = RetryTicket(failedOperation, dueAt)
        return TransportTransition(
            TransportState.CoolingDown(failedOperation, attemptsUsed, dueAt),
            listOf(TransportCommand.ScheduleRetry(ticket)),
        )
    }

    private fun activeSession(state: TransportState): Pair<TransportSession, Int>? = when (state) {
        is TransportState.Connected -> state.session to state.attemptsUsed
        is TransportState.FrameDelivered -> state.session to state.attemptsUsed
        else -> null
    }

    private fun bindingOf(state: TransportState): TransportBinding? = when (state) {
        is TransportState.Opening -> state.operation.binding
        is TransportState.Connected -> state.session.operation.binding
        is TransportState.FrameDelivered -> state.session.operation.binding
        is TransportState.CoolingDown -> state.failedOperation.binding
        is TransportState.BluetoothOff -> state.binding
        is TransportState.Exhausted -> state.failedOperation.binding
        TransportState.Stopped -> null
    }

    private fun operationOf(state: TransportState): TransportOperation? = when (state) {
        is TransportState.Opening -> state.operation
        is TransportState.Connected -> state.session.operation
        is TransportState.FrameDelivered -> state.session.operation
        is TransportState.CoolingDown -> state.failedOperation
        is TransportState.Exhausted -> state.failedOperation
        is TransportState.BluetoothOff,
        TransportState.Stopped,
        -> null
    }

    private fun unchanged(state: TransportState): TransportTransition = TransportTransition(state)
}
