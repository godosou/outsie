package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.security.KeyFactory
import java.security.spec.ECPrivateKeySpec
import java.math.BigInteger

/**
 * The phone's pairing arithmetic, checked against vectors OpenSSL produced --
 * not against the Mac, and not against itself.
 *
 * Regenerate with tools/ble-spike/mac/pair-vectors.sh. If these numbers ever
 * need "updating" to make a test pass, the protocol changed and both sides plus
 * the spec have to change with it; editing the expected value alone is how two
 * implementations end up confidently disagreeing.
 */
class PairingCryptoTest {

    private val skM = hex("b96e0676189db5cb28470e4dca48941385603d5e627726997fb97321746e69f9")
    private val skP = hex("e6c595364232f20b66876d41be23313ae40b255668757b3c06878364c3c7c0b2")
    private val pkM = hex("04ceeac1a625c3039e5e3176362e2b257461c66a8b3a5b23dbe67424c84b3f1202f34dd8c37e726e0474d28ff984c6fad4d3158baae761a1cfa50d3575d4d2af21")
    private val pkP = hex("04bba0ac866c040dee63395dc7ea9cd2ae65df7475c35295da07264de36a85dcc78f30b930a1af3147ac63a0d902593e53a78f7c9ab8773670562f50afea4bf035")
    private val nm = hex("000102030405060708090a0b0c0d0e0f")
    private val np = hex("f0e0d0c0b0a090807060504030201000")
    // v3 folds the key slot and the phone's identity into the transcript, so
    // every expected value below moved. Recomputed with Python's hashlib/hmac,
    // the same third opinion the Mac's self-test is measured against.
    private val keyId = 37
    private val phoneId = hex("0badc0de0badc0de")
    private val wantCommit = "952be98c1c4aa1d54d4c416d56b72c333277976c70db3e6306af6c4f2ec91c6c"
    private val wantSasHash = "c254c9a5bfcecf268768e85aa90ae6e4509750b153f98181cf8b9f3efbe5f896"
    private val wantDigits = "336549"
    private val wantEcdhX = "8264224d7eb11f8f5240fe1b94a14c7202a9c493cdfc5fd406f6a46d681f6f94"
    private val wantKey = "1c58d63ba975bb90784b4bc442d79b6841ccf58c5a446b2a679f015502f4edcb"

    private fun hex(s: String) = ByteArray(s.length / 2) {
        s.substring(it * 2, it * 2 + 2).toInt(16).toByte()
    }
    private fun hex(b: ByteArray) = b.joinToString("") { "%02x".format(it) }

    private fun privateKey(scalar: ByteArray) =
        KeyFactory.getInstance("EC").generatePrivate(
            ECPrivateKeySpec(BigInteger(1, scalar), PairingCrypto.p256),
        )

    @Test fun commitmentMatchesOpenssl() {
        assertEquals(wantCommit, hex(PairingCrypto.commitment(pkM, pkP, np)))
    }

    @Test fun transcriptHashMatchesOpenssl() {
        assertEquals(wantSasHash, hex(PairingCrypto.sasHash(pkM, pkP, nm, np, keyId, phoneId)))
    }

    @Test fun sixDigitsMatchOpenssl() {
        assertEquals(wantDigits, PairingCrypto.sasDigits(PairingCrypto.sasHash(pkM, pkP, nm, np, keyId, phoneId)))
    }

    @Test fun ecdhAgreesInBothDirections() {
        // If these ever differ, each side derives a different key and the only
        // symptom is a phone whose beacons never verify -- indistinguishable
        // from being out of range.
        val xm = PairingCrypto.ecdhX(privateKey(skM), PairingCrypto.decodePublicKey(pkP))
        val xp = PairingCrypto.ecdhX(privateKey(skP), PairingCrypto.decodePublicKey(pkM))
        assertEquals(wantEcdhX, hex(xm))
        assertEquals(wantEcdhX, hex(xp))
    }

    @Test fun derivedKeyMatchesOpenssl() {
        val x = PairingCrypto.ecdhX(privateKey(skM), PairingCrypto.decodePublicKey(pkP))
        val salt = PairingCrypto.sasHash(pkM, pkP, nm, np, keyId, phoneId)
        assertEquals(wantKey, hex(PairingCrypto.deriveKey(x, salt)))
    }

