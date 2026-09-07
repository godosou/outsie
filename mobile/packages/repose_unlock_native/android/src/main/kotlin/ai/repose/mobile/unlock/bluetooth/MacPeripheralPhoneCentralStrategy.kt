package ai.repose.mobile.unlock.bluetooth

internal class MacPeripheralPhoneCentralStrategy(
    private val port: PhoneCentralPlatformPort,
    reducer: BleTransportReducer = BleTransportReducer(),
) : ReducerBackedBleRoleStrategy(reducer) {
    override fun execute(command: TransportCommand) {
        when (command) {
            is TransportCommand.OpenLink -> port.beginScanAndConnect(command.operation)
            is TransportCommand.CancelOpen -> port.cancelScanAndConnect(command.operation)
            is TransportCommand.CloseLink -> port.disconnectPeripheral(command.session)
            is TransportCommand.ScheduleRetry -> port.scheduleRetry(command.ticket)
            is TransportCommand.CancelRetry -> port.cancelRetry(command.ticket)
            is TransportCommand.ChallengeReady -> port.onChallengeFrame(
                command.session,
                command.copyChallengeForNativeConsumer(),
            )
        }
    }
}
