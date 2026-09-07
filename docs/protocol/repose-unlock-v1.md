# Repose Phone Unlock Protocol v1

Status: normative for protocol version 1. All offsets and integers below are byte-oriented. Multi-byte integers are unsigned and encoded in network byte order (big-endian). Implementations MUST reject rather than coerce any non-canonical input.

This protocol mutually authenticates a previously paired Mac and phone for one already-issued, Mac-local lock-screen challenge. The phone authenticates the Challenge with the Mac identity public key saved during pairing before it may construct a Response, sign, or advance its durable counter; the Mac authenticates the Response with the phone identity public key saved during pairing. It does not transmit or store the macOS password, does not unlock FileVault or a logged-out/rebooted Mac, and does not claim relay-resistant ranging. BLE RSSI is only a proximity gate outside this protocol.

## Fixed frame header

Every frame begins with this 12-byte header:

| Absolute offset | Size | Field | Required value |
|---:|---:|---|---|
| 0 | 4 | magic | ASCII `RPUK` (`52 50 55 4b`) |
| 4 | 1 | version | `01` |
| 5 | 1 | kind | `01` Challenge, `02` Response |
| 6 | 2 | flags/reserved | exactly `0000` |
| 8 | 4 | payload length | exact length for the kind |

There are exactly two v1 frame sizes. A Challenge payload is 237 bytes and its frame is 249 bytes. A Response payload is 378 bytes and its frame is 390 bytes. The v1 maximum frame length is therefore 390 bytes. A parser MUST reject an unknown version or kind, nonzero flags/reserved bytes, a payload length other than the exact kind length, a declared length above 378, truncation, and any trailing byte. It MUST inspect the fixed header before allocating and MUST NOT allocate from the untrusted payload length.

## Scalar and identifier types

- `MacId` and `DeviceId` are stable 16-byte, opaque, independently assigned identifiers. They are not UUID text on the wire.
- `pairing_generation` is the Mac-owned unsigned 64-bit generation of the paired-device record. Re-pairing increments it and invalidates protocol/replay state from every older generation.
- `console_uid` and `audit_session_id` are unsigned 32-bit values.
- `lock_epoch`, `ChallengeId`, `counter_floor`, and `counter` are unsigned 64-bit values.
- `SessionBinding` is the exact tuple `(console_uid, audit_session_id, lock_epoch)`; all three components are security-relevant.
- Each nonce is exactly 32 bytes.
- Each P-256 public key is exactly 65-byte SEC1 uncompressed form: prefix `04`, 32-byte X, 32-byte Y. The point MUST be on P-256 and MUST NOT be the identity.
- Each P-256 ECDSA signature is exactly 64-byte IEEE-P1363/raw form `r || s`, with 32-byte big-endian scalars. Both scalars MUST be canonical and nonzero, and `s` MUST be low (`s <= n/2`, where `n` is the P-256 group order).

## Challenge (`kind = 01`)

The Challenge frame is exactly 249 bytes:

| Absolute offset | Payload offset | Size | Field |
|---:|---:|---:|---|
| 0 | - | 12 | fixed header; payload length `000000ed` |
| 12 | 0 | 16 | Mac ID |
| 28 | 16 | 16 | device ID |
| 44 | 32 | 8 | pairing generation |
| 52 | 40 | 4 | console UID |
| 56 | 44 | 4 | audit session ID |
| 60 | 48 | 8 | lock epoch |
| 68 | 56 | 8 | reducer-owned ChallengeId |
| 76 | 64 | 8 | last durable counter known by the Mac (`counter_floor`) |
| 84 | 72 | 4 | relative challenge TTL in milliseconds |
| 88 | 76 | 32 | Mac nonce |
| 120 | 108 | 65 | Mac ephemeral P-256 public key |
| 185 | 173 | 64 | Mac identity-key raw low-S signature |

TTL is a relative hint and is bounded to `1..=60000`. The Mac records `issued_at` and `deadline` from its own monotonic clock when issuing the challenge and MUST require `deadline - issued_at == ttl_ms`. Expiration is `now >= deadline`. The phone's wall clock and monotonic clock are never accepted as expiry evidence. A Mac-local observation earlier than `issued_at` is invalid.

`ChallengeId` is minted by the leave-then-return reducer. It is never selected by BLE input or the phone. The phone MUST verify the Mac identity signature and all paired-record identities before it returns a counter strictly greater than `counter_floor`; zero is never an accepted first counter. A phone implementation MUST expose response construction and counter advancement only from an opaque result of that verification, not from a raw or merely parsed Challenge.

