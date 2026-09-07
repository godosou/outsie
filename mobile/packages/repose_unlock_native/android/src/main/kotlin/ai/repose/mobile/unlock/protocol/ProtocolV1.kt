package ai.repose.mobile.unlock.protocol

import java.math.BigInteger
import java.nio.charset.StandardCharsets
import java.security.MessageDigest

class ProtocolFormatException : IllegalArgumentException("non-canonical Repose protocol v1 frame")

class ChallengeFrame internal constructor(private val frame: ByteArray) {
    val macId: ByteArray
        get() = frame.copyOfRange(12, 28)
    val deviceId: ByteArray
        get() = frame.copyOfRange(28, 44)
    val pairingGeneration: ULong
        get() = frame.readULong(44)
    val consoleUid: UInt
        get() = frame.readUInt(52)
    val auditSessionId: UInt
        get() = frame.readUInt(56)
    val lockEpoch: ULong
        get() = frame.readULong(60)
    val challengeId: ULong
        get() = frame.readULong(68)
    val counterFloor: ULong
        get() = frame.readULong(76)
    val ttlMillis: UInt
        get() = frame.readUInt(84)
    val macNonce: ByteArray
        get() = frame.copyOfRange(88, 120)

    fun hasPairingGenerationBits(expected: Long): Boolean =
        frame.readULong(44).toLong() == expected

    fun encoded(): ByteArray = frame.copyOf()
}

class ResponseFrame internal constructor(private val frame: ByteArray) {
    val pairingGeneration: ULong
        get() = frame.readULong(44)
    val challengeId: ULong
        get() = frame.readULong(68)
    val counter: ULong
        get() = frame.readULong(76)
    val ciphertext: ByteArray
        get() = frame.copyOfRange(278, 310)
    val tag: ByteArray
        get() = frame.copyOfRange(310, 326)
    val signature: ByteArray
        get() = frame.copyOfRange(326, 390)

    fun encoded(): ByteArray = frame.copyOf()
}

object ProtocolV1 {
    const val headerLength: Int = 12
    const val challengePayloadLength: Int = 237
    const val challengeFrameLength: Int = headerLength + challengePayloadLength
    const val responsePayloadLength: Int = 378
    const val responseFrameLength: Int = headerLength + responsePayloadLength
    const val maxFrameLength: Int = responseFrameLength

    internal fun phoneResponseSigningRequest(
        verifiedChallenge: VerifiedMacChallenge,
        responsePrefix: ByteArray,
    ): PhoneResponseSigningRequest {
        val challenge = verifiedChallenge.challengeForResponse()
        validatePhoneResponseSigningPrefix(challenge, responsePrefix)
        val digest = MessageDigest.getInstance("SHA-256").apply {
            update(PHONE_RESPONSE_SIGNATURE_LABEL)
            update(challenge.encoded())
            update(responsePrefix)
        }.digest()
        return PhoneResponseSigningRequest.fromCanonicalPrehash(digest)
    }

    fun decodeChallenge(input: ByteArray): ChallengeFrame {
        validateHeader(input, expectedKind = 1, expectedPayloadLength = challengePayloadLength)
        val ttl = input.readUInt(84)
        if (ttl == 0u || ttl > 60_000u) throw ProtocolFormatException()
        validateP256Point(input, 120)
        validateP256Signature(input, 185)
        return ChallengeFrame(input.copyOf())
    }

    fun decodeResponse(input: ByteArray): ResponseFrame {
        validateHeader(input, expectedKind = 2, expectedPayloadLength = responsePayloadLength)
        validateP256Point(input, 148)
        validateP256Point(input, 213)
        validateP256Signature(input, 326)
        return ResponseFrame(input.copyOf())
    }

