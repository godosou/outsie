package ai.repose.mobile.unlock.protocol

import java.math.BigInteger
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.nio.charset.StandardCharsets

class PairingProtocolFormatException :
    IllegalArgumentException("non-canonical Repose pairing payload v1")

class PairingPayloadV1 internal constructor(
    sessionId: ByteArray,
    val expiresAtEpochMillis: ULong,
    macId: ByteArray,
    macIdentityPublicKey: ByteArray,
    pairingSecret: ByteArray,
    val macName: String,
    encoded: ByteArray,
) {
    private val storedSessionId = sessionId.copyOf()
    private val storedMacId = macId.copyOf()
    private val storedMacIdentityPublicKey = macIdentityPublicKey.copyOf()
    private val storedPairingSecret = pairingSecret.copyOf()
    private val storedEncoding = encoded.copyOf()

    val sessionId: ByteArray
        get() = storedSessionId.copyOf()
    val macId: ByteArray
        get() = storedMacId.copyOf()
    val macIdentityPublicKey: ByteArray
        get() = storedMacIdentityPublicKey.copyOf()
    val pairingSecret: ByteArray
        get() = storedPairingSecret.copyOf()

    fun encoded(): ByteArray = storedEncoding.copyOf()
}

object PairingProtocolV1 {
    const val bleServiceUuid: String = "A53E0001-7A6B-4D59-9F2E-5245504F5345"
    const val bleControlCharacteristicUuid: String = "A53E0002-7A6B-4D59-9F2E-5245504F5345"
    const val bleStatusCharacteristicUuid: String = "A53E0003-7A6B-4D59-9F2E-5245504F5345"

    const val headerLength: Int = 8
    const val minimumFrameLength: Int = 147
    const val maxFrameLength: Int = 210

    fun encode(
        sessionId: ByteArray,
        expiresAtEpochMillis: ULong,
        macId: ByteArray,
        macIdentityPublicKey: ByteArray,
        pairingSecret: ByteArray,
        macName: String,
    ): ByteArray {
        validateFields(
            sessionId,
            expiresAtEpochMillis,
            macId,
            macIdentityPublicKey,
            pairingSecret,
            macName,
        )
        val name = macName.toByteArray(StandardCharsets.UTF_8)
        val payloadLength = fixedPayloadLength + name.size
        val frame = ByteArray(headerLength + payloadLength)
        MAGIC.copyInto(frame, destinationOffset = 0)
        frame[4] = VERSION
        frame[5] = 0
        frame.writeUShort(6, payloadLength)
        sessionId.copyInto(frame, destinationOffset = sessionIdOffset)
        frame.writeULong(expiryOffset, expiresAtEpochMillis)
        macId.copyInto(frame, destinationOffset = macIdOffset)
        macIdentityPublicKey.copyInto(frame, destinationOffset = publicKeyOffset)
        pairingSecret.copyInto(frame, destinationOffset = secretOffset)
        frame[nameLengthOffset] = name.size.toByte()
        name.copyInto(frame, destinationOffset = nameOffset)
        return frame
    }

    fun decode(input: ByteArray): PairingPayloadV1 {
        validateHeader(input)
        val declaredPayloadLength = input.readUShort(6)
        val totalLength = headerLength + declaredPayloadLength
        if (input.size != totalLength) throw PairingProtocolFormatException()

        val nameLength = input[nameLengthOffset].toInt() and 0xff
        if (nameLength !in minimumMacNameLength..maximumMacNameLength) {
            throw PairingProtocolFormatException()
        }
        if (declaredPayloadLength != fixedPayloadLength + nameLength) {
            throw PairingProtocolFormatException()
        }
        val macName = decodeUtf8(input.copyOfRange(nameOffset, input.size))
        val sessionId = input.copyOfRange(sessionIdOffset, expiryOffset)
        val expiresAtEpochMillis = input.readULong(expiryOffset)
        val macId = input.copyOfRange(macIdOffset, publicKeyOffset)
        val macIdentityPublicKey = input.copyOfRange(publicKeyOffset, secretOffset)
        val pairingSecret = input.copyOfRange(secretOffset, nameLengthOffset)
        validateFields(
            sessionId,
            expiresAtEpochMillis,
            macId,
            macIdentityPublicKey,
            pairingSecret,
            macName,
        )
        return PairingPayloadV1(
            sessionId,
            expiresAtEpochMillis,
            macId,
            macIdentityPublicKey,
            pairingSecret,
            macName,
            input,
        )
    }

