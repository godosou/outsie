package ai.repose.mobile.unlock.responder

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.math.BigInteger
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.PublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec
import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NoBackupResponderStoreTest {
    @Test
    fun twoInstancesShareOneDurableCasDomainInsideNoBackupDirectory() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val databaseName = "responder-test-${UUID.randomUUID()}.sqlite3"
        val first = NoBackupResponderStore(context, databaseName)
        val second = NoBackupResponderStore(context, databaseName)
        try {
            val pairing = TrustedPairedMac(
                MAC_ID,
                DEVICE_ID,
                7uL,
                publicKey(MAC_PUBLIC),
                publicKey(PHONE_PUBLIC),
            )
            val active = DurableResponderRecord.Active(pairing, DurableResponseState.Ready())
            val revoked = DurableResponderRecord.Revoked(MAC_ID, DEVICE_ID, 7uL)

            assertEquals(
                context.noBackupFilesDir.canonicalFile,
                first.databaseFile.parentFile?.canonicalFile,
            )
            assertTrue(first.compareAndSwap(MAC_ID, DEVICE_ID, null, active))
            assertEquals(active, second.load(MAC_ID, DEVICE_ID))
            assertTrue(second.compareAndSwap(MAC_ID, DEVICE_ID, active, revoked))
            assertFalse(first.compareAndSwap(MAC_ID, DEVICE_ID, active, active))
            assertEquals(revoked, first.load(MAC_ID, DEVICE_ID))
        } finally {
            first.close()
            second.close()
            listOf("", "-wal", "-shm", "-journal").forEach { suffix ->
                context.noBackupFilesDir.resolve(databaseName + suffix).delete()
            }
        }
    }

    private fun publicKey(raw: ByteArray): PublicKey {
        val point = ECPoint(
            BigInteger(1, raw.copyOfRange(1, 33)),
            BigInteger(1, raw.copyOfRange(33, 65)),
        )
        return KeyFactory.getInstance("EC").generatePublic(ECPublicKeySpec(point, parameters()))
    }

    private fun parameters(): ECParameterSpec = AlgorithmParameters.getInstance("EC").run {
        init(ECGenParameterSpec("secp256r1"))
        getParameterSpec(ECParameterSpec::class.java)
    }

    private companion object {
        val MAC_ID = ByteArray(16) { it.toByte() }
        val DEVICE_ID = ByteArray(16) { (it + 16).toByte() }
        val MAC_PUBLIC = hex(
            "04e2534a3532d08fbba02dde659ee62bd0031fe2db785596ef509302446b030852" +
                "e0f1575a4c633cc719dfee5fda862d764efc96c3f30ee0055c42c23f184ed8c6",
        )
        val PHONE_PUBLIC = hex(
            "045ecbe4d1a6330a44c8f7ef951d4bf165e6c6b721efada985fb41661bc6e7fd" +
                "6c8734640c4998ff7e374b06ce1a64a2ecd82ab036384fb83d9a79b127a27d5032",
        )

        fun hex(value: String): ByteArray =
            value.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }
}
