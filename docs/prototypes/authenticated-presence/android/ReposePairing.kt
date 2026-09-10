package ai.repose.blespike

import android.content.Context
import java.security.KeyPair
import java.security.MessageDigest
import java.security.interfaces.ECPublicKey

/**
 * The `repose-pair-v1` responder (§1) — the phone's half of the MITM-resistant bootstrap that
 * mints the shared presence key `K`. Built entirely from [PairingCrypto] / codex primitives;
 * no crypto is invented.
 *
 * Roles: **Mac = initiator, Phone = responder.** The wire legs are:
 * ```
 * M0  QR (authentic, Mac→Phone) : ver ‖ MacId(16) ‖ gen(8) ‖ sessionId(16) ‖ PK_M(65) ‖ IK_M(65) ‖ expiry(8)
 * P1  BLE GATT (Phone→Mac)      : PK_P(65) ‖ IK_P(65) ‖ deviceId(16) ‖ Cp(32)   (phone commits its nonce)
 * M2  BLE GATT (Mac→Phone)      : Nm(16)                                        (Mac reveals)
 * P3  BLE GATT (Phone→Mac)      : Np(16)                                        (phone reveals; Mac checks Cp)
 * ```
 * then SAS numeric-comparison, ECDH + HKDF → {K_confirm, K}, and a mandatory two-message
 * key-confirmation, verified before anything is persisted.
 *
 * The GATT/QR transport itself is NOT wired in this Android-only build — that needs the real
 * phone + Mac. [preparePairing] therefore runs a clearly-marked dev harness that plays the Mac
 * initiator locally so the walking skeleton can mint a real `K` and advertise a well-formed,
 * self-consistent rotating beacon today. The responder methods below are the real ones; the
 * harness only supplies the Mac's messages.
 */
class ReposePairing(context: Context) {

    private val identity = ReposeIdentityKey(context)

    // ---- M0 / P1 wire layout ----

    private object M0 {
        const val LEN = 1 + 16 + 8 + 16 + 65 + 65 + 8 // 179
        const val OFF_MAC_ID = 1
        const val OFF_GEN = 17
        const val OFF_SESSION = 25
        const val OFF_PK_M = 41
        const val OFF_IK_M = 106
        const val OFF_EXPIRY = 171
    }

    /** Parsed, validated QR payload. */
    class MacHello(
        val raw: ByteArray,
        val macId: ByteArray,
        val gen: Long,
        val sessionId: ByteArray,
        val pkMSec1: ByteArray,
        val pkM: ECPublicKey,
        val ikMSec1: ByteArray,
        val expiry: Long,
    )

    /** Live responder session — ephemerals + nonces die with it on confirm / abort / timeout. */
    class Session internal constructor(
        internal val mac: MacHello,
        internal val ephemeral: KeyPair,
        internal val pkPSec1: ByteArray,
        internal val ikPSec1: ByteArray,
        internal val deviceId: ByteArray,
        internal val np: ByteArray,
    ) {
        internal var nm: ByteArray? = null
        internal var sas: String? = null
        internal var kConfirm: ByteArray? = null
        internal var k: ByteArray? = null
        internal var ctx: ByteArray? = null
    }

    fun parseMacHello(m0: ByteArray): MacHello {
        require(m0.size == M0.LEN) { "M0 wrong length" }
        require(m0[0].toInt() == 1) { "M0 version" }
        val pkMSec1 = m0.copyOfRange(M0.OFF_PK_M, M0.OFF_PK_M + 65)
        return MacHello(
            raw = m0.copyOf(),
            macId = m0.copyOfRange(M0.OFF_MAC_ID, M0.OFF_MAC_ID + 16),
            gen = readLong(m0, M0.OFF_GEN),
            sessionId = m0.copyOfRange(M0.OFF_SESSION, M0.OFF_SESSION + 16),
            pkMSec1 = pkMSec1,
            pkM = PairingCrypto.decodePublicKey(pkMSec1),
            ikMSec1 = m0.copyOfRange(M0.OFF_IK_M, M0.OFF_IK_M + 65),
            expiry = readLong(m0, M0.OFF_EXPIRY),
        )
    }

    /** P1: generate the ephemeral + commit the phone nonce; returns the session and P1 bytes. */
    fun begin(mac: MacHello, deviceId: ByteArray): Pair<Session, ByteArray> {
        require(deviceId.size == 16) { "deviceId must be 16 bytes" }
        val ephemeral = PairingCrypto.newEphemeralKeyPair()
        val pkPSec1 = PairingCrypto.encodePublicKey(ephemeral.public)
        val ikPSec1 = identity.publicKeySec1()
        val np = PairingCrypto.randomBytes(16)
        val cp = PairingCrypto.sha256(
            PairingCrypto.ascii(SpikeContract.PairLabels.COMMIT_PHONE),
            pkPSec1, ikPSec1, deviceId, np,
        )
        val session = Session(mac, ephemeral, pkPSec1, ikPSec1, deviceId, np)
        val p1 = pkPSec1 + ikPSec1 + deviceId + cp
        return session to p1
    }

