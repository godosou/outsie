package ai.repose.mobile.unlock.protocol;

/** Canonical response prehash that may be consumed by the hardware-backed signer. */
public final class PhoneResponseSigningRequest {
    private static final int SHA256_LENGTH = 32;

    private final byte[] prehash;

    private PhoneResponseSigningRequest(byte[] prehash) {
        if (prehash.length != SHA256_LENGTH) {
            throw new IllegalArgumentException("response prehash must be exactly 32 bytes");
        }
        this.prehash = prehash.clone();
    }

    static PhoneResponseSigningRequest fromCanonicalPrehash(byte[] prehash) {
        return new PhoneResponseSigningRequest(prehash);
    }

    public byte[] copyPrehashForSigner() {
        return prehash.clone();
    }
}