The phone MUST durably coordinate response allocation for each `(MacId, DeviceId, pairing_generation)` pairing. Its durable record is either ready with a counter, or cached with that counter plus the complete `SessionBinding`, `ChallengeId`, `SHA-256(C)` fingerprint of the exact signed 249-byte Challenge frame, and exact 390-byte Response frame. Response construction consumes the opaque authenticated-Challenge capability and allocates `max(durable_phone_counter, counter_floor) + 1` using checked arithmetic. It MUST atomically compare-and-swap the complete cached record and make it durable before returning the Response; a storage error, failed CAS without an identical winner, signing failure, randomness failure, overflow, or failed exact read-back returns no candidate Response.

An exact retransmission of the current signed Challenge returns the exact cached Response without invoking randomness or the phone signer and without advancing the counter. Within one lock epoch, a lower `ChallengeId` is stale and the same `ChallengeId` with any different signed frame, nonce, binding, or other fingerprint input is a conflict. A strictly higher lock epoch may restart the reducer `ChallengeId` sequence, but a full Mac unlock-service restart MUST mint a strictly higher `lock_epoch`; reuse of the same epoch/ID fails closed. Initial pairing and every pairing-generation change require an explicit durable phone-side pairing rotation/reset operation; missing state and response-time generation changes fail closed, so loss of the counter record cannot silently reset it. The durable cache and exact Response bytes survive ordinary phone-process restart.

## Authenticated Response (`kind = 02`)

The Response frame is exactly 390 bytes. It mirrors the complete challenge identity and adds a fresh counter, nonce, ephemeral key, key-confirmation ciphertext, tag, and phone identity-key signature:

| Absolute offset | Payload offset | Size | Field |
|---:|---:|---:|---|
| 0 | - | 12 | fixed header; payload length `0000017a` |
| 12 | 0 | 16 | Mac ID |
| 28 | 16 | 16 | device ID |
| 44 | 32 | 8 | pairing generation |
| 52 | 40 | 4 | console UID |
| 56 | 44 | 4 | audit session ID |
| 60 | 48 | 8 | lock epoch |
| 68 | 56 | 8 | ChallengeId |
| 76 | 64 | 8 | proposed monotonic device counter |
| 84 | 72 | 32 | mirrored Mac nonce |
| 116 | 104 | 32 | phone nonce |
| 148 | 136 | 65 | mirrored Mac ephemeral public key |
| 213 | 201 | 65 | phone ephemeral P-256 public key |
| 278 | 266 | 32 | AES-256-GCM ciphertext |
| 310 | 298 | 16 | AES-256-GCM tag |
| 326 | 314 | 64 | phone identity-key raw low-S signature |

The response is accepted only when every mirrored field matches the locally retained issued challenge, every `SessionBinding` component matches the service's authoritative current locked session, the Mac ID, device ID, and pairing generation match the selected local paired-device record, the local deadline is still open, and `counter > counter_floor`. The phone identity public key is obtained only from that local record; a response never supplies or selects its verification key. Durable anti-replay commitment imposes an additional check described below.

## Domain separation and cryptography

Labels are the exact ASCII bytes shown, without an implicit NUL or length prefix:

| Purpose | Exact label |
|---|---|
| KDF context | `repose-unlock-v1 kdf-context phone-response` |
| HKDF salt | `repose-unlock-v1 hkdf-salt` |
| Mac-to-phone key | `repose-unlock-v1 key mac-to-phone` |
| Mac-to-phone nonce | `repose-unlock-v1 nonce mac-to-phone` |
| Phone-to-Mac key | `repose-unlock-v1 key phone-to-mac` |
| Phone-to-Mac nonce | `repose-unlock-v1 nonce phone-to-mac` |
| Phone-to-Mac AAD | `repose-unlock-v1 aad phone-to-mac` |
| Response plaintext | `repose-unlock-v1 proof phone-to-mac` |
| Mac Challenge signature | `repose-unlock-v1 signature mac-to-phone challenge` |
| Phone signature | `repose-unlock-v1 signature phone-to-mac` |

Let `C` be the exact 249-byte Challenge frame and `R` the exact 390-byte Response frame. Let `C_signed_prefix = C[0..185]`, through and including the Mac ephemeral public key, and let `R_prefix = R[0..278]`, through and including the phone ephemeral public key. Before any phone-side response work:

```
mac_challenge_signature_hash =
    SHA-256(L_signature_mac_challenge || C_signed_prefix)
C[185..249] = P-256-ECDSA-prehash(mac_identity_private_key,
                                  mac_challenge_signature_hash)
```

