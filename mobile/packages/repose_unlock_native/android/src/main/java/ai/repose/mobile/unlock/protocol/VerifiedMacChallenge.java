package ai.repose.mobile.unlock.protocol;

import java.util.Objects;

/** Capability minted only after the paired Mac challenge has been authenticated. */
public final class VerifiedMacChallenge {
    private final ChallengeFrame challenge;

    private VerifiedMacChallenge(ChallengeFrame challenge) {
        this.challenge = Objects.requireNonNull(challenge, "challenge");
    }

    static VerifiedMacChallenge fromAuthenticatedChallenge(ChallengeFrame challenge) {
        return new VerifiedMacChallenge(challenge);
    }

    ChallengeFrame challengeForResponse() {
        return challenge;
    }
}
