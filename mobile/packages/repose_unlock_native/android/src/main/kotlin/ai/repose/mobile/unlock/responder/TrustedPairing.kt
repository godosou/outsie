package ai.repose.mobile.unlock.responder

import ai.repose.mobile.unlock.protocol.ChallengeVerifier
import ai.repose.mobile.unlock.protocol.ProtocolV1
import java.security.PublicKey

internal class TrustedPairedMac(
    macId: ByteArray,
    deviceId: ByteArray,
    val pairingGeneration: ULong,
    val macIdentityPublicKey: PublicKey,
    val phoneIdentityPublicKey: PublicKey,
) {
    private val storedMacId = macId.copyOf()
    private val storedDeviceId = deviceId.copyOf()

    init {
        require(storedMacId.size == IDENTIFIER_LENGTH)
        require(storedDeviceId.size == IDENTIFIER_LENGTH)
        requireP256PublicKey(macIdentityPublicKey)
        requireP256PublicKey(phoneIdentityPublicKey)
    }

    val macId: ByteArray
        get() = storedMacId.copyOf()
    val deviceId: ByteArray
        get() = storedDeviceId.copyOf()

    internal fun matches(macId: ByteArray, deviceId: ByteArray): Boolean =
        storedMacId.contentEquals(macId) && storedDeviceId.contentEquals(deviceId)

    internal fun sameAuthority(other: TrustedPairedMac): Boolean =
        pairingGeneration == other.pairingGeneration &&
            matches(other.storedMacId, other.storedDeviceId) &&
            samePublicKey(macIdentityPublicKey, other.macIdentityPublicKey) &&
            samePublicKey(phoneIdentityPublicKey, other.phoneIdentityPublicKey)

    private companion object {
        const val IDENTIFIER_LENGTH = 16
    }
}

internal interface PairedMacRepository {
    fun find(macId: ByteArray, deviceId: ByteArray): TrustedPairedMac?
}

internal class MemoryPairedMacRepository(records: List<TrustedPairedMac>) : PairedMacRepository {
    private val records = records.toList()

    override fun find(macId: ByteArray, deviceId: ByteArray): TrustedPairedMac? =
        records.singleOrNull { it.matches(macId, deviceId) }
}

internal object TrustedChallengeAuthenticator {
    fun authenticate(
        repository: PairedMacRepository,
        encodedChallenge: ByteArray,
    ): AuthenticatedMacChallenge {
        if (encodedChallenge.size != ProtocolV1.challengeFrameLength) {
            throw ResponderUnavailableException()
        }
        // This API is also an input boundary: keep the parsed capability and signature verification
        // tied to one immutable byte snapshot even when a repository callback is re-entrant.
        val challengeSnapshot = encodedChallenge.copyOf()
        val parsed = try {
            ProtocolV1.decodeChallenge(challengeSnapshot)
        } catch (_: IllegalArgumentException) {
            throw ResponderUnavailableException()
        }
        val pairedMac = repository.find(parsed.macId, parsed.deviceId)
            ?: throw ResponderUnavailableException()
        val verification = try {
            ChallengeVerifier.verifyTrustedPairing(
                pairedMac.macId,
                pairedMac.deviceId,
                pairedMac.pairingGeneration.toLong(),
                pairedMac.macIdentityPublicKey,
                challengeSnapshot,
            )
        } catch (_: IllegalArgumentException) {
            throw ResponderUnavailableException()
        }
        return AuthenticatedMacChallenge.fromLocalRepository(
            verification,
            parsed,
            pairedMac,
        )
    }

    fun authenticate(
        pairedMac: TrustedPairedMac,
        encodedChallenge: ByteArray,
    ): AuthenticatedMacChallenge = authenticate(
        MemoryPairedMacRepository(listOf(pairedMac)),
        encodedChallenge,
    )
}