The Challenge signature therefore covers the final Challenge header, including its final payload length, and every Challenge field preceding the signature. Its algorithm is ECDSA-with-SHA-256 with exactly one hash: the Mac signer signs the displayed 32-byte digest through a prehash API, and the phone verifies that digest through a prehash API. Neither side hashes that digest again. The phone obtains the Mac identity public key only from its local paired-Mac record and MUST reject a wrong ID or pairing generation, malformed or high-S signature, or wrong-key signature before responding, signing, or advancing its counter. A Mac identity signing adapter MUST accept only a purpose-specific, implementation-created Challenge-signing request rather than arbitrary caller bytes; after signing, the Mac implementation MUST validate the raw signature encoding, low-S form, and signature against the expected paired Mac public key before issuing the Challenge.

After that verification, both sides compute an ephemeral P-256 ECDH shared secret from the Mac and phone ephemeral keys. Leading zero bytes of the 32-byte ECDH X coordinate MUST be preserved.

```
context_hash = SHA-256(L_kdf_context || C || R_prefix)
salt         = SHA-256(L_hkdf_salt || context_hash)
PRK          = HKDF-Extract-SHA-256(salt, ecdh_shared_secret)
mac_key      = HKDF-Expand(PRK, L_key_mac_to_phone, 32)
mac_nonce    = HKDF-Expand(PRK, L_nonce_mac_to_phone, 12)
phone_key    = HKDF-Expand(PRK, L_key_phone_to_mac, 32)
phone_nonce  = HKDF-Expand(PRK, L_nonce_phone_to_mac, 12)
aad          = L_aad_phone_to_mac || C || R_prefix
plaintext    = SHA-256(L_proof_phone_to_mac || context_hash)
```

The Response ciphertext/tag is AES-256-GCM encryption of the exact 32-byte `plaintext` with `phone_key`, the derived 12-byte `phone_nonce`, and `aad`. Decryption MUST authenticate the detached 16-byte tag before plaintext comparison. The derived Mac-to-phone material is reserved for a future authenticated acknowledgement in this handshake context and MUST NOT be substituted for the phone-to-Mac material or reused in another direction.

The phone then computes:

```
signature_hash = SHA-256(L_signature_phone_to_mac || C || R[0..326])
signature      = P-256-ECDSA-prehash(phone_identity_private_key,
                                    signature_hash)
```

The signature algorithm is ECDSA-with-SHA-256 with exactly one hash: cross-platform signers sign the displayed 32-byte `signature_hash` using a prehash API, and verifiers verify that digest using a prehash API. They MUST NOT ask an ECDSA-with-SHA-256 convenience API to hash `signature_hash` a second time.

The 64-byte raw low-S signature occupies `R[326..390]`. Verification MUST reject high-S twins before cryptographic verification. Thus the signature covers both headers, IDs, pairing generation, the complete binding, ChallengeId, counter, both nonces, both ephemeral public keys, ciphertext/tag, and the phone-to-Mac direction. Header/AAD, key derivation, encryption, and signature checks all bind this same context and fail closed.

Private scalar generation and nonces MUST use an operating-system CSPRNG through an injected cryptographic RNG interface. Deterministic RNGs and fixed private scalars are permitted only in committed test vectors. Callers and this implementation MUST place owned private-scalar bytes, the copied 32-byte ECDH result, plaintext, AEAD keys, nonces, and HKDF output arrays in zeroizing containers immediately and wipe them on every success and error path; secret-bearing capability values MUST NOT implement ordinary `Debug` or `Clone` APIs. The RustCrypto `aes/zeroize` feature wipes the AES-256 expanded key schedule on drop, while `aes-gcm/zeroize` wipes the temporary GHASH key created by the portable constructor. These upstream features do not promise that every backend-specific GHASH/POLYVAL internal state is wiped. Likewise, `hkdf` 0.12 does not document `ZeroizeOnDrop` for its opaque internal PRK, so its scope is kept minimal and no stronger wiping guarantee is claimed for that third-party internal value.

## Verification, replay ordering, and permits

Successful wire and cryptographic verification yields an opaque `AuthenticatedResponse` bound to one `DeviceId`, `SessionBinding`, `ChallengeId`, and counter. This token is not sufficient to drive the reducer.

For each device, durable replay storage holds exactly one of: no value; an `Active` record containing the last accepted nonzero counter, Mac ID, device ID, pairing generation, complete binding, and ChallengeId; or a `Revoked` tombstone containing Mac ID, device ID, and the revoked pairing generation. The proposed counter MUST be greater than the durable value and MUST advance by at most 1,000,000. Larger jumps are rejected to prevent accidental or hostile exhaustion; there is no requirement that increments equal one. Counter `u64::MAX` can be committed but no later counter can advance it. The same `(Mac ID, device ID, pairing generation, lock epoch, ChallengeId)` can commit at most once even if two independently valid responses propose different counters.

