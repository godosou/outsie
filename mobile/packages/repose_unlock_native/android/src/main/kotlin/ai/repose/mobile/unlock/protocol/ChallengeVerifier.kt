package ai.repose.mobile.unlock.protocol

import ai.repose.mobile.unlock.crypto.P256SignatureCodec
import java.math.BigInteger
import java.nio.charset.StandardCharsets
import java.security.GeneralSecurityException
import java.security.MessageDigest
import java.security.PublicKey
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.ECFieldFp

internal class ChallengeVerificationException : IllegalArgumentException(
    "the Mac challenge could not be authenticated",
)

internal object ChallengeVerifier {
    internal fun verify(
        pairedMac: PairedMacRecord,
        encodedChallenge: ByteArray,
    ): VerifiedMacChallenge = VerifiedMacChallenge.fromAuthenticatedChallenge(
        verifyFrame(pairedMac, encodedChallenge),
    )

    internal fun verifyFrame(
        pairedMac: PairedMacRecord,
        encodedChallenge: ByteArray,
    ): ChallengeFrame {
        val challenge = try {
            ProtocolV1.decodeChallenge(encodedChallenge)
        } catch (_: IllegalArgumentException) {
            throw ChallengeVerificationException()
        }
        if (!pairedMac.matches(challenge)) throw ChallengeVerificationException()
        val identityKey = pairedMac.identityPublicKey()
        if (!isP256PublicKey(identityKey)) throw ChallengeVerificationException()

        val encoded = challenge.encoded()
        val signature = encoded.copyOfRange(
            CHALLENGE_SIGNED_PREFIX_LENGTH,
            ProtocolV1.challengeFrameLength,
        )
        val canonicalDer = try {
            P256SignatureCodec.derFromRaw(signature)
        } catch (_: IllegalArgumentException) {
            throw ChallengeVerificationException()
        }
        val prehash = MessageDigest.getInstance("SHA-256").apply {
            update(MAC_CHALLENGE_SIGNATURE_LABEL)
            update(encoded, 0, CHALLENGE_SIGNED_PREFIX_LENGTH)
        }.digest()
        val authenticated = try {
            Signature.getInstance("NONEwithECDSA").run {
                initVerify(identityKey)
                update(prehash)
                verify(canonicalDer)
            }
        } catch (_: GeneralSecurityException) {
            false
        }
        if (!authenticated) throw ChallengeVerificationException()
        return challenge
    }

    private fun isP256PublicKey(publicKey: PublicKey): Boolean {
        if (publicKey.algorithm != "EC") return false
        val ecPublicKey = publicKey as? ECPublicKey ?: return false
        val parameters = ecPublicKey.params ?: return false
        val field = parameters.curve.field as? ECFieldFp ?: return false
        val point = ecPublicKey.w
        val x = point.affineX ?: return false
        val y = point.affineY ?: return false
        val exactDomain =
            field.p == P256_PRIME &&
                parameters.curve.a == P256_A &&
                parameters.curve.b == P256_B &&
                parameters.generator.affineX == P256_GENERATOR_X &&
                parameters.generator.affineY == P256_GENERATOR_Y &&
                parameters.order == P256_ORDER &&
                parameters.cofactor == 1
        if (!exactDomain || x.signum() < 0 || y.signum() < 0 || x >= P256_PRIME || y >= P256_PRIME) {
            return false
        }
        return y.modPow(TWO, P256_PRIME) ==
            x.modPow(THREE, P256_PRIME)
                .add(P256_A.multiply(x))
                .add(P256_B)
                .mod(P256_PRIME)
    }

    private const val CHALLENGE_SIGNED_PREFIX_LENGTH = 185
    private val MAC_CHALLENGE_SIGNATURE_LABEL =
        "repose-unlock-v1 signature mac-to-phone challenge"
            .toByteArray(StandardCharsets.US_ASCII)
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
