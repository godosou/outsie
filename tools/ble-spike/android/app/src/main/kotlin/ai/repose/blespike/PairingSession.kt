package ai.repose.blespike

import java.security.KeyPair

/**
 * The phone's `repose-pair-v2` responder, as a state machine with no Android in
 * it. Protocol: docs/plans/2026-09-11-pairing-v2.md
 *
 * Kept free of BLE and Keystore so the ordering rules can be tested on the JVM.
 * Those rules are not bookkeeping -- they are the protocol:
 *
 *   - PK_P and the commitment are not handed out until PK_M has arrived, because
 *     the commitment covers PK_M. Producing one earlier would mean committing to
 *     a Mac we have not been told about.
 *
 *   - Np is not handed out until Nm has arrived. A phone that reveals its nonce
 *     first has given it away for free, and its commitment stops committing to
 *     anything: a man in the middle could then choose his own nonce knowing
 *     ours, and drive both screens to the same six digits.
 *
 * So every out-of-order request is refused. Not "ignored" and not "answered with
 * zeros" -- refused, so a caller that has the order wrong finds out.
 */
class PairingSession(
    private val keyPair: KeyPair = PairingCrypto.newEphemeralKeyPair(),
    private val np: ByteArray = PairingCrypto.randomBytes(16),
) {

    enum class Stage { WaitingForMacKey, WaitingForMacNonce, AwaitingHuman, Done, Failed }

    var stage: Stage = Stage.WaitingForMacKey
        private set

    /** The six digits, once both nonces are known. Null before that. */
    var digits: String? = null
        private set

    private var pkM: ByteArray? = null
    private var nm: ByteArray? = null
    private var sasHash: ByteArray? = null

    val publicKey: ByteArray by lazy { PairingCrypto.encodePublicKey(keyPair.public) }

    /** M1. Rejects anything that is not a point on P-256. */
    fun receiveMacKey(bytes: ByteArray): Boolean {
        if (stage != Stage.WaitingForMacKey) return false
        return runCatching {
            // Validates the curve. An unauthenticated peer key is exactly the
            // input an invalid-curve attack arrives on.
            PairingCrypto.decodePublicKey(bytes)
            pkM = bytes.copyOf()
            stage = Stage.WaitingForMacNonce
            true
        }.getOrElse {
            stage = Stage.Failed
            false
        }
    }

    /** P1 payload: PK_P(65) ‖ Cp(32). Only after M1. */
    fun keyAndCommitment(): ByteArray? {
        val mac = pkM ?: return null
        if (stage != Stage.WaitingForMacNonce) return null
        return publicKey + PairingCrypto.commitment(mac, publicKey, np)
    }

    /** M2. */
    fun receiveMacNonce(bytes: ByteArray): Boolean {
        if (stage != Stage.WaitingForMacNonce || bytes.size != 16) return false
        val mac = pkM ?: return false
        nm = bytes.copyOf()
        val hash = PairingCrypto.sasHash(mac, publicKey, bytes, np)
        sasHash = hash
        digits = PairingCrypto.sasDigits(hash)
        stage = Stage.AwaitingHuman
        return true
    }

    /** P2: the reveal. Only after M2. */
    fun revealNonce(): ByteArray? =
        if (stage == Stage.AwaitingHuman) np.copyOf() else null

    /**
     * The shared key, once a human has confirmed the digits match.
     *
     * Deriving it is cheap and harmless; what must not happen is storing it
     * without the confirmation, so the caller is the one that decides, and this
     * returns null in every state where nobody has been asked yet.
     */
    fun deriveKey(): ByteArray? {
        val mac = pkM ?: return null
        val salt = sasHash ?: return null
        if (stage != Stage.AwaitingHuman) return null
        val x = PairingCrypto.ecdhX(keyPair.private, PairingCrypto.decodePublicKey(mac))
        return PairingCrypto.deriveKey(x, salt)
    }

    fun complete() { stage = Stage.Done }
    fun fail() { stage = Stage.Failed }
}