Revocation is itself one exact durable compare-and-swap: no value or `Active(G)` becomes `Revoked(G)`. Repeating the same revocation is explicitly idempotent. A response for generation `G` or older can never initialize or commit from `Revoked(G)`, including after restart. Only a strictly newer paired generation may initialize an `Active` counter by CAS from that exact tombstone. A generation change directly from an `Active` record is rejected and MUST first create the tombstone; stale, lower, or identity-mismatched generations fail closed. Finalization rechecks the durable replacement, but an overlapping revoke can linearize immediately after that read. Therefore `ChallengeVerified` and every derived permit retain the originating Mac/device identity, pairing generation, binding, ChallengeId, durable counter, and replay-guard identity until final consumption; a stale proof is never sufficient by itself after revocation.

The service performs one compare-and-swap from the exact loaded snapshot to the proposed value and does not retry a failed intent. Storage errors and failed/concurrent CAS operations yield no committed proof. The non-Clone/non-Copy committed token owns both the authenticated response and a private receipt containing the originating guard/store instance, exact expected old record or tombstone, and exact durable replacement, including all Mac/device IDs, generation, binding, challenge, and old/new counters. Only the same replay guard may consume and finalize that token. It recalculates the transition, requires the receipt to match exactly, and reads back the exact durable replacement before creating the non-Clone/non-Copy `ChallengeVerified`. Thus a token cannot be replayed, reversed, finalized through another guard/store, or mixed with another device, generation, counter, challenge, or binding. A production durable store remains a trusted adapter: returning a successful CAS means that exact replacement was durably persisted before return.

The reducer may then emit one non-Clone/non-Copy linear permit command. A permit binds the complete `SessionBinding`, ChallengeId, device identity, pairing generation, durable counter, and originating replay guard, and uses only Mac-local creation/expiry times. Public consumption requires that same replay guard as a sealed generation authority. Under the permit store's non-poisoning lock it checks time, the authoritative current session, and an exact durable `Active` record matching every retained provenance field before removing and returning the permit. An unavailable store, wrong guard, changed durable state, or matching/later revoked tombstone yields no consumed capability and destroys the permit. Thus every consume invoked after a revoke has returned observes the tombstone and fails. For genuinely overlapping consume/revoke calls, the durable authority load and revoke CAS are the ordering points: a consume whose authority load wins may succeed; one ordered after the revoke cannot.

Wrong requester binding leaves an otherwise current, unexpired and durably authorized permit intact. Expiry (`now >= expires_at`), authoritative session mismatch (including no logged-in session), nonmonotonic time, or authority failure destroys it. Conditional expiry first validates the currently stored permit against the authoritative optional session while holding the lock, before deciding that a timer handle is stale. Replacement assigns an internal permit generation, so delayed expiry/clear work for an older generation cannot delete a newer valid permit. Clear, revoke, and service restart erase it. Exactly one concurrent consumer can succeed.

Successful consumption returns an opaque non-Clone/non-Copy `ConsumedPermit`; it is the only public event payload that can move the reducer from permit-ready to unlocking. Callers cannot manufacture this event from known binding or ChallengeId fields. Because the one-shot capability cannot be duplicated or recovered after transition, the reducer rebases only this consumed-permit event onto its last observed monotonic time if its caller supplies an older timestamp; all other nonmonotonic events continue to fail. This makes the authoritative transition infallible with respect to clock rollback without reopening a raw-event bypass.

## Failure behavior

All parse, local-context, cryptographic, durable-storage, CAS, and permit errors are terminal for that response and disclose no secret material. Implementations MUST specifically reject: bad magic; unknown version/kind; nonzero flags; bad/deceptive lengths; truncation/trailing bytes; malformed/off-curve keys; zero, stale, or over-policy counters; expired or time-invalid local challenges; wrong IDs/binding/ChallengeId; a malformed, high-S, or wrong-key Mac Challenge signature; AEAD/AAD/tag/plaintext mismatch; a malformed, wrong-key, or high-S phone Response signature; phone-cache fingerprint/order/generation conflicts; phone randomness, signing, storage, CAS, or exact-read-back failure; revoked/stale/unrevoked-new generations; replay-authority mismatch or unavailability; durable storage failure; and failed CAS or receipt finalization.

The fixture files under `protocol/fixtures/v1/` are public, deterministic, test-only interoperability material. Their private scalars MUST never be used by a production pairing.
