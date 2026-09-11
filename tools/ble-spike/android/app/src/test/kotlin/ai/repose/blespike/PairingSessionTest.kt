package ai.repose.blespike

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The responder's ordering rules. These are not bookkeeping -- each one is a
 * step a man in the middle has to get past, so each is asserted on its own
 * rather than implied by a happy path that happens to go in order.
 */
class PairingSessionTest {

    /** What PairingSession uses when the caller does not choose. */
    private val DEFAULT_KEY_ID = SpikeContract.PRESENCE_KEY_ID
    private val DEFAULT_PHONE_ID = ByteArray(8)


    private fun macSide(): Triple<ByteArray, ByteArray, java.security.KeyPair> {
        val kp = PairingCrypto.newEphemeralKeyPair()
        return Triple(PairingCrypto.encodePublicKey(kp.public), PairingCrypto.randomBytes(16), kp)
    }

    @Test fun aFullExchangeAgreesOnDigitsAndKey() {
        val (pkM, nm, macKeys) = macSide()
        val phone = PairingSession()

        assertTrue(phone.receiveMacKey(pkM))
        val p1 = phone.keyAndCommitment()!!
        val pkP = p1.copyOfRange(0, 65)
        val commit = p1.copyOfRange(65, 97)

        assertTrue(phone.receiveMacNonce(nm))
        val np = phone.revealNonce()!!

        // The Mac's checks, done here the way the Mac will do them.
        assertArrayEquals(
            "the commitment must match the revealed nonce",
            PairingCrypto.commitment(pkM, pkP, np), commit,
        )
        val macDigits = PairingCrypto.sasDigits(PairingCrypto.sasHash(pkM, pkP, nm, np, DEFAULT_KEY_ID, DEFAULT_PHONE_ID))
        assertEquals("both sides must show the same six digits", macDigits, phone.digits)

        val macKey = PairingCrypto.deriveKey(
            PairingCrypto.ecdhX(macKeys.private, PairingCrypto.decodePublicKey(pkP)),
            PairingCrypto.sasHash(pkM, pkP, nm, np, DEFAULT_KEY_ID, DEFAULT_PHONE_ID),
        )
        assertArrayEquals("both sides must derive the same key", macKey, phone.deriveKey())
    }

    @Test fun theCommitmentIsNotHandedOutBeforeTheMacKeyArrives() {
        // It covers PK_M. Producing one earlier would be committing to a Mac we
        // have not been told about, which commits to nothing.
        assertNull(PairingSession().keyAndCommitment())
    }

    @Test fun theNonceIsNotRevealedBeforeTheMacNonceArrives() {
        // The step that matters most. A phone that reveals first has given its
        // nonce away, and a man in the middle can then pick his own knowing ours
        // and drive both screens to the same digits.
        val (pkM, _, _) = macSide()
        val phone = PairingSession()
        phone.receiveMacKey(pkM)
        assertNull("Np must not be available before Nm", phone.revealNonce())
        assertNull("and neither must the key", phone.deriveKey())
    }

    @Test fun aKeyOffTheCurveFailsTheSession() {
        val (pkM, _, _) = macSide()
        val bogus = pkM.copyOf().also { it[64] = (it[64].toInt() xor 1).toByte() }
        val phone = PairingSession()
        assertFalse(phone.receiveMacKey(bogus))
        assertEquals(PairingSession.Stage.Failed, phone.stage)
        // And a failed session stays failed rather than quietly accepting a
        // second, well-formed attempt on the same ephemerals.
        assertFalse(phone.receiveMacKey(pkM))
    }

    @Test fun aSecondMacKeyIsRefused() {
        // Re-keying mid-session would let an attacker who arrives late replace
        // the key the commitment was made against.
        val (pkM, _, _) = macSide()
        val (other, _, _) = macSide()
        val phone = PairingSession()
        assertTrue(phone.receiveMacKey(pkM))
        assertFalse(phone.receiveMacKey(other))
    }

    @Test fun aWrongLengthNonceIsRefused() {
        val (pkM, _, _) = macSide()
        val phone = PairingSession()
        phone.receiveMacKey(pkM)
        assertFalse(phone.receiveMacNonce(ByteArray(15)))
        assertFalse(phone.receiveMacNonce(ByteArray(32)))
        assertNull(phone.digits)
    }

    @Test fun twoSessionsNeverProduceTheSameNonceOrKey() {
        // Ephemerals are per session. Reuse would make an old transcript
        // replayable against a new pairing.
        val (pkM, nm, _) = macSide()
        val a = PairingSession().apply { receiveMacKey(pkM); receiveMacNonce(nm) }
        val b = PairingSession().apply { receiveMacKey(pkM); receiveMacNonce(nm) }
        assertNotNull(a.revealNonce()); assertNotNull(b.revealNonce())
        assertFalse(a.revealNonce()!!.contentEquals(b.revealNonce()!!))
        assertFalse(a.publicKey.contentEquals(b.publicKey))
        assertFalse(a.deriveKey()!!.contentEquals(b.deriveKey()!!))
    }
}
