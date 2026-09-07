package ai.repose.mobile.unlock.responder

import java.nio.file.Path
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidPhoneResponderContractTest {
    @Test
    fun `production composition keeps typed signing and durable responder native-only`() {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val source = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/responder/AndroidPhoneResponder.kt",
        ).toFile().readText()
        val pigeon = root.resolve(
            "mobile/packages/repose_unlock_native/pigeons/repose_unlock_api.dart",
        ).toFile().readText()

        assertTrue(source.contains("AndroidKeyStoreSigner"))
        assertTrue(source.contains("signProtocolV1PhoneResponsePrehash(request)"))
        assertTrue(source.contains("NoBackupResponderStore"))
        assertTrue(source.contains("coordinator.respond(challengeFrame)"))
        listOf(
            "respondToAuthenticatedChallenge",
            "PhoneResponseSigningRequest",
            "rawSignature",
            "sharedSecret",
            "ephemeralPrivateKey",
        ).forEach { forbidden -> assertFalse(pigeon.contains(forbidden)) }
        assertFalse(source.contains("android.util.Log"))
    }
}
