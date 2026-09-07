package ai.repose.mobile.unlock.responder

import ai.repose.mobile.unlock.crypto.P256SignatureCodec
import ai.repose.mobile.unlock.protocol.PhoneResponseSigningRequest
import ai.repose.mobile.unlock.protocol.ProtocolV1
import java.nio.charset.StandardCharsets
import java.security.AlgorithmParameters
import java.security.GeneralSecurityException
import java.security.MessageDigest
import java.security.PublicKey
import java.security.Signature
import java.security.interfaces.ECPublicKey
import java.security.spec.ECFieldFp
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import javax.crypto.Cipher
import javax.crypto.KeyAgreement
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

internal interface ResponseEntropy {
    fun openEphemeralAgreement(): EphemeralAgreement

    fun fillPhoneNonce(output: ByteArray)
}

internal interface EphemeralAgreement : AutoCloseable {
    val publicKey: PublicKey

    fun deriveSharedSecret(peerPublicKey: PublicKey): ByteArray
}

internal interface PhoneIdentitySigner {
    val publicKey: PublicKey

    fun sign(request: PhoneResponseSigningRequest): ByteArray
}

internal interface ResponseAeadEncryptor {
    fun encrypt(
        key: ByteArray,
        nonce: ByteArray,
        aad: ByteArray,
        plaintext: ByteArray,
    ): ByteArray
}

private object JceResponseAeadEncryptor : ResponseAeadEncryptor {
    override fun encrypt(
        key: ByteArray,
        nonce: ByteArray,
        aad: ByteArray,
        plaintext: ByteArray,
    ): ByteArray = Cipher.getInstance("AES/GCM/NoPadding").run {
        init(
            Cipher.ENCRYPT_MODE,
            SecretKeySpec(key, "AES"),
            GCMParameterSpec(GCM_TAG_BITS, nonce),
        )
        updateAAD(aad)
        doFinal(plaintext)
    }
}

internal class ResponderUnavailableException : IllegalStateException("phone response unavailable")

internal class DirectionalSessionMaterial(
    contextHash: ByteArray,
    macToPhoneKey: ByteArray,
    macToPhoneNonce: ByteArray,
    phoneToMacKey: ByteArray,
    phoneToMacNonce: ByteArray,
) : AutoCloseable {
    private var open = true
    private val contextHash = contextHash.copyOf()
    private val macToPhoneKey = macToPhoneKey.copyOf()
    private val macToPhoneNonce = macToPhoneNonce.copyOf()
    private val phoneToMacKey = phoneToMacKey.copyOf()
    private val phoneToMacNonce = phoneToMacNonce.copyOf()

    @Synchronized
    fun copyContextHash(): ByteArray = copyWhileOpen(contextHash)

    @Synchronized
    fun copyMacToPhoneKey(): ByteArray = copyWhileOpen(macToPhoneKey)

    @Synchronized
    fun copyMacToPhoneNonce(): ByteArray = copyWhileOpen(macToPhoneNonce)

    @Synchronized
    fun copyPhoneToMacKey(): ByteArray = copyWhileOpen(phoneToMacKey)

    @Synchronized
    fun copyPhoneToMacNonce(): ByteArray = copyWhileOpen(phoneToMacNonce)

    @Synchronized
    override fun close() {
        contextHash.fill(0)
        macToPhoneKey.fill(0)
        macToPhoneNonce.fill(0)
        phoneToMacKey.fill(0)
        phoneToMacNonce.fill(0)
        open = false
    }

    private fun copyWhileOpen(value: ByteArray): ByteArray {
        if (!open) throw ResponderUnavailableException()
        return value.copyOf()
    }
}

