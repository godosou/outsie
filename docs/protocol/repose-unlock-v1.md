# Repose Phone Unlock Protocol v1

Status: normative for protocol version 1. All offsets and integers below are byte-oriented. Multi-byte integers are unsigned and encoded in network byte order (big-endian). Implementations MUST reject rather than coerce any non-canonical input.

This protocol authenticates a previously paired phone to the Mac for one already-issued, Mac-local lock-screen challenge. It does not transmit or store the macOS password, does not unlock FileVault or a logged-out/rebooted Mac, and does not claim relay-resistant ranging. BLE RSSI is only a proximity gate outside this protocol.

## Fixed frame header

Every frame begins with this 12-byte header:

| Absolute offset | Size | Field | Required value |
|---:|---:|---|---|
| 0 | 4 | magic | ASCII `RPUK` (`52 50 55 4b`) |
| 4 | 1 | version | `01` |
| 5 | 1 | kind | `01` Challenge, `02` Response |
| 6 | 2 | flags/reserved | exactly `0000` |
| 8 | 4 | payload length | exact length for the kind |

There are exactly two v1 frame sizes. A Challenge payload is 173 bytes and its frame is 185 bytes. A Response payload is 378 bytes and its frame is 390 bytes. The v1 maximum frame length is therefore 390 bytes. A parser MUST reject an unknown version or kind, nonzero flags/reserved bytes, a payload length other than the exact kind length, a declared length above 378, truncation, and any trailing byte. It MUST inspect the fixed header before allocating and MUST NOT allocate from the untrusted payload length.

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

The Challenge frame is exactly 185 bytes:

| Absolute offset | Payload offset | Size | Field |
|---:|---:|---:|---|
| 0 | - | 12 | fixed header; payload length `000000ad` |
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

TTL is a relative hint and is bounded to `1..=60000`. The Mac records `issued_at` and `deadline` from its own monotonic clock when issuing the challenge and MUST require `deadline - issued_at == ttl_ms`. Expiration is `now >= deadline`. The phone's wall clock and monotonic clock are never accepted as expiry evidence. A Mac-local observation earlier than `issued_at` is invalid.

`ChallengeId` is minted by the leave-then-return reducer. It is never selected by BLE input or the phone. The phone MUST return a counter strictly greater than `counter_floor`; zero is never an accepted first counter.

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
| Phone signature | `repose-unlock-v1 signature phone-to-mac` |

Let `C` be the exact 185-byte Challenge frame and `R` the exact 390-byte Response frame. Let `R_prefix = R[0..278]`, through and including the phone ephemeral public key. Both sides compute an ephemeral P-256 ECDH shared secret from the Mac and phone ephemeral keys. Leading zero bytes of the 32-byte ECDH X coordinate MUST be preserved.

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

Private scalar generation and nonces MUST use an operating-system CSPRNG through an injected cryptographic RNG interface. Deterministic RNGs and fixed private scalars are permitted only in committed test vectors. Private/ECDH/HKDF/AEAD material MUST be zeroized on drop and MUST NOT implement ordinary `Debug` or `Clone` APIs.

## Verification, replay ordering, and permits

Successful wire and cryptographic verification yields an opaque `AuthenticatedResponse` bound to one `DeviceId`, `SessionBinding`, `ChallengeId`, and counter. This token is not sufficient to drive the reducer.

For each device, durable replay storage holds either no value (semantically counter 0) or the last accepted nonzero counter together with Mac ID, pairing generation, lock epoch, and ChallengeId. The proposed counter MUST be greater than the durable value and MUST advance by at most 1,000,000. Larger jumps are rejected to prevent accidental or hostile exhaustion; there is no requirement that increments equal one. Counter `u64::MAX` can be committed but no later counter can advance it. The same `(Mac ID, device ID, pairing generation, lock epoch, ChallengeId)` can commit at most once even if two independently valid responses propose different counters. The service performs one compare-and-swap from the exact loaded snapshot to the proposed value and does not retry a failed intent. Storage errors and failed/concurrent CAS operations yield no committed proof. Private intent/receipt values bind the store instance, Mac/device IDs, pairing generation, expected old state, new counter, binding, and challenge. Only the opaque, non-Clone/non-Copy token returned after a successful durable CAS can create the non-Clone/non-Copy `ChallengeVerified`; it cannot be reversed or mixed with another device, counter, challenge, store, or binding.

The reducer may then emit one non-Clone/non-Copy linear permit command. A permit binds the complete `SessionBinding` and ChallengeId and uses only Mac-local creation/expiry times. It is consumed atomically under one non-poisoning lock. Wrong requester binding leaves an otherwise current, unexpired permit intact. Expiry (`now >= expires_at`) or mismatch with the authoritative current session destroys it. Replacement assigns an internal permit generation: delayed expiry/clear work for an older generation cannot delete a newer permit. Clear, revoke, and service restart erase it. Exactly one concurrent consumer can succeed.

## Failure behavior

All parse, local-context, cryptographic, durable-storage, CAS, and permit errors are terminal for that response and disclose no secret material. Implementations MUST specifically reject: bad magic; unknown version/kind; nonzero flags; bad/deceptive lengths; truncation/trailing bytes; malformed/off-curve keys; zero, stale, or over-policy counters; expired or time-invalid local challenges; wrong IDs/binding/ChallengeId; AEAD/AAD/tag/plaintext mismatch; malformed, wrong-key, or high-S signatures; durable storage failure; and failed CAS.

The fixture files under `protocol/fixtures/v1/` are public, deterministic, test-only interoperability material. Their private scalars MUST never be used by a production pairing.
