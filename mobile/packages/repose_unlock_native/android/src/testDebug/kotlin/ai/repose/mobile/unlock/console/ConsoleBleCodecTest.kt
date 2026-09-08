package ai.repose.mobile.unlock.console

import java.nio.file.Path
import java.util.Base64
import org.junit.Assert.*
import org.junit.Test

class ConsoleBleCodecTest {
    @Test fun `AES GCM HKDF request matches independently generated Node vector`() {
        val fixture = Path.of(System.getProperty("repose.workspaceRoot"), "protocol/fixtures/console-ble-v1.json").toFile().readText()
        fun hex(key: String): ByteArray = Regex("\"$key\"\\s*:\\s*\"([0-9a-f]+)\"").find(fixture)!!.groupValues[1].hexBytes()
        val codec = ConsoleBleCipher(hex("secretHex"), hex("sessionHex"), hex("challengeHex"))
        assertArrayEquals(hex("requestHex"), codec.encrypt("{\"type\":\"status\"}".toByteArray()))
        codec.close()
    }
    @Test fun `response vectors reject replay and forged tag without consuming counter`() {
        val fixture = Path.of(System.getProperty("repose.workspaceRoot"), "protocol/fixtures/console-ble-v1.json").toFile().readText()
        fun hex(key: String): ByteArray = Regex("\"$key\"\\s*:\\s*\"([0-9a-f]+)\"").find(fixture)!!.groupValues[1].hexBytes()
        val codec = ConsoleBleCipher(hex("secretHex"), hex("sessionHex"), hex("challengeHex"))
        val response = hex("responseHex")
        val forged = response.copyOf().also { it[it.lastIndex] = (it.last().toInt() xor 1).toByte() }
        assertThrows(javax.crypto.AEADBadTagException::class.java) { codec.decrypt(forged) }
        assertEquals("{\"ok\":true}", codec.decrypt(response).toString(Charsets.UTF_8))
        assertThrows(IllegalArgumentException::class.java) { codec.decrypt(response) }
        assertEquals("{\"ok\":true}", codec.decrypt(hex("response2Hex")).toString(Charsets.UTF_8))
        codec.close()
        assertThrows(IllegalArgumentException::class.java) { codec.encrypt(byteArrayOf(1)) }
    }

    @Test fun `fragmentation handles minimum and maximum ATT limits and large config`() {
        for (limit in listOf(20, 244, 512)) {
            val original = ByteArray(256 * 1024) { (it % 251).toByte() }
            val fragments = ConsoleBleFragments.encode(99, original, limit)
            val receiver = ConsoleBleFragments.Receiver()
            var actual: ByteArray? = null
            fragments.forEach { assertTrue(it.size <= limit); actual = receiver.accept(it, 99) }
            assertArrayEquals(original, actual)
        }
    }
    @Test fun `fragmentation rejects reordered duplicate and cross request fragments`() {
        val fragments = ConsoleBleFragments.encode(7, ByteArray(2000), 512)
        val receiver = ConsoleBleFragments.Receiver()
        assertThrows(IllegalArgumentException::class.java) { receiver.accept(fragments[1], 7) }
        assertNull(receiver.accept(fragments[0], 7))
        assertThrows(IllegalArgumentException::class.java) { receiver.accept(fragments[2], 7) }
        assertThrows(IllegalArgumentException::class.java) { receiver.accept(fragments[1], 8) }
        assertNull(receiver.accept(fragments[0], 7))
        assertNull(receiver.accept(fragments[1], 7))
        assertThrows(IllegalArgumentException::class.java) { receiver.accept(fragments[1], 7) }
    }
    @Test fun `response header must match direction session challenge and counter`() {
        val codec = ConsoleBleCipher(ByteArray(32) { 2 }, ByteArray(16) { 1 }, ByteArray(16) { 3 })
        val request = codec.encrypt("{}".toByteArray())
        assertThrows(IllegalArgumentException::class.java) { codec.decrypt(request) }
        assertThrows(IllegalArgumentException::class.java) { codec.decrypt(ByteArray(58)) }
        codec.close()
    }
}
internal fun String.hexBytes(): ByteArray = chunked(2).map { it.toInt(16).toByte() }.toByteArray()
