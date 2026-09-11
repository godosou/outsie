package ai.repose.blespike

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.math.BigInteger
import java.security.KeyFactory
import java.security.KeyPair
import java.security.spec.ECPrivateKeySpec
import java.security.spec.ECPublicKeySpec
import java.security.spec.ECPoint

/**
 * The responder's WIRE OUTPUT against OpenSSL, and the man-in-the-middle it is
 * supposed to defeat.
 *
 * PairingCryptoTest checks the primitives. This checks the layer above: given
 * the Mac's key and nonce, does PairingSession put the right bytes on P1 and P2?
 * Two implementations can agree on SHA-256 and still disagree on what goes into
 * it, and the symptom of that is a commitment check failing on a Mac with
 * nobody in the middle -- which reads as an attack rather than a bug.
 */
class PairingExchangeTest {

    private fun hex(s: String) = ByteArray(s.length / 2) {
        s.substring(it * 2, it * 2 + 2).toInt(16).toByte()
    }
    private fun hex(b: ByteArray) = b.joinToString("") { "%02x".format(it) }

    private val skP = hex("e6c595364232f20b66876d41be23313ae40b255668757b3c06878364c3c7c0b2")
    private val pkM = hex("04ceeac1a625c3039e5e3176362e2b257461c66a8b3a5b23dbe67424c84b3f1202f34dd8c37e726e0474d28ff984c6fad4d3158baae761a1cfa50d3575d4d2af21")
    private val pkP = hex("04bba0ac866c040dee63395dc7ea9cd2ae65df7475c35295da07264de36a85dcc78f30b930a1af3147ac63a0d902593e53a78f7c9ab8773670562f50afea4bf035")
    private val nm = hex("000102030405060708090a0b0c0d0e0f")
    private val np = hex("f0e0d0c0b0a090807060504030201000")
    private val wantCommit = "7161008840f0d9173b7c81381c9e5d81216c004373e08bbd597bdea5fd7018d0"
    private val wantDigits = "063529"
    private val wantKey = "d4cdf8ede653c929321d134fea6e4382bcb1a02a37401392316d60a90ff532ee"

    /** The phone side pinned to the vector's ephemeral, so the wire bytes are fixed. */
    private fun fixedSession(): PairingSession {
        val kf = KeyFactory.getInstance("EC")
        val priv = kf.generatePrivate(ECPrivateKeySpec(BigInteger(1, skP), PairingCrypto.p256))
        val point = PairingCrypto.decodePublicKey(pkP).w
        val pub = kf.generatePublic(ECPublicKeySpec(ECPoint(point.affineX, point.affineY), PairingCrypto.p256))
        return PairingSession(KeyPair(pub, priv), np)
    }

    @Test fun p1CarriesExactlyTheBytesOpensslExpects() {
        val phone = fixedSession()
        assertTrue(phone.receiveMacKey(pkM))
        val p1 = phone.keyAndCommitment()!!
        assertEquals("P1 must be PK_P(65) ‖ Cp(32)", 97, p1.size)
        assertEquals(hex(pkP), hex(p1.copyOfRange(0, 65)))
        assertEquals(wantCommit, hex(p1.copyOfRange(65, 97)))
    }

    @Test fun p2AndTheDigitsMatchOpenssl() {
        val phone = fixedSession()
        phone.receiveMacKey(pkM)
        phone.keyAndCommitment()
        assertTrue(phone.receiveMacNonce(nm))
        assertEquals(hex(np), hex(phone.revealNonce()!!))
        assertEquals(wantDigits, phone.digits)
        assertEquals(wantKey, hex(phone.deriveKey()!!))
    }

    /**
     * The claim the whole design rests on.
     *
     * A man in the middle runs two sessions: one with the Mac using his own key,
     * one with the phone using another. He can read and rewrite everything. What
     * he cannot do is make both screens show the same six digits, because each
     * side's transcript contains the key it actually saw -- his, not the real
     * peer's.
     */
    @Test fun aManInTheMiddleCannotMakeBothScreensAgree() {
        val realMac = PairingCrypto.newEphemeralKeyPair()
        val pkMacReal = PairingCrypto.encodePublicKey(realMac.public)
        val nmReal = PairingCrypto.randomBytes(16)

        // The attacker, with a key of his own for each leg.
        val towardPhone = PairingCrypto.newEphemeralKeyPair()
        val pkAttackerToPhone = PairingCrypto.encodePublicKey(towardPhone.public)
        val towardMac = PairingCrypto.newEphemeralKeyPair()
        val pkAttackerToMac = PairingCrypto.encodePublicKey(towardMac.public)
        val nmAttacker = PairingCrypto.randomBytes(16)

        // Phone leg: the phone believes it is talking to the attacker's key.
        val phone = PairingSession()
        assertTrue(phone.receiveMacKey(pkAttackerToPhone))
        val p1 = phone.keyAndCommitment()!!
        val pkPhone = p1.copyOfRange(0, 65)
        assertTrue(phone.receiveMacNonce(nmAttacker))
        val npPhone = phone.revealNonce()!!

        // Mac leg: the attacker must commit to a nonce BEFORE seeing the Mac's,
        // exactly as the phone does -- the protocol gives him no better position
        // than the party he is impersonating.
        val npAttacker = PairingCrypto.randomBytes(16)
        val commitToMac = PairingCrypto.commitment(pkMacReal, pkAttackerToMac, npAttacker)
        assertEquals(32, commitToMac.size)

        val macDigits = PairingCrypto.sasDigits(
            PairingCrypto.sasHash(pkMacReal, pkAttackerToMac, nmReal, npAttacker),
        )
        val phoneDigits = phone.digits!!

        // One in a million says they could collide by luck; with random nonces
        // this asserts the overwhelming case, and the protocol's answer to the
        // unlucky one is that a mismatch aborts and cannot be retried.
        assertNotEquals(
            "a man in the middle must not be able to show both screens the same digits",
            macDigits, phoneDigits,
        )

        // And even if they had collided, the two legs derive different keys, so
        // the relay still does not join up.
        val macSideKey = PairingCrypto.deriveKey(
            PairingCrypto.ecdhX(realMac.private, PairingCrypto.decodePublicKey(pkAttackerToMac)),
            PairingCrypto.sasHash(pkMacReal, pkAttackerToMac, nmReal, npAttacker),
        )
        val phoneSideKey = PairingCrypto.deriveKey(
            PairingCrypto.ecdhX(towardPhone.private, PairingCrypto.decodePublicKey(pkPhone)),
            PairingCrypto.sasHash(pkAttackerToPhone, pkPhone, nmAttacker, npPhone),
        )
        assertNotEquals(hex(macSideKey), hex(phoneSideKey))
    }

    /**
     * The other thing the commitment buys: the attacker cannot wait to see the
     * Mac's nonce and then pick one that lands on the digits he wants.
     */
    @Test fun choosingANonceAfterTheFactDoesNotHelp() {
        val phone = fixedSession()
        phone.receiveMacKey(pkM)
        val p1 = phone.keyAndCommitment()!!
        val commitment = p1.copyOfRange(65, 97)
        phone.receiveMacNonce(nm)

        // Try a thousand alternative nonces; none can satisfy a commitment made
        // before they existed.
        var matched = 0
        repeat(1000) {
            val forged = PairingCrypto.randomBytes(16)
            if (PairingCrypto.commitment(pkM, pkP, forged).contentEquals(commitment)) matched++
        }
        assertEquals("no substituted nonce may satisfy the commitment", 0, matched)
    }
}
