package ai.repose.blespike

import org.junit.Assert.assertFalse
import org.junit.Test

/**
 * The window has to close when the pairing succeeds.
 *
 * Not a detail: while it stayed open the screen rebuilt back into the six
 * digits after a successful confirmation, which reads as "nothing happened"
 * and invites a second press -- and the second press reported failure for a
 * pairing that had worked.
 *
 * This asserts the property the screen reads (`Pairing.isOpen`), not the
 * internals underneath it, because the screen is where the damage showed up.
 */
class PairingWindowTest {

    @Test fun `a pairing that never started is not open`() {
        Pairing.stop()
        assertFalse("no session, so no window", Pairing.isOpen)
        assertFalse("and nothing to compare", Pairing.digits != null)
    }

    @Test fun `stopping closes the window and forgets the digits`() {
        Pairing.stop()
        assertFalse(Pairing.isOpen)
        assertFalse(Pairing.digits != null)
        // lastError has to go with it: the pairing screen leads with
        // 「这次没配成」 whenever one is present, so an error left behind by an
        // earlier attempt would caption a successful one.
        assertFalse("a closed window carries no error", Pairing.lastError != null)
    }
}
