package ai.repose.mobile.unlock.bluetooth

internal enum class BleRole {
    Disabled,
    MacCentralPhonePeripheral,
    MacPeripheralPhoneCentral,
}

/**
 * The production role is intentionally closed until physical GT5 Pro evidence
 * is recorded. Test construction of either prototype never changes this gate.
 */
internal object BleRoleSelector {
    fun productionRole(): BleRole = BleRole.Disabled

    fun productionStrategy(): BleRoleStrategy = DisabledBleRoleStrategy
}

internal interface BleRoleStrategy {
    val currentState: TransportState

    fun handle(event: TransportEvent): TransportState
}

internal data object DisabledBleRoleStrategy : BleRoleStrategy {
    override val currentState: TransportState = TransportState.Stopped

    override fun handle(event: TransportEvent): TransportState = currentState
}

/** Implementations must fence all delayed work by the tokens carried in each command. */
internal interface TransportPlatformPort {
    fun scheduleRetry(ticket: RetryTicket)

    fun cancelRetry(ticket: RetryTicket)

    /** Receives only an exact, canonically parsed RPUK v1 Challenge. */
    fun onChallengeFrame(session: TransportSession, challengeFrame: ByteArray)
}

internal interface PhonePeripheralPlatformPort : TransportPlatformPort {
    fun beginGattServerAndAdvertising(operation: TransportOperation)

    fun stopGattServerAndAdvertising(operation: TransportOperation)

    fun disconnectCentral(session: TransportSession)
}

internal interface PhoneCentralPlatformPort : TransportPlatformPort {
    fun beginScanAndConnect(operation: TransportOperation)

    fun cancelScanAndConnect(operation: TransportOperation)

    fun disconnectPeripheral(session: TransportSession)
}

internal abstract class ReducerBackedBleRoleStrategy(
    private val reducer: BleTransportReducer,
) : BleRoleStrategy {
    @Volatile
    final override var currentState: TransportState = TransportState.Stopped
        private set

    @Synchronized
    final override fun handle(event: TransportEvent): TransportState {
        val transition = reducer.reduce(currentState, event)
        currentState = transition.state
        transition.commands.forEach(::execute)
        return currentState
    }

    protected abstract fun execute(command: TransportCommand)
}