internal object SessionKeyDerivation {
    fun derive(
        sharedSecret: ByteArray,
        challengeFrame: ByteArray,
        responsePrefix: ByteArray,
    ): DirectionalSessionMaterial {
        if (sharedSecret.size != SHARED_SECRET_LENGTH ||
            challengeFrame.size != ProtocolV1.challengeFrameLength ||
            responsePrefix.size != RESPONSE_PREFIX_LENGTH
        ) {
            throw ResponderUnavailableException()
        }
        var contextHash: ByteArray? = null
        var salt: ByteArray? = null
        var prk: ByteArray? = null
        var macKey: ByteArray? = null
        var macNonce: ByteArray? = null
        var phoneKey: ByteArray? = null
        var phoneNonce: ByteArray? = null
        try {
            contextHash = hash(KDF_CONTEXT_LABEL, challengeFrame, responsePrefix)
            salt = hash(HKDF_SALT_LABEL, contextHash)
            prk = hmac(salt, sharedSecret)
            macKey = hkdfExpand(prk, MAC_TO_PHONE_KEY_LABEL, AES_KEY_LENGTH)
            macNonce = hkdfExpand(prk, MAC_TO_PHONE_NONCE_LABEL, GCM_NONCE_LENGTH)
            phoneKey = hkdfExpand(prk, PHONE_TO_MAC_KEY_LABEL, AES_KEY_LENGTH)
            phoneNonce = hkdfExpand(prk, PHONE_TO_MAC_NONCE_LABEL, GCM_NONCE_LENGTH)
            return DirectionalSessionMaterial(
                contextHash,
                macKey,
                macNonce,
                phoneKey,
                phoneNonce,
            )
        } finally {
            contextHash?.fill(0)
            salt?.fill(0)
            prk?.fill(0)
            macKey?.fill(0)
            macNonce?.fill(0)
            phoneKey?.fill(0)
            phoneNonce?.fill(0)
        }
    }
}

internal class PhoneResponseBuilder(
    private val entropy: ResponseEntropy,
    private val aeadEncryptor: ResponseAeadEncryptor = JceResponseAeadEncryptor,
) {
    fun build(
        challenge: AuthenticatedMacChallenge,
        previousCounter: ULong,
        signer: PhoneIdentitySigner,
    ): ByteArray {
        val parsed = challenge.challengeFrame()
        val counter = maxOf(previousCounter, parsed.counterFloor)
            .takeUnless { it == ULong.MAX_VALUE }
            ?.plus(1uL)
            ?: throw ResponderUnavailableException()
        val pairedMac = challenge.pairedMac()
        if (!samePublicKey(signer.publicKey, pairedMac.phoneIdentityPublicKey)) {
            throw ResponderUnavailableException()
        }

        var sharedSecret: ByteArray? = null
        var contextHash: ByteArray? = null
        var phoneKey: ByteArray? = null
        var phoneNonceMaterial: ByteArray? = null
        var proof: ByteArray? = null
        var aad: ByteArray? = null
        var material: DirectionalSessionMaterial? = null
        var ephemeral: EphemeralAgreement? = null
        val phoneNonce = ByteArray(NONCE_LENGTH)
        try {
            ephemeral = entropy.openEphemeralAgreement()
            requireP256PublicKey(ephemeral.publicKey)
            entropy.fillPhoneNonce(phoneNonce)
            val response = responsePrefix(
                parsed.encoded(),
                counter,
                phoneNonce,
                encodeP256PublicKey(ephemeral.publicKey),
            )
            sharedSecret = ephemeral.deriveSharedSecret(
                decodeP256PublicKey(parsed.encoded().copyOfRange(120, 185)),
            )
            material = SessionKeyDerivation.derive(
                sharedSecret,
                parsed.encoded(),
                response.copyOfRange(0, RESPONSE_PREFIX_LENGTH),
            )
            contextHash = material.copyContextHash()
            phoneKey = material.copyPhoneToMacKey()
            phoneNonceMaterial = material.copyPhoneToMacNonce()
            proof = hash(PHONE_TO_MAC_PROOF_LABEL, contextHash)
            aad = concat(PHONE_TO_MAC_AAD_LABEL, parsed.encoded(), response.copyOfRange(0, 278))
            val encrypted = aeadEncryptor.encrypt(
                phoneKey,
                phoneNonceMaterial,
                aad,
                proof,
            )
            if (encrypted.size != CIPHERTEXT_LENGTH + TAG_LENGTH) {
                throw ResponderUnavailableException()
            }
            verifyAeadRoundTrip(
                phoneKey,
                phoneNonceMaterial,
                aad,
                encrypted,
                proof,
            )
            encrypted.copyInto(response, destinationOffset = 278, startIndex = 0, endIndex = 32)
            encrypted.copyInto(response, destinationOffset = 310, startIndex = 32, endIndex = 48)
            encrypted.fill(0)

            val signingRequest = ProtocolV1.phoneResponseSigningRequest(
                challenge.verificationForResponse(),
                response.copyOfRange(0, 326),
            )
            val rawSignature = signer.sign(signingRequest)
            try {
                val canonicalDer = P256SignatureCodec.derFromRaw(rawSignature)
                val verified = Signature.getInstance("NONEwithECDSA").run {
                    initVerify(pairedMac.phoneIdentityPublicKey)
                    update(signingRequest.copyPrehashForSigner())
                    verify(canonicalDer)
                }
                if (!verified) throw ResponderUnavailableException()
                rawSignature.copyInto(response, destinationOffset = 326)
            } finally {
                rawSignature.fill(0)
            }
            ProtocolV1.decodeResponse(response)
            return response
        } catch (exception: ResponderUnavailableException) {
            throw exception
        } catch (_: GeneralSecurityException) {
            throw ResponderUnavailableException()
        } catch (_: IllegalArgumentException) {
            throw ResponderUnavailableException()
        } finally {
            phoneNonce.fill(0)
            sharedSecret?.fill(0)
            contextHash?.fill(0)
            phoneKey?.fill(0)
            phoneNonceMaterial?.fill(0)
            proof?.fill(0)
            aad?.fill(0)
            material?.close()
            ephemeral?.close()
        }
    }

    private fun verifyAeadRoundTrip(
        key: ByteArray,
        nonce: ByteArray,
        aad: ByteArray,
        encrypted: ByteArray,
        expectedProof: ByteArray,
    ) {
        val decrypted = Cipher.getInstance("AES/GCM/NoPadding").run {
            init(
                Cipher.DECRYPT_MODE,
                SecretKeySpec(key, "AES"),
                GCMParameterSpec(GCM_TAG_BITS, nonce),
            )
            updateAAD(aad)
            doFinal(encrypted)
        }
        try {
            if (!MessageDigest.isEqual(decrypted, expectedProof)) {
                throw ResponderUnavailableException()
            }
        } finally {
            decrypted.fill(0)
        }
    }

    private fun responsePrefix(
        challenge: ByteArray,
        counter: ULong,
        phoneNonce: ByteArray,
        phoneEphemeralPublicKey: ByteArray,
    ): ByteArray = ByteArray(ProtocolV1.responseFrameLength).also { response ->
        byteArrayOf(0x52, 0x50, 0x55, 0x4b, 1, 2, 0, 0, 0, 0, 1, 0x7a)
            .copyInto(response)
        challenge.copyInto(response, destinationOffset = 12, startIndex = 12, endIndex = 76)
        response.writeULong(76, counter)
        challenge.copyInto(response, destinationOffset = 84, startIndex = 88, endIndex = 120)
        phoneNonce.copyInto(response, destinationOffset = 116)
        challenge.copyInto(response, destinationOffset = 148, startIndex = 120, endIndex = 185)
        phoneEphemeralPublicKey.copyInto(response, destinationOffset = 213)
    }
}

