package ai.repose.mobile.unlock.crypto

import java.math.BigInteger

internal object P256SignatureCodec {
    fun canonicalRawFromDer(der: ByteArray): ByteArray {
        val reader = DerReader(der)
        reader.requireTag(SEQUENCE_TAG)
        val sequenceLength = reader.readLength()
        require(sequenceLength == reader.remaining) { "trailing or truncated ECDSA sequence" }
        val r = reader.readPositiveScalar()
        val s = reader.readPositiveScalar()
        require(reader.remaining == 0) { "trailing ECDSA data" }

        val canonicalS = if (s > P256_HALF_ORDER) P256_ORDER.subtract(s) else s
        return r.toFixedWidth() + canonicalS.toFixedWidth()
    }

    fun derFromRaw(raw: ByteArray): ByteArray {
        require(raw.size == RAW_SIGNATURE_LENGTH) { "P-256 raw signature must be 64 bytes" }
        val r = BigInteger(1, raw.copyOfRange(0, SCALAR_LENGTH))
        val s = BigInteger(1, raw.copyOfRange(SCALAR_LENGTH, RAW_SIGNATURE_LENGTH))
        requireCanonicalScalar(r)
        requireCanonicalScalar(s)
        require(s <= P256_HALF_ORDER) { "P-256 signature must use low-S form" }

        val encodedR = r.toDerInteger()
        val encodedS = s.toDerInteger()
        val bodyLength = 2 + encodedR.size + 2 + encodedS.size
        return byteArrayOf(
            SEQUENCE_TAG.toByte(),
            bodyLength.toByte(),
            INTEGER_TAG.toByte(),
            encodedR.size.toByte(),
        ) + encodedR + byteArrayOf(INTEGER_TAG.toByte(), encodedS.size.toByte()) + encodedS
    }

    private fun requireCanonicalScalar(value: BigInteger) {
        require(value.signum() > 0 && value < P256_ORDER) {
            "ECDSA scalar is outside the P-256 group order"
        }
    }

    private fun BigInteger.toFixedWidth(): ByteArray {
        requireCanonicalScalar(this)
        val signed = toByteArray()
        val unsigned = if (signed.size == SCALAR_LENGTH + 1 && signed[0] == 0.toByte()) {
            signed.copyOfRange(1, signed.size)
        } else {
            signed
        }
        require(unsigned.size <= SCALAR_LENGTH)
        return ByteArray(SCALAR_LENGTH).also { fixed ->
            unsigned.copyInto(fixed, destinationOffset = SCALAR_LENGTH - unsigned.size)
        }
    }

    private fun BigInteger.toDerInteger(): ByteArray {
        requireCanonicalScalar(this)
        return toByteArray()
    }

    private class DerReader(private val encoded: ByteArray) {
        private var cursor = 0

        val remaining: Int
            get() = encoded.size - cursor

        fun requireTag(expected: Int) {
            require(readByte() == expected) { "unexpected ECDSA DER tag" }
        }

        fun readLength(): Int {
            val length = readByte()
            require(length < LONG_FORM_LENGTH_BIT) { "non-canonical ECDSA DER length" }
            require(length <= remaining) { "truncated ECDSA DER value" }
            return length
        }

        fun readPositiveScalar(): BigInteger {
            requireTag(INTEGER_TAG)
            val length = readLength()
            require(length in 1..SCALAR_LENGTH + 1) { "invalid ECDSA integer length" }
            val integer = readBytes(length)
            require(integer[0].toInt() and SIGN_BIT == 0) { "negative ECDSA integer" }
            if (integer.size > 1 && integer[0] == 0.toByte()) {
                require(integer[1].toInt() and SIGN_BIT != 0) {
                    "redundant ECDSA integer padding"
                }
            }
            val scalar = BigInteger(1, integer)
            requireCanonicalScalar(scalar)
            return scalar
        }

        private fun readByte(): Int {
            require(cursor < encoded.size) { "truncated ECDSA DER signature" }
            return encoded[cursor++].toInt() and 0xff
        }

        private fun readBytes(length: Int): ByteArray {
            require(length <= remaining) { "truncated ECDSA DER integer" }
            return encoded.copyOfRange(cursor, cursor + length).also { cursor += length }
        }
    }

    private const val SEQUENCE_TAG = 0x30
    private const val INTEGER_TAG = 0x02
    private const val LONG_FORM_LENGTH_BIT = 0x80
    private const val SIGN_BIT = 0x80
    private const val SCALAR_LENGTH = 32
    private const val RAW_SIGNATURE_LENGTH = 2 * SCALAR_LENGTH
    private val P256_ORDER = BigInteger(
        "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
        16,
    )
    private val P256_HALF_ORDER = P256_ORDER.shiftRight(1)
}
