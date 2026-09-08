# Repose One-Time Pairing Payload v1

Status: normative for pairing payload version 1. This format is independent from the `RPUK` Challenge and Response frames in `repose-unlock-v1.md`; implementations MUST NOT pass a pairing payload to an unlock-frame parser or reuse its one-time secret as an unlock-protocol nonce or key.

All offsets and lengths are byte-oriented. Multi-byte integers are unsigned and encoded in network byte order (big-endian). A decoder MUST reject rather than coerce any non-canonical input.

## BLE GATT profile

The Mac is the BLE peripheral and the Android phone is the BLE central. Both implementations use these exact UUIDs:

| Role | UUID |
|---|---|
| Repose service | `A53E0001-7A6B-4D59-9F2E-5245504F5345` |
| Control characteristic | `A53E0002-7A6B-4D59-9F2E-5245504F5345` |
| Status characteristic | `A53E0003-7A6B-4D59-9F2E-5245504F5345` |

The pairing payload codec only defines the bytes exchanged during setup. It does not grant a BLE connection, authenticate an unlock Challenge, or change the `RPUK` wire format.

## Frame

The frame contains an 8-byte fixed header followed by a bounded payload:

| Absolute offset | Size | Field | Canonical value |
|---:|---:|---|---|
| 0 | 4 | magic | ASCII `RPPK` (`52 50 50 4b`) |
| 4 | 1 | version | `01` |
| 5 | 1 | flags/reserved | exactly `00` |
| 6 | 2 | payload length | exact number of bytes following the header, `138 + mac_name_length` |
| 8 | 16 | pairing session ID | opaque, nonzero 16-byte value |
| 24 | 8 | expiry | nonzero Unix epoch milliseconds |
| 32 | 16 | Mac ID | stable, opaque, nonzero 16-byte value |
| 48 | 65 | Mac identity public key | P-256 SEC1 uncompressed form; prefix `04`, on curve, not the identity |
| 113 | 32 | one-time pairing secret | CSPRNG output; not all zero |
| 145 | 1 | Mac name length | `1..=64`, in bytes |
| 146 | variable | Mac name | exactly the declared number of bytes, strict UTF-8, containing no Unicode control character |

The minimum frame length is 147 bytes and the maximum is 210 bytes. The declared payload length MUST be between 139 and 202 bytes and MUST equal both `138 + mac_name_length` and the exact bytes remaining after the header. A decoder MUST inspect the fixed header and bounded length before reading variable data. It MUST reject truncation, trailing bytes, an unknown version, nonzero flags, invalid identifiers, an invalid public key, an invalid secret, an empty or overlong name, malformed UTF-8, and control characters.

## Lifetime and one-time use

The Mac creates `pairing_session_id` and `one_time_pairing_secret` using the operating-system CSPRNG, gives the session a short absolute expiry, and retains server-side state bound to that exact session ID, secret, Mac ID, and identity key. The Android client MUST reject the session when its trusted wall-clock observation is at or past the expiry.

Successful confirmation atomically consumes the server-side session. A second confirmation, a confirmation after expiry, or any field mismatch fails closed. Merely decoding this payload does not consume or authenticate the session. Neither implementation may log, include in ordinary debug output, or persist the one-time secret beyond the pending pairing transaction. Secret-bearing buffers should be wiped when their platform permits.

## Interoperability fixture

`protocol/fixtures/v1/pairing-payload.bin` is the canonical 164-byte v1 fixture. It is public, deterministic, and test-only. Its session ID, expiry, key, and secret MUST never be used by a production pairing.
