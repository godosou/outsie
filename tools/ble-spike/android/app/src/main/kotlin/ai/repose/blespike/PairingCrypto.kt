package ai.repose.blespike

import java.math.BigInteger
import java.nio.charset.StandardCharsets
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.MessageDigest
import java.security.PrivateKey
import java.security.PublicKey
import java.security.SecureRandom
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec
import javax.crypto.KeyAgreement
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * The phone's half of `repose-pair-v2`. Protocol:
 * docs/plans/2026-09-11-pairing-v2.md
 *
 * Pure arithmetic — no Keystore, no radio, no Android APIs — so it can be tested
 * on the JVM against vectors OpenSSL produced. The Mac's implementation
 * (tools/ble-spike/mac/pair-crypto.swift) checks itself against the same ones.
 *
 * WHY THE TWO SIDES ARE NOT CHECKED AGAINST EACH OTHER
 *
 * The obvious test is to run one, record what it says, and assert the other
 * matches. That proves they agree, and proves nothing about whether they agree
 * on the right thing: both wrong the same way passes it. So each side is
 * measured against a third implementation neither shares code with. This
 * project has already shipped things that were only ever checked against
 * themselves.
 *
 * Every primitive is a standard: P-256, SEC1-uncompressed points with on-curve
 * validation, ECDH with a leading-zero-preserved X, HKDF-SHA-256, SHA-256 with
 * exact-ASCII domain separation. Nothing here is invented.
 */
object PairingCrypto {

    const val COMMIT_LABEL = "repose-pair-v3 commit"
    const val SAS_LABEL = "repose-pair-v3 sas"
    const val KDF_LABEL = "repose-pair-v3 presence-key"

    private const val CURVE = "secp256r1"
    private const val FIELD_LEN = 32

    private val random = SecureRandom()

    val p256: ECParameterSpec by lazy {
        AlgorithmParameters.getInstance("EC").run {
            init(ECGenParameterSpec(CURVE))
            getParameterSpec(ECParameterSpec::class.java)
        }
    }

    fun ascii(s: String): ByteArray = s.toByteArray(StandardCharsets.US_ASCII)

    fun sha256(vararg parts: ByteArray): ByteArray =
        MessageDigest.getInstance("SHA-256").apply { parts.forEach { update(it) } }.digest()

    fun hmacSha256(key: ByteArray, vararg parts: ByteArray): ByteArray =
        Mac.getInstance("HmacSHA256").run {
            init(SecretKeySpec(key, "HmacSHA256"))
            parts.forEach { update(it) }
            doFinal()
        }

    fun randomBytes(n: Int): ByteArray = ByteArray(n).also { random.nextBytes(it) }

    // ---- the protocol's four values ---------------------------------------

    /** `SHA256(COMMIT ‖ PK_M ‖ PK_P ‖ Np)`. Binds the phone's nonce AND the Mac it is for. */
    fun hexToBytes(hex: String): ByteArray {
        val clean = hex.trim()
        if (clean.length % 2 != 0) return ByteArray(0)
        return ByteArray(clean.length / 2) {
            ((Character.digit(clean[it * 2], 16) shl 4) or Character.digit(clean[it * 2 + 1], 16)).toByte()
        }
    }

    fun commitment(pkM: ByteArray, pkP: ByteArray, np: ByteArray): ByteArray =
        sha256(ascii(COMMIT_LABEL), pkM, pkP, np)

    /**
     * The transcript both screens' digits come from.
     *
     * `keyId` and `phoneId` are in it, and that is load-bearing rather than
     * tidy: they decide WHICH slot on the Mac this key lands in and WHOSE key it
     * replaces. Outside the transcript, a man in the middle could rewrite either
     * and have the Mac overwrite a different phone's key without the digits
     * changing. The label is v3 because a v2 transcript must not verify as one.
     */
    fun sasHash(
        pkM: ByteArray,
        pkP: ByteArray,
        nm: ByteArray,
        np: ByteArray,
        keyId: Int,
        phoneId: ByteArray,
    ): ByteArray =
        sha256(ascii(SAS_LABEL), pkM, pkP, nm, np, byteArrayOf(keyId.toByte()), phoneId)

