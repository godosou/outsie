package ai.repose.mobile.unlock.responder;

import ai.repose.mobile.unlock.protocol.ChallengeFrame;
import ai.repose.mobile.unlock.protocol.VerifiedMacChallenge;
import java.util.Objects;

/** Capability minted only after a record loaded from the local pairing repository verifies. */
public final class AuthenticatedMacChallenge {
    private final VerifiedMacChallenge verification;
    private final ChallengeFrame challenge;
    private final TrustedPairedMac pairedMac;

    private AuthenticatedMacChallenge(
            VerifiedMacChallenge verification,
            ChallengeFrame challenge,
            TrustedPairedMac pairedMac) {
        this.verification = Objects.requireNonNull(verification, "verification");
        this.challenge = Objects.requireNonNull(challenge, "challenge");
        this.pairedMac = Objects.requireNonNull(pairedMac, "pairedMac");
    }

    static AuthenticatedMacChallenge fromLocalRepository(
            VerifiedMacChallenge verification,
            ChallengeFrame challenge,
            TrustedPairedMac pairedMac) {
        return new AuthenticatedMacChallenge(verification, challenge, pairedMac);
    }

    VerifiedMacChallenge verificationForResponse() {
        return verification;
    }

    ChallengeFrame challengeFrame() {
        return challenge;
    }

    TrustedPairedMac pairedMac() {
        return pairedMac;
    }
}
