package ai.repose.blespike

import java.util.UUID

/** Fixed contract shared with the macOS central spike. Do not change either side alone. */
object SpikeContract {
    val SERVICE_UUID: UUID = UUID.fromString("7265706F-7365-0001-8000-00805F9B34FB")
    val CHARACTERISTIC_UUID: UUID = UUID.fromString("7265706F-7365-0002-8000-00805F9B34FB")
    const val PAYLOAD = "repose-hello"
}
