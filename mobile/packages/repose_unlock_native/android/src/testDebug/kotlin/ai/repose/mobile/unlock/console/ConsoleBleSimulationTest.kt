package ai.repose.mobile.unlock.console

import java.nio.file.Path
import java.util.Base64
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test

/** Runs the production Kotlin cipher/fragment codecs against the Rust service,
 * with a fake keyboard and stdio radio. Never opens Bluetooth or network sockets. */
class ConsoleBleSimulationTest {
    @Test fun `Kotlin phone and Rust Mac execute the Bluetooth console protocol without hardware`() {
        val workspace = Path.of(System.getProperty("repose.workspaceRoot")).toFile()
        val binary = Path.of(workspace.path, "src-tauri/target/debug/examples/console_ble_simulation").toFile()
        assumeTrue("Build the Rust console_ble_simulation example to run cross-runtime simulation", binary.canExecute())
        val process = ProcessBuilder(binary.path).directory(workspace).redirectError(ProcessBuilder.Redirect.INHERIT).start()
        val writer = process.outputStream.bufferedWriter()
        val reader = process.inputStream.bufferedReader()
        val executor = Executors.newSingleThreadExecutor()
        fun exchange(message: String): String {
            writer.write(message); writer.newLine(); writer.flush()
            return executor.submit<String> { reader.readLine() ?: error("Simulator closed unexpectedly") }.get(10, TimeUnit.SECONDS)
        }
        fun data(envelope: String): ByteArray = Base64.getDecoder().decode(
            Regex("\"data\"\\s*:\\s*\"([^\"]+)\"").find(envelope)?.groupValues?.get(1) ?: error("Missing simulation packet: ${envelope.take(120)}")
        )
        try {
            val subscribed = exchange("{\"event\":\"subscribe\",\"central\":\"sim\"}")
            val challenge = data(subscribed)
            var cipher = ConsoleBleCipher(ByteArray(32) { 2 }, ByteArray(16) { 1 }, challenge)
            var id = 0
            var lastFrame: ByteArray? = null
            fun rpc(json: String): String {
                id++
                val encrypted = cipher.encrypt(json.toByteArray(Charsets.UTF_8))
                // Simulate ATT writes at minimum MTU23 and notifications at MTU67.
                val macReceiver = ConsoleBleFragments.Receiver()
                var wire: ByteArray? = null
                ConsoleBleFragments.encode(id, encrypted, 20).forEach { wire = macReceiver.accept(it, id) }
                assertArrayEquals(encrypted, wire)
                lastFrame = wire
                val event = exchange("{\"event\":\"frame\",\"central\":\"sim\",\"data\":\"${Base64.getEncoder().encodeToString(wire)}\"}")
                val phoneReceiver = ConsoleBleFragments.Receiver()
                var response: ByteArray? = null
                ConsoleBleFragments.encode(id, data(event), 64).forEach { response = phoneReceiver.accept(it, id) }
                return cipher.decrypt(requireNotNull(response)).toString(Charsets.UTF_8)
            }
            fun success(json: String) { assertTrue(json, Regex("\"ok\"\\s*:\\s*true").containsMatchIn(json)) }
            val first = rpc("{\"type\":\"status\"}")
            success(first)
            assertTrue(first.contains("\"config\""))
            val revision = Regex("\"revision\"\\s*:\\s*(\\d+)").find(first)!!.groupValues[1].toInt()
            fun keyCount(): Int = Regex("\"key\"\\s*:").findAll(exchange("{\"event\":\"state\"}")).count()

            success(rpc("{\"type\":\"activate\",\"requestId\":\"sim-activate\",\"appId\":\"tmux\"}"))
            success(rpc("{\"type\":\"execute\",\"requestId\":\"sim-execute\",\"appId\":\"tmux\",\"actionId\":\"split-horizontal\"}"))
            Thread.sleep(80)
            assertEquals(1, keyCount())
            val cancelled = rpc("{\"type\":\"cancel\",\"requestId\":\"sim-cancel\"}")
            success(cancelled)
            Thread.sleep(550)
            assertEquals(1, keyCount())
            val stopped = rpc("{\"type\":\"status\"}")
            assertTrue(stopped, Regex("\"running\"\\s*:\\s*false").containsMatchIn(stopped))
            val reordered = rpc("{\"type\":\"reorder\",\"requestId\":\"sim-reorder\",\"appId\":\"tmux\",\"revision\":$revision,\"actionIds\":[\"workspace\",\"new-window\",\"close-pane\",\"zoom\",\"next-pane\",\"split-vertical\",\"split-horizontal\"]}")
            success(reordered)
            assertTrue(Regex("\"revision\"\\s*:\\s*${revision + 1}").containsMatchIn(reordered))
            val unchanged = rpc("{\"type\":\"status\",\"knownRevision\":${revision + 1}}")
            success(unchanged)
            assertFalse(unchanged.contains("\"config\""))
            val duplicate = rpc("{\"type\":\"cancel\",\"requestId\":\"sim-cancel\"}")
            assertTrue(Regex("\"ok\"\\s*:\\s*false").containsMatchIn(duplicate))
            success(rpc("{\"type\":\"execute\",\"requestId\":\"sim-disconnect-run\",\"appId\":\"tmux\",\"actionId\":\"split-horizontal\"}"))
            Thread.sleep(80)
            assertEquals(2, keyCount())
            exchange("{\"event\":\"disconnect\"}")
            Thread.sleep(550)
            assertEquals(2, keyCount())
            val nextChallenge = data(exchange("{\"event\":\"subscribe\",\"central\":\"sim\"}"))
            val replay = exchange("{\"event\":\"frame\",\"central\":\"sim\",\"data\":\"${Base64.getEncoder().encodeToString(lastFrame)}\"}")
            assertTrue(replay, replay.contains("error") || replay.contains("rejected"))
            cipher.close()
            cipher = ConsoleBleCipher(ByteArray(32) { 2 }, ByteArray(16) { 1 }, nextChallenge)
            id = 0
            success(rpc("{\"type\":\"status\"}"))
            success(rpc("{\"type\":\"execute\",\"requestId\":\"sim-complete-sequence\",\"appId\":\"tmux\",\"actionId\":\"split-horizontal\"}"))
            Thread.sleep(80)
            assertEquals(3, keyCount())
            Thread.sleep(550)
            val completedState = exchange("{\"event\":\"state\"}")
            assertEquals(4, Regex("\"key\"\\s*:").findAll(completedState).count())
            val keyTimes = Regex("\"atMs\"\\s*:\\s*(\\d+)").findAll(completedState).map { it.groupValues[1].toLong() }.toList()
            assertEquals(4, keyTimes.size)
            assertTrue("Configured 500ms delay must separate key events", keyTimes.last() - keyTimes[keyTimes.lastIndex - 1] >= 500)
            val completed = rpc("{\"type\":\"status\"}")
            success(completed)
            assertTrue(completed, Regex("\"running\"\\s*:\\s*false").containsMatchIn(completed))
            exchange("{\"event\":\"revoke\"}")
            val revokedPacket = cipher.encrypt("{\"type\":\"status\"}".toByteArray())
            val revoked = exchange("{\"event\":\"frame\",\"central\":\"sim\",\"data\":\"${Base64.getEncoder().encodeToString(revokedPacket)}\"}")
            assertTrue(revoked, revoked.contains("error"))
            cipher.close()
        } finally {
            writer.close()
            if (!process.waitFor(3, TimeUnit.SECONDS)) process.destroyForcibly()
            executor.shutdownNow()
            reader.close()
        }
    }
}
