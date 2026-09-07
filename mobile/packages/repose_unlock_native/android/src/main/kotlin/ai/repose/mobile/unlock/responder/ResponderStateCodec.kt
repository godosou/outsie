package ai.repose.mobile.unlock.responder

import ai.repose.mobile.unlock.protocol.ProtocolV1
import java.math.BigInteger
import java.nio.charset.StandardCharsets
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.MessageDigest
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec

internal object ResponderStateCodec {
    fun encode(record: DurableResponderRecord): ByteArray {
        val kind = when (record) {
            is DurableResponderRecord.Active -> when (record.responseState) {
                is DurableResponseState.Ready -> KIND_READY
                is DurableResponseState.Cached -> KIND_CACHED
            }
            is DurableResponderRecord.Revoked -> KIND_REVOKED
        }
        val bodyLength = when (kind) {
            KIND_READY -> READY_BODY_LENGTH
            KIND_CACHED -> CACHED_BODY_LENGTH
            KIND_REVOKED -> REVOKED_BODY_LENGTH
            else -> throw ResponderStoreException()
        }
        val withoutChecksum = ByteArray(HEADER_LENGTH + bodyLength)
        MAGIC.copyInto(withoutChecksum)
        withoutChecksum[4] = VERSION
        withoutChecksum[5] = kind
        withoutChecksum.writeUInt(8, bodyLength.toUInt())
        record.macId.copyInto(withoutChecksum, destinationOffset = 12)
        record.deviceId.copyInto(withoutChecksum, destinationOffset = 28)
        withoutChecksum.writeULong(44, record.pairingGeneration)

        if (record is DurableResponderRecord.Active) {
            encodeP256PublicKey(record.pairing.macIdentityPublicKey)
                .copyInto(withoutChecksum, destinationOffset = 52)
            encodeP256PublicKey(record.pairing.phoneIdentityPublicKey)
                .copyInto(withoutChecksum, destinationOffset = 117)
            withoutChecksum.writeULong(182, record.responseState.counter)
            if (record.responseState is DurableResponseState.Cached) {
                val cached = record.responseState
                withoutChecksum.writeUInt(190, cached.consoleUid)
                withoutChecksum.writeUInt(194, cached.auditSessionId)
                withoutChecksum.writeULong(198, cached.lockEpoch)
                withoutChecksum.writeULong(206, cached.challengeId)
                cached.challengeFingerprint.copyInto(withoutChecksum, destinationOffset = 214)
                cached.response.copyInto(withoutChecksum, destinationOffset = 246)
            }
        }
        return withoutChecksum + checksum(withoutChecksum)
    }

    fun decode(encoded: ByteArray): DurableResponderRecord {
        try {
            if (encoded.size < HEADER_LENGTH + CHECKSUM_LENGTH) fail()
            if (!encoded.copyOfRange(0, 4).contentEquals(MAGIC) ||
                encoded[4] != VERSION ||
                encoded[6] != 0.toByte() ||
                encoded[7] != 0.toByte()
            ) {
                fail()
            }
            val kind = encoded[5]
            val bodyLength = encoded.readUInt(8).toInt()
            val expectedBodyLength = when (kind) {
                KIND_READY -> READY_BODY_LENGTH
                KIND_CACHED -> CACHED_BODY_LENGTH
                KIND_REVOKED -> REVOKED_BODY_LENGTH
                else -> fail()
            }
            if (bodyLength != expectedBodyLength ||
                encoded.size != HEADER_LENGTH + bodyLength + CHECKSUM_LENGTH
            ) {
                fail()
            }
            val payloadEnd = HEADER_LENGTH + bodyLength
            val actualChecksum = encoded.copyOfRange(payloadEnd, encoded.size)
            if (!MessageDigest.isEqual(actualChecksum, checksum(encoded.copyOfRange(0, payloadEnd)))) {
                fail()
            }

            val macId = encoded.copyOfRange(12, 28)
            val deviceId = encoded.copyOfRange(28, 44)
            val generation = encoded.readULong(44)
            if (kind == KIND_REVOKED) {
                return DurableResponderRecord.Revoked(macId, deviceId, generation)
            }
            val pairing = TrustedPairedMac(
                macId,
                deviceId,
                generation,
                decodeP256PublicKey(encoded.copyOfRange(52, 117)),
                decodeP256PublicKey(encoded.copyOfRange(117, 182)),
            )
            val counter = encoded.readULong(182)
            if (kind == KIND_READY) {
                return DurableResponderRecord.Active(pairing, DurableResponseState.Ready(counter))
            }
            val cached = DurableResponseState.Cached(
                counter = counter,
                consoleUid = encoded.readUInt(190),
                auditSessionId = encoded.readUInt(194),
                lockEpoch = encoded.readULong(198),
                challengeId = encoded.readULong(206),
                challengeFingerprint = encoded.copyOfRange(214, 246),
                response = encoded.copyOfRange(246, 636),
            )
            validateCachedRecord(pairing, cached)
            return DurableResponderRecord.Active(pairing, cached)
        } catch (exception: ResponderStoreException) {
            throw exception
        } catch (_: Exception) {
            throw ResponderStoreException()
        }
    }

