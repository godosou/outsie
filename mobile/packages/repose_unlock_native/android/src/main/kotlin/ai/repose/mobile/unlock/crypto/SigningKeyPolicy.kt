package ai.repose.mobile.unlock.crypto

import java.math.BigInteger
import java.security.PublicKey
import java.security.interfaces.ECPublicKey
import java.security.spec.ECFieldFp

internal data class SigningKeyPolicySnapshot(
    val privateKeyAlgorithm: String,
    val privateKeyEncoded: ByteArray?,
    val publicKey: PublicKey,
    val keySize: Int,
    val purposes: Int,
    val origin: Int,
    val securityLevel: Int,
    val digests: Array<String>,
    val userAuthenticationRequired: Boolean,
    val trustedPolicyMarkerMatches: Boolean,
)

internal object SigningKeyPolicy {
    private const val KEY_ALGORITHM_EC = "EC"
    private const val PURPOSE_SIGN = 4
    private const val PURPOSE_VERIFY = 8
    private const val ORIGIN_GENERATED = 1
    private const val SECURITY_LEVEL_TRUSTED_ENVIRONMENT = 1
    private const val SECURITY_LEVEL_STRONGBOX = 2

    const val curveName: String = "secp256r1"
    const val digestName: String = "SHA-256"
    const val keyStoreDigestName: String = "NONE"
    const val userAuthenticationRequired: Boolean = false
    const val unlockedDeviceRequired: Boolean = false
    const val requiredKeySize: Int = 256
    const val requiredPurposes: Int = PURPOSE_SIGN or PURPOSE_VERIFY
    const val requiredOrigin: Int = ORIGIN_GENERATED

    fun accepts(snapshot: SigningKeyPolicySnapshot): Boolean =
        snapshot.privateKeyAlgorithm == KEY_ALGORITHM_EC &&
            snapshot.privateKeyEncoded == null &&
            isP256PublicKey(snapshot.publicKey) &&
            snapshot.keySize == requiredKeySize &&
            snapshot.purposes == requiredPurposes &&
            snapshot.origin == requiredOrigin &&
            acceptsSecurityLevel(snapshot.securityLevel) &&
            snapshot.digests.contentEquals(arrayOf(keyStoreDigestName)) &&
            !snapshot.userAuthenticationRequired &&
            snapshot.trustedPolicyMarkerMatches

    fun acceptsSecurityLevel(securityLevel: Int): Boolean =
        securityLevel == SECURITY_LEVEL_TRUSTED_ENVIRONMENT ||
            securityLevel == SECURITY_LEVEL_STRONGBOX

    private fun isP256PublicKey(publicKey: PublicKey): Boolean {
        if (publicKey.algorithm != KEY_ALGORITHM_EC) return false
        val ecPublicKey = publicKey as? ECPublicKey ?: return false
        val parameters = ecPublicKey.params ?: return false
        val field = parameters.curve.field as? ECFieldFp ?: return false
        val generator = parameters.generator
        val point = ecPublicKey.w
        val x = point.affineX ?: return false
        val y = point.affineY ?: return false

        val parametersMatch =
            field.p == P256_PRIME &&
                parameters.curve.a == P256_A &&
                parameters.curve.b == P256_B &&
                generator.affineX == P256_GENERATOR_X &&
                generator.affineY == P256_GENERATOR_Y &&
                parameters.order == P256_ORDER &&
                parameters.cofactor == 1
        if (!parametersMatch) return false
        if (x.signum() < 0 || y.signum() < 0 || x >= P256_PRIME || y >= P256_PRIME) {
            return false
        }

        return y.modPow(TWO, P256_PRIME) ==
            x.modPow(THREE, P256_PRIME)
                .add(P256_A.multiply(x))
                .add(P256_B)
                .mod(P256_PRIME)
    }

    private val TWO = BigInteger.valueOf(2)
    private val THREE = BigInteger.valueOf(3)
    private val P256_PRIME = BigInteger(
        "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff",
        16,
    )
    private val P256_A = P256_PRIME.subtract(THREE)
    private val P256_B = BigInteger(
        "5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b",
        16,
    )
    private val P256_GENERATOR_X = BigInteger(
        "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        16,
    )
    private val P256_GENERATOR_Y = BigInteger(
        "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        16,
    )
    private val P256_ORDER = BigInteger(
        "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
        16,
    )
}