    @Test fun aDifferentNonceBreaksTheCommitment() {
        // The step a man in the middle has to beat, so it is asserted rather
        // than implied by the happy path.
        val other = np.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() }
        assertNotEquals(wantCommit, hex(PairingCrypto.commitment(pkM, pkP, other)))
    }

    @Test fun aSubstitutedPublicKeyChangesTheDigits() {
        val other = pkM.copyOf().also { it[1] = (it[1].toInt() xor 1).toByte() }
        assertNotEquals(wantDigits, PairingCrypto.sasDigits(PairingCrypto.sasHash(other, pkP, nm, np, keyId, phoneId)))
    }

    @Test fun aPointOffTheCurveIsRefused() {
        // The classic invalid-curve attack: a crafted "public key" that leaks
        // the private scalar through repeated agreements. The peer's key arrives
        // unauthenticated by design, so this is exactly the untrusted input.
        val bogus = pkP.copyOf().also { it[64] = (it[64].toInt() xor 1).toByte() }
        try {
            PairingCrypto.decodePublicKey(bogus)
            fail("a point off the curve was accepted")
        } catch (expected: IllegalArgumentException) {
            // expected
        }
    }

    @Test fun malformedPeerKeysAreRefused() {
        for (bad in listOf(ByteArray(0), ByteArray(65), ByteArray(64) { 4 })) {
            try {
                PairingCrypto.decodePublicKey(bad)
                fail("accepted a malformed peer key of size ${bad.size}")
            } catch (expected: IllegalArgumentException) {
                // expected
            }
        }
    }

    @Test fun leadingZerosInTheSharedXAreKept() {
        // A trimmed X agrees with the other side 255 times out of 256 and fails
        // silently the other time. Fixed-width output is asserted directly.
        val x = PairingCrypto.ecdhX(privateKey(skM), PairingCrypto.decodePublicKey(pkP))
        assertEquals(32, x.size)
    }

    @Test fun aRewrittenKeySlotChangesTheDigits() {
        // The reason the slot number is in the transcript at all. A man in the
        // middle who could change it unnoticed would have the Mac install this
        // key into ANOTHER phone's slot, replacing that phone's key with one he
        // chose -- and the six digits, the only thing a person checks, would
        // still match on both screens.
        assertNotEquals(
            wantDigits,
            PairingCrypto.sasDigits(PairingCrypto.sasHash(pkM, pkP, nm, np, keyId xor 1, phoneId)),
        )
    }

    @Test fun aRewrittenPhoneIdentityChangesTheDigits() {
        val other = phoneId.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() }
        assertNotEquals(
            wantDigits,
            PairingCrypto.sasDigits(PairingCrypto.sasHash(pkM, pkP, nm, np, keyId, other)),
        )
    }

    @Test fun theV3LabelsAreNotTheV2Ones() {
        // A transcript from a recorded v2 session must not verify as a v3 one.
        for (label in listOf(PairingCrypto.COMMIT_LABEL, PairingCrypto.SAS_LABEL, PairingCrypto.KDF_LABEL)) {
            assertTrue("$label still says v2", label.contains("v3"))
        }
    }

    @Test fun theConsoleKeyIsADifferentKeyFromTheSameExchange() {
        // The separation is the point: one checks a button list, the other
        // opens a Mac. If they were equal, leaking the catalogue key -- which
        // lives in the app's data directory on the Mac, not in root's -- would
        // hand over the presence key with it.
        val x = hex(wantEcdhX)
        val t = PairingCrypto.sasHash(pkM, pkP, nm, np, keyId, phoneId)
        val presence = hex(PairingCrypto.deriveKey(x, t))
        val console = hex(PairingCrypto.deriveKey(x, t, PairingCrypto.CONSOLE_KDF_LABEL))
        assertEquals(wantKey, presence)
        assertEquals("ba6797f63dd6a57b39bcd0bb509e14481303d8ef505d35e27e7d21a8fd37181d", console)
        assertNotEquals(presence, console)
    }
}
