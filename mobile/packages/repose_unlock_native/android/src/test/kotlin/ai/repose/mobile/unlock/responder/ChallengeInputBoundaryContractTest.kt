package ai.repose.mobile.unlock.responder

import java.nio.file.Path
import org.junit.Assert.assertTrue
import org.junit.Test

class ChallengeInputBoundaryContractTest {
    @Test
    fun `fixed challenge length is checked before copying caller memory`() {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val source = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/responder/PhoneResponseCoordinator.kt",
        ).toFile().readText()
        val lengthCheck = source.indexOf(
            "if (encodedChallenge.size != ProtocolV1.challengeFrameLength)",
        )
        val snapshotCopy = source.indexOf("val challengeSnapshot = encodedChallenge.copyOf()")

        assertTrue(lengthCheck >= 0)
        assertTrue(snapshotCopy > lengthCheck)
    }
}
