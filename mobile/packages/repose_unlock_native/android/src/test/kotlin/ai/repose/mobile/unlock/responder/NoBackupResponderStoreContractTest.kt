package ai.repose.mobile.unlock.responder

import java.nio.file.Path
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NoBackupResponderStoreContractTest {
    @Test
    fun `production store uses one no-backup SQLite transaction domain and no logging bridge`() {
        val root = Path.of(requireNotNull(System.getProperty("repose.workspaceRoot")))
        val source = root.resolve(
            "mobile/packages/repose_unlock_native/android/src/main/kotlin/" +
                "ai/repose/mobile/unlock/responder/NoBackupResponderStore.kt",
        ).toFile().readText()

        assertTrue(source.contains("applicationContext.noBackupFilesDir"))
        assertTrue(source.contains("beginTransaction()"))
        assertTrue(source.contains("ResponderStateCodec.encode(expected)"))
        assertTrue(source.contains("ResponderStateCodec.encode(replacement)"))
        assertTrue(source.contains("state_blob BLOB NOT NULL"))
        assertFalse(source.contains("pairing_generation INTEGER"))
        assertFalse(source.contains("counter INTEGER"))
        assertFalse(source.contains("android.util.Log"))
        assertFalse(source.contains("Pigeon"))
    }
}