internal fun requireP256PublicKey(publicKey: PublicKey) {
    val ec = publicKey as? ECPublicKey ?: throw ResponderUnavailableException()
    val expected = P256_PARAMETERS
    val actual = ec.params ?: throw ResponderUnavailableException()
    val field = actual.curve.field as? ECFieldFp ?: throw ResponderUnavailableException()
    val expectedField = expected.curve.field as ECFieldFp
    val point = ec.w
    val x = point.affineX
    val y = point.affineY
    val exactDomain =
        publicKey.algorithm == "EC" &&
            field.p == expectedField.p &&
            actual.curve.a == expected.curve.a &&
            actual.curve.b == expected.curve.b &&
            actual.generator == expected.generator &&
            actual.order == expected.order &&
            actual.cofactor == expected.cofactor
    val validPoint =
        point != java.security.spec.ECPoint.POINT_INFINITY &&
            x.signum() >= 0 &&
            y.signum() >= 0 &&
            x < field.p &&
            y < field.p &&
            y.modPow(TWO, field.p) ==
            x.modPow(THREE, field.p)
                .add(actual.curve.a.multiply(x))
                .add(actual.curve.b)
                .mod(field.p)
    if (!exactDomain || !validPoint) {
        throw ResponderUnavailableException()
    }
}

internal fun encodeP256PublicKey(publicKey: PublicKey): ByteArray {
    requireP256PublicKey(publicKey)
    val point = (publicKey as ECPublicKey).w
    return byteArrayOf(4) + point.affineX.toUnsignedFixed() + point.affineY.toUnsignedFixed()
}

internal fun samePublicKey(left: PublicKey, right: PublicKey): Boolean = try {
    MessageDigest.isEqual(encodeP256PublicKey(left), encodeP256PublicKey(right))
} catch (_: IllegalArgumentException) {
    false
}

