package ai.repose.mobile.unlock.responder

import ai.repose.mobile.unlock.crypto.P256SignatureCodec
import ai.repose.mobile.unlock.protocol.PhoneResponseSigningRequest
import ai.repose.mobile.unlock.protocol.ProtocolV1
import java.math.BigInteger
import java.nio.charset.StandardCharsets
import java.nio.file.Path
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.KeyPair
import java.security.PrivateKey
import java.security.PublicKey
import java.security.Signature
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPrivateKeySpec
import java.security.spec.ECPublicKeySpec
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import javax.crypto.KeyAgreement
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneResponseCoordinatorTest {
    @Test
    fun `missing durable pairing fails before randomness or signing`() {
        val entropy = CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce"))
        val signer = FixtureSigner(fixturePairing().phoneIdentityPublicKey)
        val coordinator = PhoneResponseCoordinator(MemoryResponderStore(), entropy, signer)

        assertThrows(ResponderUnavailableException::class.java) {
            coordinator.respond(fixtureBytes("challenge_frame"))
        }

        assertEquals(0, entropy.calls.get())
        assertEquals(0, signer.calls.get())
    }

    @Test
    fun `exact challenge retransmits durable bytes across restart without crypto or counter advance`() {
        val pairing = fixturePairing()
        val store = MemoryResponderStore()
        val entropy = CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce"))
        val signer = FixtureSigner(pairing.phoneIdentityPublicKey)
        val first = PhoneResponseCoordinator(store, entropy, signer)
        assertEquals(PairingChange.Initialized, first.installPairing(pairing))

        val allocated = first.respond(fixtureBytes("challenge_frame"))
        val repeated = first.respond(fixtureBytes("challenge_frame"))
        val restarted = PhoneResponseCoordinator(store, entropy, signer)
            .respond(fixtureBytes("challenge_frame"))

        assertArrayEquals(fixtureBytes("response_frame"), allocated)
        assertArrayEquals(allocated, repeated)
        assertArrayEquals(allocated, restarted)
        assertEquals(1, entropy.calls.get())
        assertEquals(1, signer.calls.get())
        val active = store.load(fixtureBytes("mac_id"), fixtureBytes("device_id"))
            as DurableResponderRecord.Active
        assertEquals(42uL, active.responseState.counter)
        assertTrue(active.responseState is DurableResponseState.Cached)
    }

    @Test
    fun `revocation tombstone blocks its generation and only a newer trusted pairing can rotate`() {
        val pairing = fixturePairing()
        val store = MemoryResponderStore()
        val coordinator = PhoneResponseCoordinator(
            store,
            CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce")),
            FixtureSigner(pairing.phoneIdentityPublicKey),
        )
        coordinator.installPairing(pairing)

        assertEquals(
            RevocationChange.Revoked,
            coordinator.revoke(pairing.macId, pairing.deviceId, 7uL),
        )
        assertEquals(
            RevocationChange.AlreadyRevoked,
            coordinator.revoke(pairing.macId, pairing.deviceId, 7uL),
        )
        assertThrows(ResponderUnavailableException::class.java) {
            coordinator.respond(fixtureBytes("challenge_frame"))
        }
        assertEquals(PairingChange.RevokedGeneration, coordinator.installPairing(pairing))

        val rotated = TrustedPairedMac(
            pairing.macId,
            pairing.deviceId,
            8uL,
            pairing.macIdentityPublicKey,
            pairing.phoneIdentityPublicKey,
        )
        assertEquals(PairingChange.Rotated, coordinator.installPairing(rotated))
        assertTrue(store.load(pairing.macId, pairing.deviceId) is DurableResponderRecord.Active)
    }

    @Test
    fun `pairing and revocation idempotence reject a false self CAS acknowledgement`() {
        val pairing = fixturePairing()
        val active = DurableResponderRecord.Active(pairing, DurableResponseState.Ready())
        val tombstone = DurableResponderRecord.Revoked(pairing.macId, pairing.deviceId, 7uL)
        val lyingActiveStore = SelfCasLiesStore(active, tombstone)
        val activeCoordinator = coordinator(lyingActiveStore, pairing)

        assertEquals(
            PairingChange.ConcurrentUpdate,
            activeCoordinator.installPairing(pairing),
        )

        val replacementPairing = TrustedPairedMac(
            pairing.macId,
            pairing.deviceId,
            8uL,
            pairing.macIdentityPublicKey,
            pairing.phoneIdentityPublicKey,
        )
        val newer = DurableResponderRecord.Active(
            replacementPairing,
            DurableResponseState.Ready(),
        )
        val lyingRevokedStore = SelfCasLiesStore(tombstone, newer)
        val revokedCoordinator = coordinator(lyingRevokedStore, pairing)
        assertEquals(
            RevocationChange.ConcurrentUpdate,
            revokedCoordinator.revoke(pairing.macId, pairing.deviceId, 7uL),
        )
    }

    @Test
    fun `two coordinators race once and both return the exact durable winner`() {
        val pairing = fixturePairing()
        val initial = DurableResponderRecord.Active(pairing, DurableResponseState.Ready())
        val store = AllocationBarrierStore(initial)
        val signer = JceFixtureSigner(pairing.phoneIdentityPublicKey)
        val first = PhoneResponseCoordinator(
            store,
            CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce")),
            signer,
        )
        val second = PhoneResponseCoordinator(
            store,
            CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce")),
            signer,
        )
        val executor = Executors.newFixedThreadPool(2)
        try {
            val futures = listOf(first, second).map { coordinator ->
                executor.submit<ByteArray> {
                    coordinator.respond(fixtureBytes("challenge_frame"))
                }
            }
            val responses = futures.map { it.get(10, TimeUnit.SECONDS) }

            assertArrayEquals(responses[0], responses[1])
            assertEquals(2, signer.calls.get())
            val durable = store.load(pairing.macId, pairing.deviceId)
                as DurableResponderRecord.Active
            assertArrayEquals(
                (durable.responseState as DurableResponseState.Cached).response,
                responses[0],
            )
        } finally {
            executor.shutdownNow()
        }
    }

    @Test
    fun `false allocation acknowledgement and overlapping revoke return no candidate bytes`() {
        val pairing = fixturePairing()
        val ready = DurableResponderRecord.Active(pairing, DurableResponseState.Ready())
        val signer = JceFixtureSigner(pairing.phoneIdentityPublicKey)
        val falseAck = FalseAllocationAckStore(ready)

        assertThrows(ResponderUnavailableException::class.java) {
            PhoneResponseCoordinator(
                falseAck,
                CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce")),
                signer,
            )
                .respond(fixtureBytes("challenge_frame"))
        }
        assertEquals(ready, falseAck.load(pairing.macId, pairing.deviceId))

        val revokeRace = RevokeWinsAllocationStore(ready)
        assertThrows(ResponderUnavailableException::class.java) {
            PhoneResponseCoordinator(
                revokeRace,
                CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce")),
                signer,
            )
                .respond(fixtureBytes("challenge_frame"))
        }
        assertTrue(
            revokeRace.load(pairing.macId, pairing.deviceId) is DurableResponderRecord.Revoked,
        )
    }

    @Test
    fun `caller mutation during durable lookup cannot change the authenticated challenge snapshot`() {
        val pairing = fixturePairing()
        val callerOwnedChallenge = fixtureBytes("challenge_frame")
        val delegate = MemoryResponderStore(
            listOf(DurableResponderRecord.Active(pairing, DurableResponseState.Ready())),
        )
        val store = MutatesCallerOnFirstLoadStore(delegate, callerOwnedChallenge)
        val coordinator = PhoneResponseCoordinator(
            store,
            CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce")),
            FixtureSigner(pairing.phoneIdentityPublicKey),
        )

        val response = coordinator.respond(callerOwnedChallenge)

        assertArrayEquals(fixtureBytes("response_frame"), response)
        assertTrue(!callerOwnedChallenge.contentEquals(fixtureBytes("challenge_frame")))
    }

    @Test
    fun `oversized challenge fails before durable or cryptographic callbacks`() {
        val store = CountingLoadStore()
        val pairing = fixturePairing()
        val entropy = CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce"))
        val signer = FixtureSigner(pairing.phoneIdentityPublicKey)

        assertThrows(ResponderUnavailableException::class.java) {
            PhoneResponseCoordinator(store, entropy, signer).respond(
                ByteArray(ProtocolV1.challengeFrameLength + 1),
            )
        }

        assertEquals(0, store.loads.get())
        assertEquals(0, entropy.calls.get())
        assertEquals(0, signer.calls.get())
    }

    private fun coordinator(
        store: ResponderStore,
        pairing: TrustedPairedMac,
    ): PhoneResponseCoordinator = PhoneResponseCoordinator(
        store,
        CountingEntropy(fixtureEphemeralKeyPair(), fixtureBytes("phone_nonce")),
        FixtureSigner(pairing.phoneIdentityPublicKey),
    )

    private class SelfCasLiesStore(
        initial: DurableResponderRecord,
        private val replacementBehindCaller: DurableResponderRecord,
    ) : ResponderStore {
        private var current = initial

        override fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord = current

        override fun compareAndSwap(
            macId: ByteArray,
            deviceId: ByteArray,
            expected: DurableResponderRecord?,
            replacement: DurableResponderRecord,
        ): Boolean {
            current = replacementBehindCaller
            return true
        }
    }

    private class AllocationBarrierStore(initial: DurableResponderRecord) : ResponderStore {
        private val monitor = Any()
        private val allocationArrivals = CountDownLatch(2)
        private var current = initial

        override fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord =
            synchronized(monitor) { current }

        override fun compareAndSwap(
            macId: ByteArray,
            deviceId: ByteArray,
            expected: DurableResponderRecord?,
            replacement: DurableResponderRecord,
        ): Boolean {
            val allocation =
                expected is DurableResponderRecord.Active &&
                    expected.responseState is DurableResponseState.Ready &&
                    replacement is DurableResponderRecord.Active &&
                    replacement.responseState is DurableResponseState.Cached
            if (allocation) {
                allocationArrivals.countDown()
                check(allocationArrivals.await(10, TimeUnit.SECONDS))
            }
            return synchronized(monitor) {
                if (current != expected) {
                    false
                } else {
                    current = replacement
                    true
                }
            }
        }
    }

    private class FalseAllocationAckStore(initial: DurableResponderRecord) : ResponderStore {
        private val initial = initial

        override fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord = initial

        override fun compareAndSwap(
            macId: ByteArray,
            deviceId: ByteArray,
            expected: DurableResponderRecord?,
            replacement: DurableResponderRecord,
        ): Boolean = true
    }

    private class RevokeWinsAllocationStore(initial: DurableResponderRecord.Active) :
        ResponderStore {
        private var current: DurableResponderRecord = initial

        override fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord = current

        override fun compareAndSwap(
            macId: ByteArray,
            deviceId: ByteArray,
            expected: DurableResponderRecord?,
            replacement: DurableResponderRecord,
        ): Boolean {
            if (replacement is DurableResponderRecord.Active &&
                replacement.responseState is DurableResponseState.Cached
            ) {
                current = DurableResponderRecord.Revoked(macId, deviceId, 7uL)
                return false
            }
            return false
        }
    }

    private class MutatesCallerOnFirstLoadStore(
        private val delegate: ResponderStore,
        private val callerOwnedChallenge: ByteArray,
    ) : ResponderStore {
        private var mutated = false

        override fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord? {
            if (!mutated) {
                mutated = true
                callerOwnedChallenge[0] = (callerOwnedChallenge[0].toInt() xor 1).toByte()
            }
            return delegate.load(macId, deviceId)
        }

        override fun compareAndSwap(
            macId: ByteArray,
            deviceId: ByteArray,
            expected: DurableResponderRecord?,
            replacement: DurableResponderRecord,
        ): Boolean = delegate.compareAndSwap(macId, deviceId, expected, replacement)
    }

    private class CountingLoadStore : ResponderStore {
        val loads = AtomicInteger()

        override fun load(macId: ByteArray, deviceId: ByteArray): DurableResponderRecord? {
            loads.incrementAndGet()
            return null
        }

        override fun compareAndSwap(
            macId: ByteArray,
            deviceId: ByteArray,
            expected: DurableResponderRecord?,
            replacement: DurableResponderRecord,
        ): Boolean = error("unreachable")
    }

    private class CountingEntropy(
        private val keyPair: KeyPair,
        private val nonce: ByteArray,
    ) : ResponseEntropy {
        val calls = AtomicInteger()

        override fun openEphemeralAgreement(): EphemeralAgreement {
            calls.incrementAndGet()
            return object : EphemeralAgreement {
                override val publicKey: PublicKey = keyPair.public

                override fun deriveSharedSecret(peerPublicKey: PublicKey): ByteArray =
                    KeyAgreement.getInstance("ECDH").run {
                        init(keyPair.private)
                        doPhase(peerPublicKey, true)
                        generateSecret()
                    }

                override fun close() = Unit
            }
        }

        override fun fillPhoneNonce(output: ByteArray) {
            nonce.copyInto(output)
        }
    }

    private class FixtureSigner(
        override val publicKey: PublicKey,
    ) : PhoneIdentitySigner {
        val calls = AtomicInteger()

        override fun sign(request: PhoneResponseSigningRequest): ByteArray {
            calls.incrementAndGet()
            assertArrayEquals(
                fixtureBytes("signature_transcript_hash"),
                request.copyPrehashForSigner(),
            )
            return fixtureBytes("response_signature_raw_low_s")
        }
    }

    private class JceFixtureSigner(
        override val publicKey: PublicKey,
    ) : PhoneIdentitySigner {
        val calls = AtomicInteger()
        private val privateKey = privateKey(fixtureBytes("phone_signing_private_key_test_only"))

        override fun sign(request: PhoneResponseSigningRequest): ByteArray {
            calls.incrementAndGet()
            val der = Signature.getInstance("NONEwithECDSA").run {
                initSign(privateKey)
                update(request.copyPrehashForSigner())
                sign()
            }
            return P256SignatureCodec.canonicalRawFromDer(der)
        }
    }

    private fun fixturePairing(): TrustedPairedMac = TrustedPairedMac(
        macId = fixtureBytes("mac_id"),
        deviceId = fixtureBytes("device_id"),
        pairingGeneration = 7uL,
        macIdentityPublicKey = publicKey(fixtureBytes("mac_signing_public_key")),
        phoneIdentityPublicKey = publicKey(fixtureBytes("phone_signing_public_key")),
    )

    private fun fixtureEphemeralKeyPair(): KeyPair = KeyPair(
        publicKey(fixtureBytes("phone_ephemeral_public_key")),
        privateKey(fixtureBytes("phone_ephemeral_private_key_test_only")),
    )

    private fun publicKey(raw: ByteArray): PublicKey {
        val point = ECPoint(
            BigInteger(1, raw.copyOfRange(1, 33)),
            BigInteger(1, raw.copyOfRange(33, 65)),
        )
        return KeyFactory.getInstance("EC").generatePublic(ECPublicKeySpec(point, p256Parameters()))
    }

    private fun privateKey(raw: ByteArray): PrivateKey = KeyFactory.getInstance("EC")
        .generatePrivate(ECPrivateKeySpec(BigInteger(1, raw), p256Parameters()))

    private fun p256Parameters(): ECParameterSpec = AlgorithmParameters.getInstance("EC").run {
        init(ECGenParameterSpec("secp256r1"))
        getParameterSpec(ECParameterSpec::class.java)
    }

    private companion object {
        fun p256ParametersStatic(): ECParameterSpec = AlgorithmParameters.getInstance("EC").run {
            init(ECGenParameterSpec("secp256r1"))
            getParameterSpec(ECParameterSpec::class.java)
        }

        fun privateKey(raw: ByteArray): PrivateKey = KeyFactory.getInstance("EC")
            .generatePrivate(ECPrivateKeySpec(BigInteger(1, raw), p256ParametersStatic()))

        fun fixtureBytes(name: String): ByteArray {
            val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
            val json = root.resolve("protocol/fixtures/v1/crypto-vectors.json").toFile().readText()
            val hex = requireNotNull(
                Regex("\\\"${Regex.escape(name)}\\\"\\s*:\\s*\\\"([0-9a-f]+)\\\"")
                    .find(json)
                    ?.groupValues
                    ?.get(1),
            )
            return hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        }
    }
}