    private fun validateCachedRecord(pairing: TrustedPairedMac, cached: DurableResponseState.Cached) {
        val response = cached.response
        val parsed = ProtocolV1.decodeResponse(response)
        if (!response.copyOfRange(12, 28).contentEquals(pairing.macId) ||
            !response.copyOfRange(28, 44).contentEquals(pairing.deviceId) ||
            parsed.pairingGeneration != pairing.pairingGeneration ||
            response.readUInt(52) != cached.consoleUid ||
            response.readUInt(56) != cached.auditSessionId ||
            response.readULong(60) != cached.lockEpoch ||
            parsed.challengeId != cached.challengeId ||
            parsed.counter != cached.counter
        ) {
            fail()
        }
    }

    private fun checksum(encoded: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").run {
        update(CHECKSUM_LABEL)
        update(encoded)
        digest()
    }

    private fun fail(): Nothing = throw ResponderStoreException()

    private const val HEADER_LENGTH = 12
    private const val CHECKSUM_LENGTH = 32
    private const val READY_BODY_LENGTH = 178
    private const val CACHED_BODY_LENGTH = 624
    private const val REVOKED_BODY_LENGTH = 40
    private const val VERSION: Byte = 1
    private const val KIND_READY: Byte = 1
    private const val KIND_CACHED: Byte = 2
    private const val KIND_REVOKED: Byte = 3
    private val MAGIC = byteArrayOf(0x52, 0x50, 0x53, 0x44)
    private val CHECKSUM_LABEL =
        "repose-unlock-android durable-responder-state v1"
            .toByteArray(StandardCharsets.US_ASCII)
}

private fun decodeP256PublicKey(encoded: ByteArray): java.security.PublicKey {
    if (encoded.size != 65 || encoded[0] != 4.toByte()) throw ResponderStoreException()
    val point = ECPoint(
        BigInteger(1, encoded.copyOfRange(1, 33)),
        BigInteger(1, encoded.copyOfRange(33, 65)),
    )
    val publicKey = KeyFactory.getInstance("EC").generatePublic(
        ECPublicKeySpec(point, P256_PARAMETERS_FOR_STORE),
    )
    requireP256PublicKey(publicKey)
    return publicKey
}

private fun ByteArray.writeUInt(offset: Int, value: UInt) {
    repeat(UInt.SIZE_BYTES) { index ->
        this[offset + index] = (value shr (8 * (UInt.SIZE_BYTES - 1 - index))).toByte()
    }
}

private fun ByteArray.writeULong(offset: Int, value: ULong) {
    repeat(ULong.SIZE_BYTES) { index ->
        this[offset + index] = (value shr (8 * (ULong.SIZE_BYTES - 1 - index))).toByte()
    }
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

private val P256_PARAMETERS_FOR_STORE: ECParameterSpec = AlgorithmParameters.getInstance("EC").run {
    init(ECGenParameterSpec("secp256r1"))
    getParameterSpec(ECParameterSpec::class.java)
}