    /** Handle M2 (Mac's revealed nonce), compute the SAS, and return P3 (the phone's nonce). */
    fun receiveMacNonce(session: Session, nm: ByteArray): ByteArray {
        require(nm.size == 16) { "Nm must be 16 bytes" }
        session.nm = nm
        val sasHash = PairingCrypto.sha256(
            PairingCrypto.ascii(SpikeContract.PairLabels.SAS),
            session.mac.pkMSec1, session.pkPSec1, session.mac.ikMSec1, session.ikPSec1, nm, session.np,
        )
        session.sas = PairingCrypto.sasDigits(sasHash)
        return session.np.copyOf()
    }

    fun sasDigits(session: Session): String =
        session.sas ?: error("SAS not computed yet — receive M2 first")

    /**
     * After the human confirms the SAS matches: derive `Z`, the transcript context, and
     * {K_confirm, K}; return the phone→mac key-confirmation MAC. Nothing is persisted yet.
     */
    fun deriveAfterConfirm(session: Session): ByteArray {
        val nm = session.nm ?: error("no Mac nonce")
        val z = PairingCrypto.ecdhX(session.ephemeral.private, session.mac.pkM)
        val ctx = PairingCrypto.sha256(
            PairingCrypto.ascii(SpikeContract.PairLabels.TRANSCRIPT),
            session.mac.raw, session.pkPSec1, session.ikPSec1, session.deviceId, nm, session.np,
        )
        val salt = PairingCrypto.sha256(PairingCrypto.ascii(SpikeContract.PairLabels.SALT), ctx)
        val prk = PairingCrypto.hkdfExtract(salt, z)
        val kConfirm = PairingCrypto.hkdfExpand(prk, PairingCrypto.ascii(SpikeContract.PairLabels.CONFIRM), 32)
        val k = PairingCrypto.hkdfExpand(prk, PairingCrypto.ascii(SpikeContract.PairLabels.PRESENCE_KEY), 32)
        session.ctx = ctx
        session.kConfirm = kConfirm
        session.k = k
        return PairingCrypto.hmacSha256(
            kConfirm, PairingCrypto.ascii(SpikeContract.PairLabels.KC_PHONE_TO_MAC), ctx,
        )
    }

    /** Verify the Mac→phone key-confirmation MAC (constant-time). Failure ⇒ abort, persist nothing. */
    fun verifyMacKc(session: Session, kcMacToPhone: ByteArray): Boolean {
        val kConfirm = session.kConfirm ?: return false
        val ctx = session.ctx ?: return false
        val expected = PairingCrypto.hmacSha256(
            kConfirm, PairingCrypto.ascii(SpikeContract.PairLabels.KC_MAC_TO_PHONE), ctx,
        )
        return MessageDigest.isEqual(expected, kcMacToPhone)
    }

    /**
     * Persist the pairing: import `K` into the Keystore as a non-exportable HMAC key, write the
     * (non-secret) paired record, and zero the raw `K` in memory. Called only after both the SAS
     * and the key-confirmation have passed.
     */
    fun persist(session: Session, store: AppStore) {
        val k = session.k ?: error("no K derived")
        PresenceKey.importKey(SpikeContract.PRESENCE_KEY_ID, k)
        store.savePairedRecord(
            PairedRecord(
                macId = session.mac.macId,
                deviceId = session.deviceId,
                gen = session.mac.gen,
                ikMSec1 = session.mac.ikMSec1,
                keyId = SpikeContract.PRESENCE_KEY_ID,
            ),
        )
        k.fill(0)
        session.k = null
        session.kConfirm?.fill(0)
    }

    // ---- Dev harness: plays the Mac initiator locally (skeleton stand-in) ----

    /**
     * A prepared pairing whose SAS is ready to show; [complete] finishes the derive + persist
     * after the human confirms, [abort] drops all ephemerals.
     */
    class Pending internal constructor(
        val sas: String,
        private val onComplete: () -> Unit,
        private val onAbort: () -> Unit,
    ) {
        fun complete() = onComplete()
        fun abort() = onAbort()
    }

