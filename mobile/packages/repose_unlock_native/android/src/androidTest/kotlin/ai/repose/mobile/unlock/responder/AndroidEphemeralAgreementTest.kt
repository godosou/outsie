package ai.repose.mobile.unlock.responder

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.PrivateKey
import java.security.spec.ECGenParameterSpec
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AndroidEphemeralAgreementTest {
    private val context
        get() = ApplicationProvider.getApplicationContext<android.content.Context>()

    @Before
    @After
    fun removeTestAliases() {
        val keyStore = keyStore()
        ephemeralAliases(keyStore).forEach(keyStore::deleteEntry)
    }

    @Test
    fun overlappingAgreementsUseIndependentLeasedSlotsAndRemainUsable() {
        val first = AndroidKeyStoreResponseEntropy(context).openEphemeralAgreement()
        val firstAlias = ephemeralAliases(keyStore()).single()
        val second = AndroidKeyStoreResponseEntropy(context).openEphemeralAgreement()
        val aliases = ephemeralAliases(keyStore())
        val secondAlias = aliases.single { it != firstAlias }
        val peer = KeyPairGenerator.getInstance("EC").run {
            initialize(ECGenParameterSpec("secp256r1"))
            generateKeyPair()
        }
        try {
            assertEquals(2, aliases.size)
            assertKeyPolicy(firstAlias)
            assertKeyPolicy(secondAlias)
            assertEquals(32, first.deriveSharedSecret(peer.public).size)
            assertEquals(32, second.deriveSharedSecret(peer.public).size)

            first.close()

            assertEquals(listOf(secondAlias), ephemeralAliases(keyStore()))
            assertEquals(32, second.deriveSharedSecret(peer.public).size)
        } finally {
            try {
                first.close()
            } finally {
                second.close()
            }
        }
        assertTrue(ephemeralAliases(keyStore()).isEmpty())
    }

    @Test
    fun acquiringAnUnlockedSlotReplacesItsCrashLeftoverAlias() {
        val staleAlias = "${AndroidKeyStoreResponseEntropy.EPHEMERAL_ALIAS_PREFIX}1"
        generateKey(staleAlias)

        val agreement = AndroidKeyStoreResponseEntropy(context).openEphemeralAgreement()
        try {
            val liveAlias = ephemeralAliases(keyStore()).single()
            assertEquals("${AndroidKeyStoreResponseEntropy.EPHEMERAL_ALIAS_PREFIX}0", liveAlias)
            assertFalse(keyStore().containsAlias(staleAlias))
            assertKeyPolicy(liveAlias)
        } finally {
            agreement.close()
        }
        assertTrue(ephemeralAliases(keyStore()).isEmpty())
    }

    private fun assertKeyPolicy(alias: String) {
        val privateKey = keyStore().getKey(alias, null) as PrivateKey
        val keyInfo = KeyFactory.getInstance(privateKey.algorithm, "AndroidKeyStore")
            .getKeySpec(privateKey, KeyInfo::class.java)
        assertNull(privateKey.encoded)
        assertEquals(KeyProperties.ORIGIN_GENERATED, keyInfo.origin)
        assertEquals(256, keyInfo.keySize)
        assertEquals(KeyProperties.PURPOSE_AGREE_KEY, keyInfo.purposes)
        assertFalse(keyInfo.isUserAuthenticationRequired)
        assertTrue(
            keyInfo.securityLevel == KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT ||
                keyInfo.securityLevel == KeyProperties.SECURITY_LEVEL_STRONGBOX,
        )
    }

    private fun generateKey(alias: String): ByteArray =
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore").run {
            initialize(
                KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_AGREE_KEY)
                    .setKeySize(256)
                    .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                    .setUserAuthenticationRequired(false)
                    .setUnlockedDeviceRequired(false)
                    .build(),
            )
            generateKeyPair().public.encoded
        }

    private fun ephemeralAliases(keyStore: KeyStore): List<String> = buildList {
        val aliases = keyStore.aliases()
        while (aliases.hasMoreElements()) {
            val alias = aliases.nextElement()
            if (alias.startsWith(AndroidKeyStoreResponseEntropy.EPHEMERAL_ALIAS_PREFIX)) {
                add(alias)
            }
        }
    }.sorted()

    private fun keyStore(): KeyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
}
