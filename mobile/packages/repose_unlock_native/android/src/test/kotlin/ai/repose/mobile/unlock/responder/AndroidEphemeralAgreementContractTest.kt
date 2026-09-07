package ai.repose.mobile.unlock.responder

import java.nio.file.Path
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidEphemeralAgreementContractTest {
    @Test
    fun `production ECDH uses a bounded cross process leased AndroidKeyStore alias pool`() {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val source = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/responder/AndroidEphemeralAgreement.kt",
        ).toFile().readText()
        val composition = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/responder/AndroidPhoneResponder.kt",
        ).toFile().readText()
        val leaseSource = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/responder/EphemeralSlotLease.kt",
        ).toFile().readText()

        assertTrue(source.contains("KeyProperties.PURPOSE_AGREE_KEY"))
        assertTrue(source.contains("ECGenParameterSpec(\"secp256r1\")"))
        assertTrue(source.contains("privateKey.encoded != null"))
        assertTrue(source.contains("keyInfo.origin != KeyProperties.ORIGIN_GENERATED"))
        assertTrue(source.contains("keyInfo.securityLevel !in HARDWARE_SECURITY_LEVELS"))
        assertTrue(source.contains("deleteEphemeralAliasAndConfirm"))
        assertTrue(source.contains("EPHEMERAL_ALIAS_PREFIX"))
        assertTrue(source.contains("SLOT_COUNT = 4"))
        assertTrue(source.contains("EphemeralSlotLease"))
        assertTrue(leaseSource.contains("tryLock()"))
        assertTrue(source.contains("KeyAgreement.getInstance(\"ECDH\", ANDROID_KEY_STORE)"))
        assertFalse(source.contains("UUID.randomUUID"))
        assertFalse(source.contains("android.util.Log"))
        assertTrue(composition.contains("AndroidKeyStoreResponseEntropy(context)"))
        assertFalse(composition.contains("SystemResponseEntropy()"))
    }
}