    /**
     * DEV/SKELETON ONLY. Runs the whole `repose-pair-v1` exchange in-process, with this method
     * playing the Mac initiator, so the phone can produce a *real* SAS and a *real* `K` without a
     * second device. The security property that the SAS defends (comparing codes across two
     * physically-separate screens) is NOT exercised here — that is the job of the real GATT/QR
     * transport with an actual Mac, which is the "needs the real phone + Mac" item for tomorrow.
     * What this DOES give the walking skeleton today: a hardware-held `K` and a well-formed
     * rotating beacon that a Mac verifier holding the same `K` would accept.
     */
    fun preparePairing(store: AppStore): Pending {
        // --- Mac side (local stand-in) ---
        val macId = PairingCrypto.randomBytes(16)
        val gen = 1L
        val sessionId = PairingCrypto.randomBytes(16)
        val macEphemeral = PairingCrypto.newEphemeralKeyPair()
        val pkMSec1 = PairingCrypto.encodePublicKey(macEphemeral.public)
        val macIdentity = PairingCrypto.newEphemeralKeyPair() // stand-in for IK_M (real Mac: Secure Enclave)
        val ikMSec1 = PairingCrypto.encodePublicKey(macIdentity.public)
        val expiry = System.currentTimeMillis() / 1000L + 180L
        val m0 = ByteArray(M0.LEN).also { buf ->
            buf[0] = 1
            macId.copyInto(buf, M0.OFF_MAC_ID)
            PairingCrypto.longToBe(gen).copyInto(buf, M0.OFF_GEN)
            sessionId.copyInto(buf, M0.OFF_SESSION)
            pkMSec1.copyInto(buf, M0.OFF_PK_M)
            ikMSec1.copyInto(buf, M0.OFF_IK_M)
            PairingCrypto.longToBe(expiry).copyInto(buf, M0.OFF_EXPIRY)
        }

        // --- Phone side (the real responder) ---
        val mac = parseMacHello(m0)
        val (session, p1) = begin(mac, store.deviceId())
        val pkPSec1 = p1.copyOfRange(0, 65)
        val ikPSec1 = p1.copyOfRange(65, 130)
        val deviceId = p1.copyOfRange(130, 146)
        val cp = p1.copyOfRange(146, 178)

        // Mac reveals Nm (M2); phone reveals Np (P3); Mac checks the commitment.
        val nm = PairingCrypto.randomBytes(16)
        val np = receiveMacNonce(session, nm)
        val cpCheck = PairingCrypto.sha256(
            PairingCrypto.ascii(SpikeContract.PairLabels.COMMIT_PHONE), pkPSec1, ikPSec1, deviceId, np,
        )
        check(MessageDigest.isEqual(cp, cpCheck)) { "commitment mismatch (harness)" }

        val sas = sasDigits(session)

        val complete = {
            // Phone confirms → derive + kc P→M.
            val kcPhoneToMac = deriveAfterConfirm(session)
            // Mac independently derives the same secrets and verifies kc P→M, then answers kc M→P.
            // Mac's Z = ECDH(sk_M, PK_P), where PK_P is the phone's ephemeral public from P1.
            val z = PairingCrypto.ecdhX(macEphemeral.private, PairingCrypto.decodePublicKey(pkPSec1))
            val ctx = PairingCrypto.sha256(
                PairingCrypto.ascii(SpikeContract.PairLabels.TRANSCRIPT),
                m0, pkPSec1, ikPSec1, deviceId, nm, np,
            )
            val salt = PairingCrypto.sha256(PairingCrypto.ascii(SpikeContract.PairLabels.SALT), ctx)
            val prk = PairingCrypto.hkdfExtract(salt, z)
            val macKConfirm = PairingCrypto.hkdfExpand(prk, PairingCrypto.ascii(SpikeContract.PairLabels.CONFIRM), 32)
            val expectPhone = PairingCrypto.hmacSha256(
                macKConfirm, PairingCrypto.ascii(SpikeContract.PairLabels.KC_PHONE_TO_MAC), ctx,
            )
            check(MessageDigest.isEqual(expectPhone, kcPhoneToMac)) { "kc P→M mismatch (harness)" }
            val kcMacToPhone = PairingCrypto.hmacSha256(
                macKConfirm, PairingCrypto.ascii(SpikeContract.PairLabels.KC_MAC_TO_PHONE), ctx,
            )
            check(verifyMacKc(session, kcMacToPhone)) { "kc M→P verification failed" }
            persist(session, store)
        }
        return Pending(sas = sas, onComplete = complete, onAbort = {})
    }

    private fun readLong(buf: ByteArray, offset: Int): Long {
        var v = 0L
        for (i in 0 until 8) v = (v shl 8) or (buf[offset + i].toLong() and 0xff)
        return v
    }
}

/** Non-secret paired record persisted on the phone (§1.5). */
data class PairedRecord(
    val macId: ByteArray,
    val deviceId: ByteArray,
    val gen: Long,
    val ikMSec1: ByteArray,
    val keyId: Int,
)
