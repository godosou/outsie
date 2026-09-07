package ai.repose.mobile.unlock.responder

internal sealed class DurableResponseState(open val counter: ULong) {
    internal class Ready(override val counter: ULong = 0uL) : DurableResponseState(counter) {
        override fun equals(other: Any?): Boolean = other is Ready && counter == other.counter

        override fun hashCode(): Int = counter.hashCode()
    }

    internal class Cached(
        override val counter: ULong,
        val consoleUid: UInt,
        val auditSessionId: UInt,
        val lockEpoch: ULong,
        val challengeId: ULong,
        challengeFingerprint: ByteArray,
        response: ByteArray,
    ) : DurableResponseState(counter) {
        private val storedChallengeFingerprint = challengeFingerprint.copyOf()
        private val storedResponse = response.copyOf()

        init {
            require(counter > 0uL)
            require(storedChallengeFingerprint.size == SHA256_LENGTH)
            require(storedResponse.size == RESPONSE_LENGTH)
        }

        val challengeFingerprint: ByteArray
            get() = storedChallengeFingerprint.copyOf()
        val response: ByteArray
            get() = storedResponse.copyOf()

        override fun equals(other: Any?): Boolean =
            other is Cached &&
                counter == other.counter &&
                consoleUid == other.consoleUid &&
                auditSessionId == other.auditSessionId &&
                lockEpoch == other.lockEpoch &&
                challengeId == other.challengeId &&
                storedChallengeFingerprint.contentEquals(other.storedChallengeFingerprint) &&
                storedResponse.contentEquals(other.storedResponse)

        override fun hashCode(): Int {
            var result = counter.hashCode()
            result = 31 * result + consoleUid.hashCode()
            result = 31 * result + auditSessionId.hashCode()
            result = 31 * result + lockEpoch.hashCode()
            result = 31 * result + challengeId.hashCode()
            result = 31 * result + storedChallengeFingerprint.contentHashCode()
            return 31 * result + storedResponse.contentHashCode()
        }

        private companion object {
            const val SHA256_LENGTH = 32
            const val RESPONSE_LENGTH = 390
        }
    }
}

internal sealed class DurableResponderRecord {
    abstract val macId: ByteArray
    abstract val deviceId: ByteArray
    abstract val pairingGeneration: ULong

    internal class Active(
        val pairing: TrustedPairedMac,
        val responseState: DurableResponseState,
    ) : DurableResponderRecord() {
        override val macId: ByteArray
            get() = pairing.macId
        override val deviceId: ByteArray
            get() = pairing.deviceId
        override val pairingGeneration: ULong
            get() = pairing.pairingGeneration

        override fun equals(other: Any?): Boolean =
            other is Active &&
                pairing.sameAuthority(other.pairing) &&
                responseState == other.responseState

        override fun hashCode(): Int {
            var result = pairing.macId.contentHashCode()
            result = 31 * result + pairing.deviceId.contentHashCode()
            result = 31 * result + pairing.pairingGeneration.hashCode()
            result = 31 * result + encodeP256PublicKey(pairing.macIdentityPublicKey).contentHashCode()
            result = 31 * result + encodeP256PublicKey(pairing.phoneIdentityPublicKey).contentHashCode()
            return 31 * result + responseState.hashCode()
        }
    }

    internal class Revoked(
        macId: ByteArray,
        deviceId: ByteArray,
        override val pairingGeneration: ULong,
    ) : DurableResponderRecord() {
        private val storedMacId = macId.copyOf()
        private val storedDeviceId = deviceId.copyOf()

        init {
            require(storedMacId.size == 16)
            require(storedDeviceId.size == 16)
        }

        override val macId: ByteArray
            get() = storedMacId.copyOf()
        override val deviceId: ByteArray
            get() = storedDeviceId.copyOf()

        override fun equals(other: Any?): Boolean =
            other is Revoked &&
                pairingGeneration == other.pairingGeneration &&
                storedMacId.contentEquals(other.storedMacId) &&
                storedDeviceId.contentEquals(other.storedDeviceId)

        override fun hashCode(): Int {
            var result = storedMacId.contentHashCode()
            result = 31 * result + storedDeviceId.contentHashCode()
            return 31 * result + pairingGeneration.hashCode()
        }
    }
}

internal class ResponderStoreException : IllegalStateException("responder state unavailable")

internal interface ResponderStore {
    fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord?

    fun compareAndSwap(
        macId: ByteArray,
        deviceId: ByteArray,
        expected: DurableResponderRecord?,
        replacement: DurableResponderRecord,
    ): Boolean
}

internal class MemoryResponderStore(
    initial: List<DurableResponderRecord> = emptyList(),
) : ResponderStore {
    private val monitor = Any()
    private val records = mutableMapOf<RecordKey, DurableResponderRecord>()

    init {
        initial.forEach { record ->
            require(records.put(RecordKey(record.macId, record.deviceId), record) == null)
        }
    }

    override fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord? =
        synchronized(monitor) { records[RecordKey(macId, deviceId)] }

    override fun compareAndSwap(
        macId: ByteArray,
        deviceId: ByteArray,
        expected: DurableResponderRecord?,
        replacement: DurableResponderRecord,
    ): Boolean = synchronized(monitor) {
        if (!replacement.macId.contentEquals(macId) ||
            !replacement.deviceId.contentEquals(deviceId)
        ) {
            throw ResponderStoreException()
        }
        val key = RecordKey(macId, deviceId)
        if (records[key] != expected) return@synchronized false
        records[key] = replacement
        true
    }
}

private class RecordKey(macId: ByteArray, deviceId: ByteArray) {
    private val macId = macId.copyOf()
    private val deviceId = deviceId.copyOf()

    override fun equals(other: Any?): Boolean =
        other is RecordKey &&
            macId.contentEquals(other.macId) &&
            deviceId.contentEquals(other.deviceId)

    override fun hashCode(): Int = 31 * macId.contentHashCode() + deviceId.contentHashCode()
}
