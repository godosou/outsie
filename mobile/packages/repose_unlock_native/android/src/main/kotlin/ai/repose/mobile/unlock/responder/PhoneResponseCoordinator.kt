package ai.repose.mobile.unlock.responder

import ai.repose.mobile.unlock.crypto.P256SignatureCodec
import ai.repose.mobile.unlock.protocol.ProtocolV1
import java.security.MessageDigest
import java.security.Signature

internal enum class PairingChange {
    Initialized,
    Rotated,
    AlreadyCurrent,
    RequiresRevocation,
    RevokedGeneration,
    ConcurrentUpdate,
}

internal enum class RevocationChange {
    Revoked,
    AlreadyRevoked,
    StaleGeneration,
    ConcurrentUpdate,
}

internal class PhoneResponseCoordinator(
    private val store: ResponderStore,
    entropy: ResponseEntropy,
    private val signer: PhoneIdentitySigner,
) {
    private val builder = PhoneResponseBuilder(entropy)
    private val gate = Any()

    fun respond(encodedChallenge: ByteArray): ByteArray = synchronized(gate) {
        if (encodedChallenge.size != ProtocolV1.challengeFrameLength) {
            throw ResponderUnavailableException()
        }
        // Freeze caller-owned transport memory once. Decode, authentication, transcripts, and the
        // durable cache fingerprint must all describe these exact bytes even if BLE reuses a buffer.
        val challengeSnapshot = encodedChallenge.copyOf()
        val parsed = try {
            ProtocolV1.decodeChallenge(challengeSnapshot)
        } catch (_: IllegalArgumentException) {
            throw ResponderUnavailableException()
        }
        val expected = load(parsed.macId, parsed.deviceId)
        val active = expected as? DurableResponderRecord.Active
            ?: throw ResponderUnavailableException()
        if (!active.pairing.matches(parsed.macId, parsed.deviceId) ||
            active.pairing.pairingGeneration != parsed.pairingGeneration
        ) {
            throw ResponderUnavailableException()
        }
        val authenticated = TrustedChallengeAuthenticator.authenticate(
            active.pairing,
            challengeSnapshot,
        )
        val fingerprint = MessageDigest.getInstance("SHA-256").digest(challengeSnapshot)
        val currentCounter = when (val state = active.responseState) {
            is DurableResponseState.Ready -> state.counter
            is DurableResponseState.Cached -> {
                if (state.challengeFingerprint.contentEquals(fingerprint)) {
                    if (!cachedMatches(active, state, authenticated, state.counter)) {
                        throw ResponderUnavailableException()
                    }
                    confirmAndReadBack(active)
                    return@synchronized state.response
                }
                enforceFreshChallengeOrder(state, parsed)
                state.counter
            }
        }
        if (currentCounter == ULong.MAX_VALUE || parsed.counterFloor == ULong.MAX_VALUE) {
            throw ResponderUnavailableException()
        }
        val expectedCounter = maxOf(currentCounter, parsed.counterFloor) + 1uL
        val candidateResponse = builder.build(authenticated, currentCounter, signer)
        val candidate = DurableResponderRecord.Active(
            active.pairing,
            DurableResponseState.Cached(
                counter = expectedCounter,
                consoleUid = parsed.consoleUid,
                auditSessionId = parsed.auditSessionId,
                lockEpoch = parsed.lockEpoch,
                challengeId = parsed.challengeId,
                challengeFingerprint = fingerprint,
                response = candidateResponse,
            ),
        )
        val won = compareAndSwap(parsed.macId, parsed.deviceId, active, candidate)
        if (won) {
            if (!cachedMatches(
                    candidate,
                    candidate.responseState as DurableResponseState.Cached,
                    authenticated,
                    expectedCounter,
                )
            ) {
                throw ResponderUnavailableException()
            }
            confirmAndReadBack(candidate)
            return@synchronized candidateResponse
        }

        val winner = load(parsed.macId, parsed.deviceId) as? DurableResponderRecord.Active
            ?: throw ResponderUnavailableException()
        val winnerCache = winner.responseState as? DurableResponseState.Cached
            ?: throw ResponderUnavailableException()
        if (!winner.pairing.sameAuthority(active.pairing) ||
            !winnerCache.challengeFingerprint.contentEquals(fingerprint) ||
            !cachedMatches(winner, winnerCache, authenticated, expectedCounter)
        ) {
            throw ResponderUnavailableException()
        }
        confirmAndReadBack(winner)
        winnerCache.response
    }

    fun installPairing(pairing: TrustedPairedMac): PairingChange = synchronized(gate) {
        val expected = load(pairing.macId, pairing.deviceId)
        val outcome = when (expected) {
            null -> PairingChange.Initialized
            is DurableResponderRecord.Active -> {
                if (expected.pairing.sameAuthority(pairing)) {
                    if (!compareAndSwap(pairing.macId, pairing.deviceId, expected, expected)) {
                        return@synchronized PairingChange.ConcurrentUpdate
                    }
                    return@synchronized if (
                        load(pairing.macId, pairing.deviceId) == expected
                    ) {
                        PairingChange.AlreadyCurrent
                    } else {
                        PairingChange.ConcurrentUpdate
                    }
                }
                return@synchronized PairingChange.RequiresRevocation
            }
            is DurableResponderRecord.Revoked -> {
                if (pairing.pairingGeneration <= expected.pairingGeneration) {
                    return@synchronized PairingChange.RevokedGeneration
                }
                PairingChange.Rotated
            }
        }
        val replacement = DurableResponderRecord.Active(pairing, DurableResponseState.Ready())
        if (compareAndSwap(pairing.macId, pairing.deviceId, expected, replacement)) {
            outcome
        } else {
            PairingChange.ConcurrentUpdate
        }
    }

    fun revoke(
        macId: ByteArray,
        deviceId: ByteArray,
        pairingGeneration: ULong,
    ): RevocationChange = synchronized(gate) {
        val expected = load(macId, deviceId)
        when (expected) {
            is DurableResponderRecord.Active -> {
                if (expected.pairingGeneration != pairingGeneration) {
                    return@synchronized RevocationChange.StaleGeneration
                }
            }
            is DurableResponderRecord.Revoked -> {
                if (expected.pairingGeneration == pairingGeneration) {
                    if (!compareAndSwap(macId, deviceId, expected, expected)) {
                        return@synchronized RevocationChange.ConcurrentUpdate
                    }
                    return@synchronized if (load(macId, deviceId) == expected) {
                        RevocationChange.AlreadyRevoked
                    } else {
                        RevocationChange.ConcurrentUpdate
                    }
                }
                return@synchronized RevocationChange.StaleGeneration
            }
            null -> Unit
        }
        val tombstone = DurableResponderRecord.Revoked(macId, deviceId, pairingGeneration)
        if (compareAndSwap(macId, deviceId, expected, tombstone)) {
            RevocationChange.Revoked
        } else {
            RevocationChange.ConcurrentUpdate
        }
    }

    private fun cachedMatches(
        active: DurableResponderRecord.Active,
        cached: DurableResponseState.Cached,
        authenticated: AuthenticatedMacChallenge,
        expectedCounter: ULong,
    ): Boolean {
        return try {
            val challenge = authenticated.challengeFrame()
            val challengeBytes = challenge.encoded()
            val responseBytes = cached.response
            val response = ProtocolV1.decodeResponse(responseBytes)
            if (!active.pairing.sameAuthority(authenticated.pairedMac()) ||
                cached.counter != expectedCounter ||
                cached.consoleUid != challenge.consoleUid ||
                cached.auditSessionId != challenge.auditSessionId ||
                cached.lockEpoch != challenge.lockEpoch ||
                cached.challengeId != challenge.challengeId ||
                response.pairingGeneration != active.pairingGeneration ||
                response.challengeId != challenge.challengeId ||
                response.counter != cached.counter ||
                !responseBytes.copyOfRange(12, 76).contentEquals(
                    challengeBytes.copyOfRange(12, 76),
                ) ||
                !responseBytes.copyOfRange(84, 116).contentEquals(
                    challengeBytes.copyOfRange(88, 120),
                ) ||
                !responseBytes.copyOfRange(148, 213).contentEquals(
                    challengeBytes.copyOfRange(120, 185),
                )
            ) {
                false
            } else {
                // The builder already verified its AEAD round trip while the hardware-backed
                // ephemeral key and derived material existed. We intentionally persist neither.
                // A durable retransmit is therefore authorized by the exact signed-Challenge
                // fingerprint, every mirrored field, and this paired phone-key signature; the Mac
                // remains the final AEAD verifier. This avoids persisting ECDH/private material.
                val request = ProtocolV1.phoneResponseSigningRequest(
                    authenticated.verificationForResponse(),
                    responseBytes.copyOfRange(0, 326),
                )
                val signature = responseBytes.copyOfRange(326, 390)
                Signature.getInstance("NONEwithECDSA").run {
                    initVerify(active.pairing.phoneIdentityPublicKey)
                    update(request.copyPrehashForSigner())
                    verify(P256SignatureCodec.derFromRaw(signature))
                }
            }
        } catch (_: Exception) {
            false
        }
    }

    private fun enforceFreshChallengeOrder(
        cached: DurableResponseState.Cached,
        challenge: ai.repose.mobile.unlock.protocol.ChallengeFrame,
    ) {
        if (challenge.lockEpoch < cached.lockEpoch ||
            (challenge.lockEpoch == cached.lockEpoch && challenge.challengeId < cached.challengeId) ||
            (challenge.lockEpoch == cached.lockEpoch &&
                (challenge.challengeId == cached.challengeId ||
                    challenge.consoleUid != cached.consoleUid ||
                    challenge.auditSessionId != cached.auditSessionId))
        ) {
            throw ResponderUnavailableException()
        }
    }

    private fun confirmAndReadBack(candidate: DurableResponderRecord) {
        if (!compareAndSwap(candidate.macId, candidate.deviceId, candidate, candidate)) {
            throw ResponderUnavailableException()
        }
        if (load(candidate.macId, candidate.deviceId) != candidate) {
            throw ResponderUnavailableException()
        }
    }

    private fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord? = try {
        store.load(macId, deviceId)
    } catch (_: Exception) {
        throw ResponderUnavailableException()
    }

    private fun compareAndSwap(
        macId: ByteArray,
        deviceId: ByteArray,
        expected: DurableResponderRecord?,
        replacement: DurableResponderRecord,
    ): Boolean = try {
        store.compareAndSwap(macId, deviceId, expected, replacement)
    } catch (_: Exception) {
        throw ResponderUnavailableException()
    }
}
