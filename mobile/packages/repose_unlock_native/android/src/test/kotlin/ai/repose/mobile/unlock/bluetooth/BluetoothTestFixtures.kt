package ai.repose.mobile.unlock.bluetooth

import java.nio.file.Path

internal object BluetoothTestFixtures {
    fun challenge(): ByteArray = fixtureBytes("challenge_frame")

    fun response(): ByteArray = fixtureBytes("response_frame")

    private fun fixtureBytes(name: String): ByteArray {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val json = root.resolve("protocol/fixtures/v1/crypto-vectors.json").toFile().readText()
        val hex = requireNotNull(
            Regex("\\\"${Regex.escape(name)}\\\"\\s*:\\s*\\\"([0-9a-f]+)\\\"")
                .find(json)
                ?.groupValues
                ?.get(1),
        ) { "missing fixture field $name" }
        return hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }
}
