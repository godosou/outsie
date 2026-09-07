package ai.repose.mobile.unlock.bluetooth

internal class MacCentralPhonePeripheralStrategy(
    private val port: PhonePeripheralPlatformPort,
    reducer: BleTransportReducer = BleTransportReducer(),
) : ReducerBackedBleRoleStrategy(reducer) {
    override fun execute(command: TransportCommand) {
        when (command) {
            is TransportCommand.OpenLink -> port.beginGattServerAndAdvertising(command.operation)
            is TransportCommand.CancelOpen -> port.stopGattServerAndAdvertising(command.operation)
            is TransportCommand.CloseLink -> port.disconnectCentral(command.session)
            is TransportCommand.ScheduleRetry -> port.scheduleRetry(command.ticket)
            is TransportCommand.CancelRetry -> port.cancelRetry(command.ticket)
            is TransportCommand.ChallengeReady -> port.onChallengeFrame(
                command.session,
                command.copyChallengeForNativeConsumer(),
            )
        }
    }
}
