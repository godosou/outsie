package ai.repose.mobile.unlock.bluetooth

import ai.repose.mobile.unlock.protocol.ProtocolV1

internal class FragmentFormatException : IllegalArgumentException(
    "non-canonical Repose GATT fragment",
)

internal sealed interface GattReassembly {
    data object Empty : GattReassembly

    class Receiving internal constructor(
        internal val declaredTotal: Int,
        internal val nextSequence: Int,
        payload: ByteArray,
    ) : GattReassembly {
        private val payload: ByteArray = payload.copyOf()

        internal fun copyPayloadForCodec(): ByteArray = payload.copyOf()
    }

    class Complete internal constructor(frame: ByteArray) : GattReassembly {
        private val frame: ByteArray = frame.copyOf()

        internal fun copyFrameForCodec(): ByteArray = frame.copyOf()
    }
}

/**
 * Canonical envelope for transporting an unchanged RPUK v1 frame over GATT.
 *
 * Header (network byte order): magic[2], version[1], flags[1], total[2],
 * sequence[2], payloadLength[2]. Fragment metadata never becomes part of the
 * authenticated protocol frame.
 */
internal object GattFragmentCodec {
    const val headerLength: Int = 10
    const val maxAttributeValueBytes: Int = 512
    const val maxFrameLength: Int = ProtocolV1.maxFrameLength

    private const val version: Int = 1
    private const val lastFlag: Int = 1
    private const val maxSequence: Int = 0xffff
    private val magic = byteArrayOf(0x52, 0x46)

    /**
     * [maximumAttributeValueBytes] is the already-derived characteristic value
     * bound (for example MTU minus ATT overhead), never the negotiated MTU.
     */
    fun encode(frame: ByteArray, maximumAttributeValueBytes: Int): List<ByteArray> {
        validateFrame(frame)
        require(maximumAttributeValueBytes in (headerLength + 1)..maxAttributeValueBytes) {
            "GATT attribute-value bound is outside the supported range"
        }
        val maximumPayload = maximumAttributeValueBytes - headerLength
        val payloadSizes = buildList {
            var remaining = frame.size
            while (remaining > 0) {
                val size = minOf(remaining, maximumPayload)
                add(size)
                remaining -= size
            }
        }
        return encodeWithPayloadSizes(frame, payloadSizes)
    }

    internal fun encodeWithPayloadSizes(
        frame: ByteArray,
        payloadSizes: List<Int>,
    ): List<ByteArray> {
        validateFrame(frame)
        require(payloadSizes.isNotEmpty()) { "at least one payload slice is required" }
        require(payloadSizes.size <= maxSequence + 1) { "too many fragments" }
        require(payloadSizes.all { it in 1..(maxAttributeValueBytes - headerLength) }) {
            "fragment payload is outside the supported range"
        }
        require(payloadSizes.sum() == frame.size) { "payload slices must cover the frame exactly" }

        var offset = 0
        return payloadSizes.mapIndexed { sequence, payloadSize ->
            val isLast = sequence == payloadSizes.lastIndex
            ByteArray(headerLength + payloadSize).also { packet ->
                packet[0] = magic[0]
                packet[1] = magic[1]
                packet[2] = version.toByte()
                packet[3] = if (isLast) lastFlag.toByte() else 0
                packet.writeUnsignedShort(4, frame.size)
                packet.writeUnsignedShort(6, sequence)
                packet.writeUnsignedShort(8, payloadSize)
                frame.copyInto(
                    destination = packet,
                    destinationOffset = headerLength,
                    startIndex = offset,
                    endIndex = offset + payloadSize,
                )
                offset += payloadSize
            }
        }
    }

    fun accept(current: GattReassembly, packet: ByteArray): GattReassembly {
        if (current is GattReassembly.Complete) throw FragmentFormatException()
        if (packet.size !in (headerLength + 1)..maxAttributeValueBytes) {
            throw FragmentFormatException()
        }
        if (packet[0] != magic[0] || packet[1] != magic[1]) throw FragmentFormatException()
        if (packet[2].toInt() and 0xff != version) throw FragmentFormatException()
        val flags = packet[3].toInt() and 0xff
        if (flags and lastFlag.inv() != 0) throw FragmentFormatException()

        val declaredTotal = packet.readUnsignedShort(4)
        val sequence = packet.readUnsignedShort(6)
        val declaredPayload = packet.readUnsignedShort(8)
        if (declaredTotal !in 1..maxFrameLength) throw FragmentFormatException()
        if (declaredPayload == 0 || packet.size != headerLength + declaredPayload) {
            throw FragmentFormatException()
        }

        val previousPayload: ByteArray
        val expectedSequence: Int
        when (current) {
            GattReassembly.Empty -> {
                previousPayload = byteArrayOf()
                expectedSequence = 0
            }

            is GattReassembly.Receiving -> {
                if (current.declaredTotal != declaredTotal) throw FragmentFormatException()
                previousPayload = current.copyPayloadForCodec()
                expectedSequence = current.nextSequence
            }

            is GattReassembly.Complete -> error("handled above")
        }
        if (sequence != expectedSequence) throw FragmentFormatException()
        if (previousPayload.size > declaredTotal - declaredPayload) throw FragmentFormatException()

        val combined = ByteArray(previousPayload.size + declaredPayload)
        previousPayload.copyInto(combined)
        packet.copyInto(
            destination = combined,
            destinationOffset = previousPayload.size,
            startIndex = headerLength,
        )
        val declaredLast = flags == lastFlag
        val actuallyComplete = combined.size == declaredTotal
        if (declaredLast != actuallyComplete) throw FragmentFormatException()

        return if (actuallyComplete) {
            GattReassembly.Complete(combined)
        } else {
            if (expectedSequence == maxSequence) throw FragmentFormatException()
            GattReassembly.Receiving(declaredTotal, expectedSequence + 1, combined)
        }
    }

    fun finish(current: GattReassembly): ByteArray = when (current) {
        is GattReassembly.Complete -> current.copyFrameForCodec()
        GattReassembly.Empty,
        is GattReassembly.Receiving,
        -> throw FragmentFormatException()
    }

    private fun validateFrame(frame: ByteArray) {
        require(frame.size in 1..maxFrameLength) { "frame length is outside the supported range" }
    }
}

private fun ByteArray.readUnsignedShort(offset: Int): Int =
    ((this[offset].toInt() and 0xff) shl 8) or (this[offset + 1].toInt() and 0xff)

private fun ByteArray.writeUnsignedShort(offset: Int, value: Int) {
    require(value in 0..0xffff)
    this[offset] = (value ushr 8).toByte()
    this[offset + 1] = value.toByte()
}
