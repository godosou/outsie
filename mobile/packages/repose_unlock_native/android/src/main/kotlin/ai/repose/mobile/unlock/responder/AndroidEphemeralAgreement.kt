package ai.repose.mobile.unlock.responder

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.PrivateKey
import java.security.PublicKey
import java.security.SecureRandom
import java.security.spec.ECGenParameterSpec
import javax.crypto.KeyAgreement

internal class AndroidKeyStoreResponseEntropy(
    context: Context,
    private val secureRandom: SecureRandom = SecureRandom(),
) : ResponseEntropy {
    private val noBackupDirectory = context.applicationContext.noBackupFilesDir

    override fun openEphemeralAgreement(): EphemeralAgreement {
        val lease = EphemeralSlotLease.acquireAndSweep(
            noBackupDirectory,
            SLOT_COUNT,
            LOCK_FILE_PREFIX,
        ) { slot ->
            deleteEphemeralAliasAndConfirm("$EPHEMERAL_ALIAS_PREFIX$slot")
        }
        val alias = "$EPHEMERAL_ALIAS_PREFIX${lease.slot}"
        return try {
            val keyPair = KeyPairGenerator
                .getInstance(KeyProperties.KEY_ALGORITHM_EC, ANDROID_KEY_STORE)
                .run {
                    initialize(
                        KeyGenParameterSpec.Builder(
                            alias,
                            KeyProperties.PURPOSE_AGREE_KEY,
                        )
                            .setKeySize(P256_KEY_SIZE)
                            .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                            .setUserAuthenticationRequired(false)
                            .setUnlockedDeviceRequired(false)
                            .build(),
                    )
                    generateKeyPair()
                }
            validateGeneratedKey(keyPair.private, keyPair.public)
            AndroidKeyStoreEphemeralAgreement(
                alias = alias,
                lease = lease,
                privateKey = keyPair.private,
                publicKey = keyPair.public,
            )
        } catch (_: Exception) {
            closeFailedOpen(lease)
            throw ResponderUnavailableException()
        }
    }

    override fun fillPhoneNonce(output: ByteArray) {
        try {
            secureRandom.nextBytes(output)
        } catch (_: Exception) {
            throw ResponderUnavailableException()
        }
    }

    private fun validateGeneratedKey(privateKey: PrivateKey, publicKey: PublicKey) {
        val keyInfo = KeyFactory.getInstance(privateKey.algorithm, ANDROID_KEY_STORE)
            .getKeySpec(privateKey, KeyInfo::class.java)
        if (privateKey.encoded != null ||
            keyInfo.origin != KeyProperties.ORIGIN_GENERATED ||
            keyInfo.securityLevel !in HARDWARE_SECURITY_LEVELS ||
            keyInfo.keySize != P256_KEY_SIZE ||
            keyInfo.purposes != KeyProperties.PURPOSE_AGREE_KEY ||
            keyInfo.isUserAuthenticationRequired
        ) {
            throw ResponderUnavailableException()
        }
        requireP256PublicKey(publicKey)
    }

    private fun closeFailedOpen(lease: EphemeralSlotLease) {
        try {
            deleteEphemeralAliasAndConfirm("$EPHEMERAL_ALIAS_PREFIX${lease.slot}")
        } catch (_: Exception) {
            // The fixed slot remains bounded and the next lease holder retries stale cleanup.
        } finally {
            try {
                lease.close()
            } catch (_: Exception) {
                // The kernel releases the file lock when this process or descriptor terminates.
            }
        }
    }

    private class AndroidKeyStoreEphemeralAgreement(
        private val alias: String,
        private val lease: EphemeralSlotLease,
        privateKey: PrivateKey,
        override val publicKey: PublicKey,
    ) : EphemeralAgreement {
        private var privateKey: PrivateKey? = privateKey

        override fun deriveSharedSecret(peerPublicKey: PublicKey): ByteArray {
            requireP256PublicKey(peerPublicKey)
            val key = privateKey ?: throw ResponderUnavailableException()
            val generated = try {
                KeyAgreement.getInstance("ECDH", ANDROID_KEY_STORE).run {
                    init(key)
                    doPhase(peerPublicKey, true)
                    generateSecret()
                }
            } catch (_: Exception) {
                throw ResponderUnavailableException()
            }
            return normalizeSharedSecret(generated)
        }

        override fun close() {
            if (privateKey == null) return
            privateKey = null
            var failed = false
            try {
                deleteEphemeralAliasAndConfirm(alias)
            } catch (_: Exception) {
                failed = true
            } finally {
                try {
                    lease.close()
                } catch (_: Exception) {
                    failed = true
                }
            }
            if (failed) throw ResponderUnavailableException()
        }
    }

    internal companion object {
        const val EPHEMERAL_ALIAS_PREFIX =
            "ai.repose.mobile.unlock.ephemeral.ecdh.p256.v1.slot."
        const val SLOT_COUNT = 4
        internal const val LOCK_FILE_PREFIX = "repose-unlock-ephemeral-v1-slot-"
        private const val ANDROID_KEY_STORE = "AndroidKeyStore"
        private const val P256_KEY_SIZE = 256
        private val HARDWARE_SECURITY_LEVELS = setOf(
            KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT,
            KeyProperties.SECURITY_LEVEL_STRONGBOX,
        )

        private fun deleteEphemeralAliasAndConfirm(alias: String) {
            val keyStore = try {
                KeyStore.getInstance(ANDROID_KEY_STORE).apply { load(null) }
            } catch (_: Exception) {
                throw ResponderUnavailableException()
            }
            try {
                if (keyStore.containsAlias(alias)) {
                    keyStore.deleteEntry(alias)
                }
                if (keyStore.containsAlias(alias)) {
                    throw ResponderUnavailableException()
                }
            } catch (exception: ResponderUnavailableException) {
                throw exception
            } catch (_: Exception) {
                throw ResponderUnavailableException()
            }
        }
    }
}