    internal fun validatePhoneResponseSigningPrefix(
        challenge: ChallengeFrame,
        responsePrefix: ByteArray,
    ) {
        validateHeaderFields(
            responsePrefix,
            expectedKind = 2,
            expectedPayloadLength = responsePayloadLength,
        )
        if (responsePrefix.size != responseSigningPrefixLength) throw ProtocolFormatException()
        validateP256Point(responsePrefix, 148)
        validateP256Point(responsePrefix, 213)

        val challengeBytes = challenge.encoded()
        val mirrorsIdentityBindingAndChallenge = responsePrefix.copyOfRange(12, 76)
            .contentEquals(challengeBytes.copyOfRange(12, 76))
        val mirrorsMacNonce = responsePrefix.copyOfRange(84, 116)
            .contentEquals(challengeBytes.copyOfRange(88, 120))
        val mirrorsMacEphemeralKey = responsePrefix.copyOfRange(148, 213)
            .contentEquals(challengeBytes.copyOfRange(120, 185))
        val counter = responsePrefix.readULong(76)
        if (
            !mirrorsIdentityBindingAndChallenge ||
            !mirrorsMacNonce ||
            !mirrorsMacEphemeralKey ||
            counter == 0uL ||
            counter <= challenge.counterFloor
        ) {
            throw ProtocolFormatException()
        }
    }

    private fun validateHeader(input: ByteArray, expectedKind: Int, expectedPayloadLength: Int) {
        validateHeaderFields(input, expectedKind, expectedPayloadLength)
        if (input.size != headerLength + expectedPayloadLength) throw ProtocolFormatException()
    }

    private fun validateHeaderFields(
        input: ByteArray,
        expectedKind: Int,
        expectedPayloadLength: Int,
    ) {
        if (input.size < headerLength) throw ProtocolFormatException()
        if (!input.copyOfRange(0, 4).contentEquals(MAGIC)) throw ProtocolFormatException()
        if (input[4].toInt() and 0xff != 1) throw ProtocolFormatException()
        if (input[5].toInt() and 0xff != expectedKind) throw ProtocolFormatException()
        if (input[6].toInt() != 0 || input[7].toInt() != 0) throw ProtocolFormatException()
        val declared = input.readUInt(8).toULong()
        if (declared > responsePayloadLength.toULong()) throw ProtocolFormatException()
        if (declared != expectedPayloadLength.toULong()) throw ProtocolFormatException()
    }

    private fun validateP256Point(input: ByteArray, offset: Int) {
        if (input[offset].toInt() and 0xff != 4) throw ProtocolFormatException()
        val x = BigInteger(1, input.copyOfRange(offset + 1, offset + 33))
        val y = BigInteger(1, input.copyOfRange(offset + 33, offset + 65))
        if (x >= P256_PRIME || y >= P256_PRIME) throw ProtocolFormatException()
        val left = y.modPow(TWO, P256_PRIME)
        val right = x.modPow(THREE, P256_PRIME)
            .add(P256_A.multiply(x))
            .add(P256_B)
            .mod(P256_PRIME)
        if (left != right) throw ProtocolFormatException()
    }

    private fun validateP256Signature(input: ByteArray, offset: Int) {
        val r = BigInteger(1, input.copyOfRange(offset, offset + P256_SCALAR_LENGTH))
        val s = BigInteger(
            1,
            input.copyOfRange(
                offset + P256_SCALAR_LENGTH,
                offset + 2 * P256_SCALAR_LENGTH,
            ),
        )
        if (
            r.signum() == 0 ||
            r >= P256_ORDER ||
            s.signum() == 0 ||
            s >= P256_ORDER ||
            s > P256_HALF_ORDER
        ) {
            throw ProtocolFormatException()
        }
    }

    private val MAGIC = byteArrayOf(0x52, 0x50, 0x55, 0x4b)
    private val PHONE_RESPONSE_SIGNATURE_LABEL =
        "repose-unlock-v1 signature phone-to-mac".toByteArray(StandardCharsets.US_ASCII)
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
    private const val P256_SCALAR_LENGTH = 32
    private val P256_ORDER = BigInteger(
        "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
        16,
    )
    private val P256_HALF_ORDER = P256_ORDER.shiftRight(1)
    private const val responseSigningPrefixLength = 326
}

private fun ByteArray.readUInt(offset: Int): UInt {
    var value = 0u
    repeat(UInt.SIZE_BYTES) { index ->
        value = (value shl 8) or (this[offset + index].toInt() and 0xff).toUInt()
    }
    return value
}

private fun ByteArray.readULong(offset: Int): ULong {
    var value = 0uL
    repeat(ULong.SIZE_BYTES) { index ->
        value = (value shl 8) or (this[offset + index].toInt() and 0xff).toULong()
    }
    return value
}