    private fun validateHeader(input: ByteArray) {
        if (input.size < headerLength) throw PairingProtocolFormatException()
        if (!input.copyOfRange(0, 4).contentEquals(MAGIC)) {
            throw PairingProtocolFormatException()
        }
        if (input[4] != VERSION || input[5].toInt() != 0) {
            throw PairingProtocolFormatException()
        }
        val declaredPayloadLength = input.readUShort(6)
        if (declaredPayloadLength !in minimumPayloadLength..maximumPayloadLength) {
            throw PairingProtocolFormatException()
        }
    }

    private fun validateFields(
        sessionId: ByteArray,
        expiresAtEpochMillis: ULong,
        macId: ByteArray,
        macIdentityPublicKey: ByteArray,
        pairingSecret: ByteArray,
        macName: String,
    ) {
        if (
            sessionId.size != sessionIdLength || sessionId.all { it == 0.toByte() } ||
            expiresAtEpochMillis == 0uL ||
            macId.size != macIdLength || macId.all { it == 0.toByte() } ||
            pairingSecret.size != pairingSecretLength || pairingSecret.all { it == 0.toByte() }
        ) {
            throw PairingProtocolFormatException()
        }
        validateP256Point(macIdentityPublicKey)
        val encodedName = macName.toByteArray(StandardCharsets.UTF_8)
        if (
            encodedName.size !in minimumMacNameLength..maximumMacNameLength ||
            macName.any(Character::isISOControl)
        ) {
            throw PairingProtocolFormatException()
        }
    }

    private fun validateP256Point(input: ByteArray) {
        if (input.size != publicKeyLength || input[0].toInt() and 0xff != 4) {
            throw PairingProtocolFormatException()
        }
        val x = BigInteger(1, input.copyOfRange(1, 33))
        val y = BigInteger(1, input.copyOfRange(33, 65))
        if (x >= P256_PRIME || y >= P256_PRIME) throw PairingProtocolFormatException()
        val left = y.modPow(TWO, P256_PRIME)
        val right = x.modPow(THREE, P256_PRIME)
            .add(P256_A.multiply(x))
            .add(P256_B)
            .mod(P256_PRIME)
        if (left != right) throw PairingProtocolFormatException()
    }

    private fun decodeUtf8(bytes: ByteArray): String = try {
        StandardCharsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(bytes))
            .toString()
    } catch (_: java.nio.charset.CharacterCodingException) {
        throw PairingProtocolFormatException()
    }

    private val MAGIC = byteArrayOf(0x52, 0x50, 0x50, 0x4b)
    private const val VERSION: Byte = 1
    private const val fixedPayloadLength = 138
    private const val minimumPayloadLength = 139
    private const val maximumPayloadLength = 202
    private const val sessionIdLength = 16
    private const val macIdLength = 16
    private const val publicKeyLength = 65
    private const val pairingSecretLength = 32
    private const val minimumMacNameLength = 1
    private const val maximumMacNameLength = 64
    private const val sessionIdOffset = 8
    private const val expiryOffset = 24
    private const val macIdOffset = 32
    private const val publicKeyOffset = 48
    private const val secretOffset = 113
    private const val nameLengthOffset = 145
    private const val nameOffset = 146
    private val TWO = BigInteger.valueOf(2)
    private val THREE = BigInteger.valueOf(3)
    private val P256_PRIME = BigInteger(
        "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff",
        16,
    )
    private val P256_A = P256_PRIME.subtract(THREE)
    private val P256_B = BigInteger(
        "5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b",
        16,
    )
}

private fun ByteArray.readUShort(offset: Int): Int =
    ((this[offset].toInt() and 0xff) shl 8) or (this[offset + 1].toInt() and 0xff)

private fun ByteArray.readULong(offset: Int): ULong {
    var value = 0uL
    repeat(ULong.SIZE_BYTES) { index ->
        value = (value shl 8) or (this[offset + index].toInt() and 0xff).toULong()
    }
    return value
}

private fun ByteArray.writeUShort(offset: Int, value: Int) {
    this[offset] = (value ushr 8).toByte()
    this[offset + 1] = value.toByte()
}

private fun ByteArray.writeULong(offset: Int, value: ULong) {
    repeat(ULong.SIZE_BYTES) { index ->
        this[offset + index] = (value shr ((ULong.SIZE_BYTES - 1 - index) * 8)).toByte()
    }
}
