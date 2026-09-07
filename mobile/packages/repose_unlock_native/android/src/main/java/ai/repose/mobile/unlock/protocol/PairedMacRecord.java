package ai.repose.mobile.unlock.protocol;

import java.security.PublicKey;
import java.util.Arrays;
import java.util.Objects;

/** Trusted pairing material. Only code in the protocol package can construct it. */
public final class PairedMacRecord {
    private static final int IDENTIFIER_LENGTH = 16;

    private final byte[] macId;
    private final byte[] deviceId;
    private final long pairingGenerationBits;
    private final PublicKey macIdentityPublicKey;

    PairedMacRecord(
            byte[] macId,
            byte[] deviceId,
            long pairingGenerationBits,
            PublicKey macIdentityPublicKey) {
        Objects.requireNonNull(macId, "macId");
        Objects.requireNonNull(deviceId, "deviceId");
        if (macId.length != IDENTIFIER_LENGTH || deviceId.length != IDENTIFIER_LENGTH) {
            throw new IllegalArgumentException("pairing identifiers must be exactly 16 bytes");
        }
        this.macId = macId.clone();
        this.deviceId = deviceId.clone();
        this.pairingGenerationBits = pairingGenerationBits;
        this.macIdentityPublicKey = Objects.requireNonNull(
                macIdentityPublicKey,
                "macIdentityPublicKey");
    }

    boolean matches(ChallengeFrame challenge) {
        return Arrays.equals(macId, challenge.getMacId())
                && Arrays.equals(deviceId, challenge.getDeviceId())
                && challenge.hasPairingGenerationBits(pairingGenerationBits);
    }

    PublicKey identityPublicKey() {
        return macIdentityPublicKey;
    }
}