private fun decodeP256PublicKey(encodedPeer: ByteArray): PublicKey {
    if (encodedPeer.size != 65 || encodedPeer[0] != 4.toByte()) {
        throw ResponderUnavailableException()
    }
    val peerPoint = java.security.spec.ECPoint(
        java.math.BigInteger(1, encodedPeer.copyOfRange(1, 33)),
        java.math.BigInteger(1, encodedPeer.copyOfRange(33, 65)),
    )
    val peer = java.security.KeyFactory.getInstance("EC").generatePublic(
        java.security.spec.ECPublicKeySpec(peerPoint, P256_PARAMETERS),
    )
    requireP256PublicKey(peer)
    return peer
}

internal fun normalizeSharedSecret(generated: ByteArray): ByteArray {
    if (generated.size == SHARED_SECRET_LENGTH) return generated
    if (generated.size > SHARED_SECRET_LENGTH) {
        generated.fill(0)
        throw ResponderUnavailableException()
    }
    return ByteArray(SHARED_SECRET_LENGTH).also { padded ->
        generated.copyInto(padded, destinationOffset = SHARED_SECRET_LENGTH - generated.size)
        generated.fill(0)
    }
}

private fun hkdfExpand(prk: ByteArray, label: ByteArray, length: Int): ByteArray {
    require(length in 1..32)
    val block = hmac(prk, concat(label, byteArrayOf(1)))
    return block.copyOf(length).also { block.fill(0) }
}

private fun hmac(key: ByteArray, input: ByteArray): ByteArray = Mac.getInstance("HmacSHA256").run {
    init(SecretKeySpec(key, "HmacSHA256"))
    doFinal(input)
}

private fun hash(vararg parts: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").run {
    parts.forEach(::update)
    digest()
}

private fun concat(vararg parts: ByteArray): ByteArray {
    val output = ByteArray(parts.sumOf(ByteArray::size))
    var cursor = 0
    parts.forEach { part ->
        part.copyInto(output, destinationOffset = cursor)
        cursor += part.size
    }
    return output
}

private fun java.math.BigInteger.toUnsignedFixed(): ByteArray {
    val signed = toByteArray()
    val unsigned = if (signed.size == 33 && signed[0] == 0.toByte()) {
        signed.copyOfRange(1, signed.size)
    } else {
        signed
    }
    if (unsigned.size > 32) throw ResponderUnavailableException()
    return ByteArray(32).also { unsigned.copyInto(it, destinationOffset = 32 - unsigned.size) }
}

private fun ByteArray.writeULong(offset: Int, value: ULong) {
    repeat(ULong.SIZE_BYTES) { index ->
        this[offset + index] = (value shr (8 * (ULong.SIZE_BYTES - 1 - index))).toByte()
    }
}

private fun ascii(value: String): ByteArray = value.toByteArray(StandardCharsets.US_ASCII)

private val P256_PARAMETERS: ECParameterSpec = AlgorithmParameters.getInstance("EC").run {
    init(ECGenParameterSpec("secp256r1"))
    getParameterSpec(ECParameterSpec::class.java)
}
private val TWO = java.math.BigInteger.valueOf(2)
private val THREE = java.math.BigInteger.valueOf(3)
private const val NONCE_LENGTH = 32
private const val SHARED_SECRET_LENGTH = 32
private const val AES_KEY_LENGTH = 32
private const val GCM_NONCE_LENGTH = 12
private const val CIPHERTEXT_LENGTH = 32
private const val TAG_LENGTH = 16
private const val GCM_TAG_BITS = TAG_LENGTH * 8
private const val RESPONSE_PREFIX_LENGTH = 278
private val KDF_CONTEXT_LABEL = ascii("repose-unlock-v1 kdf-context phone-response")
private val HKDF_SALT_LABEL = ascii("repose-unlock-v1 hkdf-salt")
private val MAC_TO_PHONE_KEY_LABEL = ascii("repose-unlock-v1 key mac-to-phone")
private val MAC_TO_PHONE_NONCE_LABEL = ascii("repose-unlock-v1 nonce mac-to-phone")
private val PHONE_TO_MAC_KEY_LABEL = ascii("repose-unlock-v1 key phone-to-mac")
private val PHONE_TO_MAC_NONCE_LABEL = ascii("repose-unlock-v1 nonce phone-to-mac")
private val PHONE_TO_MAC_AAD_LABEL = ascii("repose-unlock-v1 aad phone-to-mac")
private val PHONE_TO_MAC_PROOF_LABEL = ascii("repose-unlock-v1 proof phone-to-mac")
