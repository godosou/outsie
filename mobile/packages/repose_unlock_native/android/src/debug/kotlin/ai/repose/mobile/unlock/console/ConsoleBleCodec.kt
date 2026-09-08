package ai.repose.mobile.unlock.console

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.MessageDigest
import javax.crypto.Cipher
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

internal class ConsoleBleCipher(secret: ByteArray, private val session: ByteArray, private val challenge: ByteArray) {
    private val key: ByteArray
    private var sent = 0L
    private var received = 0L
    private var closed = false
    init {
        require(secret.size == 32 && session.size == 16 && challenge.size == 16)
        val extract = Mac.getInstance("HmacSHA256")
        extract.init(SecretKeySpec(challenge, "HmacSHA256"))
        val prk = extract.doFinal(secret)
        val expand = Mac.getInstance("HmacSHA256")
        expand.init(SecretKeySpec(prk, "HmacSHA256"))
        key = expand.doFinal("repose-console-ble-v1".toByteArray(Charsets.UTF_8) + byteArrayOf(1))
        prk.fill(0)
    }
    fun encrypt(plaintext: ByteArray): ByteArray {
        require(!closed && plaintext.size <= MAX_PACKET - 58 && sent < Long.MAX_VALUE)
        val counter = sent + 1
        val header = header(0, counter)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, nonce(0, counter)))
        cipher.updateAAD(header)
        val packet = header + cipher.doFinal(plaintext)
        sent = counter
        return packet
    }
    fun decrypt(packet: ByteArray): ByteArray {
        require(!closed && packet.size in 58..MAX_PACKET && received < Long.MAX_VALUE)
        val counter = received + 1
        val expectedHeader = header(1, counter)
        require(MessageDigest.isEqual(packet.copyOfRange(0, 42), expectedHeader))
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, nonce(1, counter)))
        cipher.updateAAD(expectedHeader)
        val plain = cipher.doFinal(packet.copyOfRange(42, packet.size))
        received = counter // Authentication failure never consumes the counter.
        return plain
    }
    fun close() { closed = true; key.fill(0); session.fill(0); challenge.fill(0) }
    private fun header(direction: Int, counter: Long) = ByteBuffer.allocate(42).order(ByteOrder.BIG_ENDIAN)
        .put(1).put(direction.toByte()).put(session).put(challenge).putLong(counter).array()
    private fun nonce(direction: Int, counter: Long) = ByteBuffer.allocate(12).order(ByteOrder.BIG_ENDIAN)
        .put(direction.toByte()).put(byteArrayOf(0, 0, 0)).putLong(counter).array()
    companion object { const val MAX_PACKET = 256 * 1024 }
}

internal object ConsoleBleFragments {
    fun encode(messageId: Int, packet: ByteArray, attributeLimit: Int): List<ByteArray> {
        require(packet.isNotEmpty() && packet.size <= ConsoleBleCipher.MAX_PACKET && attributeLimit in 20..512)
        val size = attributeLimit - 9
        val count = (packet.size + size - 1) / size
        require(count <= 24000)
        return (0 until count).map { index ->
            ByteBuffer.allocate(9 + minOf(size, packet.size - index * size)).order(ByteOrder.BIG_ENDIAN)
                .put(0xC1.toByte()).putInt(messageId).putShort(index.toShort()).putShort(count.toShort())
                .put(packet, index * size, minOf(size, packet.size - index * size)).array()
        }
    }
    class Receiver {
        private var id = 0
        private var count = 0
        private var next = 0
        private val bytes = ByteArrayOutputStream()
        fun accept(value: ByteArray, expectedMessageId: Int): ByteArray? {
            require(value.size in 10..512)
            val header = ByteBuffer.wrap(value).order(ByteOrder.BIG_ENDIAN)
            require(header.get() == 0xC1.toByte())
            val messageId = header.int
            val index = header.short.toInt() and 0xffff
            val total = header.short.toInt() and 0xffff
            require(messageId == expectedMessageId && total in 1..24000 && index < total)
            if (index == 0) { reset(); id = messageId; count = total }
            require(messageId == id && total == count && index == next)
            require(bytes.size() + value.size - 9 <= ConsoleBleCipher.MAX_PACKET)
            bytes.write(value, 9, value.size - 9)
            next++
            if (next != count) return null
            return bytes.toByteArray().also { reset() }
        }
        fun reset() { id = 0; count = 0; next = 0; bytes.reset() }
    }
}