    /**
     * Six digits, zero padded. Short on purpose: a person has to read it off two
     * screens and compare, and a longer string gets compared less carefully --
     * which would cost more security than the extra digits buy.
     */
    fun sasDigits(hash: ByteArray): String {
        var n = 0L
        for (i in 0 until 4) n = (n shl 8) or (hash[i].toLong() and 0xFF)
        return "%06d".format(n % 1_000_000L)
    }

    /** HKDF-SHA256 with the transcript hash as salt, one block out. */
    fun deriveKey(ecdhX: ByteArray, salt: ByteArray): ByteArray {
        val prk = hmacSha256(salt, ecdhX)
        return hmacSha256(prk, ascii(KDF_LABEL), byteArrayOf(0x01))
    }

    // ---- keys --------------------------------------------------------------

    fun newEphemeralKeyPair(): KeyPair =
        KeyPairGenerator.getInstance("EC").run {
            initialize(ECGenParameterSpec(CURVE), random)
            generateKeyPair()
        }

    /** SEC1 uncompressed: `0x04 ‖ X(32) ‖ Y(32)`. */
    fun encodePublicKey(key: PublicKey): ByteArray {
        val w = (key as ECPublicKey).w
        val out = ByteArray(1 + 2 * FIELD_LEN)
        out[0] = 0x04
        fixedWidth(w.affineX).copyInto(out, 1)
        fixedWidth(w.affineY).copyInto(out, 1 + FIELD_LEN)
        return out
    }

    /**
     * Decode a peer key, refusing anything that is not a point on P-256.
     *
     * Not a formality: a public key off the curve is the classic invalid-curve
     * attack, where the shared secret leaks the private scalar a few bits at a
     * time. The peer's key arrives over an unauthenticated channel by design --
     * that is what the SAS exists to compensate for -- so it is exactly the
     * input that must not be trusted.
     */
    fun decodePublicKey(sec1: ByteArray): ECPublicKey {
        require(sec1.size == 1 + 2 * FIELD_LEN && sec1[0] == 0x04.toByte()) {
            "peer key must be 65 bytes of SEC1 uncompressed"
        }
        val x = BigInteger(1, sec1.copyOfRange(1, 1 + FIELD_LEN))
        val y = BigInteger(1, sec1.copyOfRange(1 + FIELD_LEN, sec1.size))
        requireOnCurve(x, y)
        return KeyFactory.getInstance("EC")
            .generatePublic(ECPublicKeySpec(ECPoint(x, y), p256)) as ECPublicKey
    }

    /**
     * The shared X coordinate, 32 bytes, leading zeros preserved.
     *
     * `KeyAgreement.generateSecret()` for ECDH already returns exactly this on
     * the JVM, but the padding is the part worth naming: trimming X to its
     * significant bytes agrees with the other side 255 times out of 256, and
     * fails silently the other time. A bug that appears one pairing in 256 is
     * worse than one that appears every time.
     */
    fun ecdhX(privateKey: PrivateKey, peer: PublicKey): ByteArray {
        val secret = KeyAgreement.getInstance("ECDH").run {
            init(privateKey)
            doPhase(peer, true)
            generateSecret()
        }
        require(secret.size == FIELD_LEN) { "unexpected ECDH output size ${secret.size}" }
        return secret
    }

    private fun fixedWidth(v: BigInteger): ByteArray {
        val raw = v.toByteArray()
        return when {
            raw.size == FIELD_LEN -> raw
            // BigInteger prepends a sign byte when the top bit is set.
            raw.size == FIELD_LEN + 1 && raw[0] == 0.toByte() -> raw.copyOfRange(1, raw.size)
            raw.size < FIELD_LEN -> ByteArray(FIELD_LEN).also {
                raw.copyInto(it, FIELD_LEN - raw.size)
            }
            else -> throw IllegalArgumentException("coordinate too large")
        }
    }

    private fun requireOnCurve(x: BigInteger, y: BigInteger) {
        val curve = p256.curve
        val p = (curve.field as java.security.spec.ECFieldFp).p
        require(x.signum() >= 0 && x < p && y.signum() >= 0 && y < p) { "coordinate out of field" }
        require(!(x.signum() == 0 && y.signum() == 0)) { "point at infinity" }
        val lhs = y.modPow(BigInteger.TWO, p)
        val rhs = x.modPow(BigInteger.TWO, p).add(curve.a).multiply(x).add(curve.b).mod(p)
        require(lhs == rhs) { "peer key is not on P-256" }
    }
}
