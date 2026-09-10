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
import java.security.spec.ECFieldFp
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec
import javax.crypto.KeyAgreement
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * Pure crypto for `repose-pair-v1` and `repose-presence-v1` — no Android Keystore, no radio,
 * so both the real responder ([ReposePairing]) and the dev harness that stands in for the Mac
 * share exactly one implementation of the transcript / SAS / HKDF math.
 *
 * Every primitive here is a vetted standard reused from the codex prior art
 * (`codex/phone-proximity-unlock`): P-256, SEC1-uncompressed points with on-curve validation,
 * ECDH with leading-zero-preserved X, HKDF-SHA-256 with exact-ASCII domain-separated labels,
 * SHA-256 domain-separated transcripts, and HMAC-SHA-256. Nothing is invented.
 */
object PairingCrypto {

    private const val CURVE = "secp256r1"
    private const val FIELD_LEN = 32

    /** The P-256 domain parameters, loaded once from the platform provider. */
    val p256: ECParameterSpec by lazy {
        AlgorithmParameters.getInstance("EC").run {
            init(ECGenParameterSpec(CURVE))
            getParameterSpec(ECParameterSpec::class.java)
        }
    }

    private val random = SecureRandom()

    // ---- domain-separation labels (exact ASCII, never reformatted) ----

    fun ascii(label: String): ByteArray = label.toByteArray(StandardCharsets.US_ASCII)

    // ---- hashes / macs ----

    fun sha256(vararg parts: ByteArray): ByteArray =
        MessageDigest.getInstance("SHA-256").apply { parts.forEach { update(it) } }.digest()

    fun hmacSha256(key: ByteArray, vararg parts: ByteArray): ByteArray =
        Mac.getInstance("HmacSHA256").run {
            init(SecretKeySpec(key, "HmacSHA256"))
            parts.forEach { update(it) }
            doFinal()
        }

    // ---- HKDF-SHA-256 (RFC 5869) ----

    fun hkdfExtract(salt: ByteArray, ikm: ByteArray): ByteArray = hmacSha256(salt, ikm)

    fun hkdfExpand(prk: ByteArray, info: ByteArray, length: Int): ByteArray {
        require(length <= 32) { "single-block HKDF-Expand only" }
        val t1 = hmacSha256(prk, info, byteArrayOf(0x01))
        return t1.copyOf(length)
    }

    // ---- randomness ----

    fun randomBytes(n: Int): ByteArray = ByteArray(n).also { random.nextBytes(it) }

    fun newEphemeralKeyPair(): KeyPair =
        KeyPairGenerator.getInstance("EC").run {
            initialize(ECGenParameterSpec(CURVE))
            generateKeyPair()
        }

    // ---- SEC1-uncompressed public key codec (0x04 ‖ X(32) ‖ Y(32)) ----

    fun encodePublicKey(publicKey: PublicKey): ByteArray {
        val point = (publicKey as ECPublicKey).w
        val out = ByteArray(1 + 2 * FIELD_LEN)
        out[0] = 0x04
        fixedWidth(point.affineX).copyInto(out, 1)
        fixedWidth(point.affineY).copyInto(out, 1 + FIELD_LEN)
        return out
    }

    /** Decode + validate: exactly 65 bytes, 0x04 prefix, coordinates in-field and on-curve. */
    fun decodePublicKey(sec1: ByteArray): ECPublicKey {
        require(sec1.size == 1 + 2 * FIELD_LEN && sec1[0].toInt() == 0x04) { "bad SEC1 point" }
        val x = BigInteger(1, sec1.copyOfRange(1, 1 + FIELD_LEN))
        val y = BigInteger(1, sec1.copyOfRange(1 + FIELD_LEN, 1 + 2 * FIELD_LEN))
        requireOnCurve(x, y)
        val spec = ECPublicKeySpec(ECPoint(x, y), p256)
        return KeyFactory.getInstance("EC").generatePublic(spec) as ECPublicKey
    }

    /** ECDH shared secret = the X coordinate, big-endian, leading zeros preserved to 32 bytes. */
    fun ecdhX(privateKey: PrivateKey, peer: PublicKey): ByteArray {
        val secret = KeyAgreement.getInstance("ECDH").run {
            init(privateKey)
            doPhase(peer, true)
            generateSecret()
        }
        // Some providers strip a leading zero byte; normalise to the P-256 field width.
        return when {
            secret.size == FIELD_LEN -> secret
            secret.size > FIELD_LEN -> secret.copyOfRange(secret.size - FIELD_LEN, secret.size)
            else -> ByteArray(FIELD_LEN).also { secret.copyInto(it, FIELD_LEN - secret.size) }
        }
    }

    // ---- big-endian helpers ----

    fun longToBe(value: Long): ByteArray {
        val out = ByteArray(8)
        var v = value
        for (i in 7 downTo 0) {
            out[i] = (v and 0xff).toByte()
            v = v ushr 8
        }
        return out
    }

    /** The SAS reduction: 6 decimal digits from the first 4 transcript bytes (~20 bits). */
    fun sasDigits(sasHash: ByteArray): String {
        var v = 0L
        for (i in 0 until 4) v = (v shl 8) or (sasHash[i].toLong() and 0xff)
        return "%06d".format(v % 1_000_000L)
    }

    private fun fixedWidth(coordinate: BigInteger): ByteArray {
        val raw = coordinate.toByteArray()
        return when {
            raw.size == FIELD_LEN -> raw
            raw.size == FIELD_LEN + 1 && raw[0].toInt() == 0 -> raw.copyOfRange(1, raw.size)
            raw.size < FIELD_LEN -> ByteArray(FIELD_LEN).also { raw.copyInto(it, FIELD_LEN - raw.size) }
            else -> throw IllegalArgumentException("coordinate too large for P-256")
        }
    }

    private fun requireOnCurve(x: BigInteger, y: BigInteger) {
        require(x.signum() >= 0 && y.signum() >= 0 && x < P && y < P) { "coordinate out of field" }
        val left = y.modPow(TWO, P)
        val right = x.modPow(THREE, P).add(A.multiply(x)).add(B).mod(P)
        require(left == right) { "point not on P-256" }
    }

    private val TWO = BigInteger.valueOf(2)
    private val THREE = BigInteger.valueOf(3)
    private val P = BigInteger("ffffffff00000001000000000000000000000000ffffffffffffffffffffffff", 16)
    private val A = P.subtract(THREE)
    private val B = BigInteger("5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b", 16)

    // Belt-and-suspenders: keep a reference so the field spec is initialised eagerly on first use.
    init {
        require(p256.curve.field is ECFieldFp)
    }
}
