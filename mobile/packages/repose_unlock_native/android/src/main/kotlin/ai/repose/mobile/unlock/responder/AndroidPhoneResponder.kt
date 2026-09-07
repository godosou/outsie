package ai.repose.mobile.unlock.responder

import android.content.Context
import ai.repose.mobile.unlock.crypto.AndroidKeyStoreSigner
import ai.repose.mobile.unlock.protocol.PhoneResponseSigningRequest
import java.io.Closeable
import java.security.PublicKey

/** Native-only production composition; BLE transport may pass only a complete Challenge frame. */
internal class AndroidPhoneResponder(context: Context) : Closeable {
    private val store = NoBackupResponderStore(context)
    private val identity = AndroidPhoneIdentitySigner(context)
    private val coordinator = PhoneResponseCoordinator(
        store = store,
        entropy = AndroidKeyStoreResponseEntropy(context),
        signer = identity,
    )

    fun respondToChallenge(challengeFrame: ByteArray): ByteArray =
        coordinator.respond(challengeFrame)

    fun installTrustedPairing(pairing: TrustedPairedMac): PairingChange =
        coordinator.installPairing(pairing)

    fun revokeTrustedPairing(
        macId: ByteArray,
        deviceId: ByteArray,
        pairingGeneration: ULong,
    ): RevocationChange = coordinator.revoke(macId, deviceId, pairingGeneration)

    override fun close() = store.close()
}

private class AndroidPhoneIdentitySigner(context: Context) : PhoneIdentitySigner {
    private val keyStoreSigner = AndroidKeyStoreSigner(context)

    override val publicKey: PublicKey
        get() = keyStoreSigner.publicKey()

    override fun sign(request: PhoneResponseSigningRequest): ByteArray =
        keyStoreSigner.signProtocolV1PhoneResponsePrehash(request).encoded()
}
