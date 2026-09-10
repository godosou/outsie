# Authenticated Presence — design (Android-first)

**Status:** design + scaffold boundary. Closes deferred security item #1.
**Date:** 2026-09-10 · **Branch:** `feat/phone-unlock-walking-skeleton`
**Supersedes the trust model of:** the B-spike, where *any* Android broadcasting the
public service UUID `7265706F-7365-0001-8000-00805F9B34FB` counted as "present" and could
unlock the Mac. There was **no authentication**. This document specifies how only the
**paired** phone counts.

---

## 0. Problem statement and the shape of the fix

Today the presence decision is "I saw an advertisement carrying a well-known, public UUID."
That UUID is not a secret — anyone can read it off the air with `nRF Connect` and rebroadcast
it. So today's "present" means "some Android is near," not "*my* phone is near."

The fix has a hard shape imposed by the BLE spike and §9 of the interaction design:

- **The proximity decision must not require a GATT connection/handshake.** B1 measured
  discovery p95 ≈ 5.4 s, plus connect/read ≈ 1.5 s — a challenge/response over GATT blows the
  3 s unlock budget. **The authenticator must ride inside the advertisement itself** (service
  data), verifiable by the scanner *without connecting*.
- **Identity cannot be the Bluetooth address.** Android rotates its BLE private address (RPA)
  roughly every 15 min; CoreBluetooth's `peripheral.identifier` changes with it. Identity is
  **the paired key**, never the address. (Already the design's stated position.)
- **The 128-bit UUID eats ~18 of 31 adv bytes.** The authenticator must fit the remaining
  budget → we switch to a **16-bit service UUID** and carry a **truncated HMAC** (8–16 bytes)
  in service data.
- **First release is Android only.** iOS cannot put rotating data in a backgrounded
  advertisement (§9.1). We do **not** design for iOS here.
- **No invented crypto.** Every primitive below is a vetted standard, reusing the codex
  `codex/phone-proximity-unlock` prior art (P-256, Android Keystore, HKDF-SHA-256, SHA-256
  domain-separated transcripts). The one primitive codex lacks — HMAC-SHA-256 — is a
  standard, and it is the only safe-to-truncate option that fits the byte budget.

Two layers, deliberately separated:

| Layer | When | Channel | Guarantee |
|---|---|---|---|
| **Pairing bootstrap** (`repose-pair-v1`) | once, in person | QR + BLE GATT + human-compared SAS | MITM-resistant establishment of identity keys + a shared presence key `K` |
| **Presence proof** (`repose-presence-v1`) | continuously, steady state | advertisement service-data only | only the holder of `K` can mint a currently-valid beacon |

The pairing layer is expensive and interactive; it runs once. The presence layer is cheap and
connectionless; it runs forever. `K` is the bridge: minted under the MITM-resistant SAS, then
used to authenticate every beacon.

---

## 1. Pairing bootstrap — `repose-pair-v1`

### 1.1 Why a new protocol, and what we reuse

Codex specifies the *unlock* challenge/response (`repose-unlock-v1`), which **assumes**
identity public keys "saved during pairing." It never specifies the pairing bootstrap itself:
`pairing_controller.dart` is a QR-payload + "confirm device name" UI placeholder;
`begin_pairing` / `confirm_pairing` are empty Tauri stubs. So the authenticated key exchange
must be **specified fresh** — but built entirely from codex's primitives, inventing no crypto.

Reused verbatim:
- P-256 identity keypairs, hardware-backed and non-exportable on Android
  (`AndroidKeyStoreSigner`, `SigningKeyPolicy`, alias
  `ai.repose.mobile.unlock.identity.p256.v1`, StrongBox-preferred/TEE-fallback,
  `DIGEST_NONE` + prehash-SHA-256).
- SEC1-uncompressed 65-byte public-key format (`0x04‖X(32)‖Y(32)`), on-curve / non-identity
  validation.
- HKDF-SHA-256 with **exact-ASCII, domain-separated** labels; leading-zero preservation on
  the ECDH X coordinate.
- The `TrustedPairedMac` record shape (`macId`, `deviceId`, `pairingGeneration`,
  `macIdentityPublicKey`, `phoneIdentityPublicKey`) and the `pairing_generation` /
  `Revoked(G)` tombstone discipline from `replay.rs`.

The construction adopted is **Bluetooth LE Secure Connections "Numeric Comparison"**
(equivalently ZRTP SAS / Vaudenay short-authenticated-string): commitment-before-reveal, then
a human compares a short code shown identically on both screens. This is exactly the interaction
spec's §5.7 pairing ("不一致就别继续" — MITM guard). It is a published construction; we invent
nothing.

### 1.2 The two out-of-band channels

- **QR (Mac screen → phone camera): authentic, one-directional, Mac→phone.** An attacker on
  BLE cannot alter the pixels the Mac renders on its own screen. QR is **not confidential**
  (it could be photographed), so it carries **only public data**: Mac's ephemeral + identity
  *public* keys and identifiers.
- **SAS numeric code (shown identically on both screens, human compares): ~20-bit,
  authenticates the phone→Mac direction** that QR cannot cover (the Mac has no camera on the
  phone).

### 1.3 The exchange

Roles: **Mac = initiator, Phone = responder.** Ephemeral P-256 ECDH keys `PK_M/sk_M`,
`PK_P/sk_P` (single-use, discarded after pairing). Long-term identity keys `IK_M` (Mac,
Keychain/Secure Enclave), `IK_P` (phone, non-exportable in AndroidKeyStore). Nonces `Nm`, `Np`
are 16 bytes from the OS CSPRNG. **Session TTL = 3 min** (matches §5.7); ephemerals, nonces,
and SAS all die with the session regardless of outcome. **A GATT connection here is fine** —
the no-connection constraint governs only the *steady-state presence decision*; one-time,
in-person pairing may connect.

| # | Channel | Dir | Bytes on the wire |
|---|---------|-----|-------------------|
| M0 | **QR (authentic)** | M→P | `ver=1 ‖ MacId(16) ‖ gen(8) ‖ sessionId(16) ‖ PK_M(65) ‖ IK_M(65) ‖ expiry(8)` |
| P1 | BLE GATT (untrusted) | P→M | `PK_P(65) ‖ IK_P(65) ‖ deviceId(16) ‖ Cp(32)` — **phone commits its nonce first** |
| M2 | BLE GATT | M→P | `Nm(16)` — Mac reveals its nonce |
| P3 | BLE GATT | P→M | `Np(16)` — phone reveals; Mac checks `Cp` |

Commitment (SHA-256, domain-separated):
```
Cp = SHA-256( "repose-pair-v1 commit phone" ‖ PK_P ‖ IK_P ‖ deviceId ‖ Np )
```
Mac aborts if `Cp ≠ SHA-256(… ‖ Np)` after P3.

SAS both sides compute and display:
```
sas      = SHA-256( "repose-pair-v1 sas" ‖ PK_M ‖ PK_P ‖ IK_M ‖ IK_P ‖ Nm ‖ Np )
6-digit  = ( int_be(sas[0..4]) mod 1_000_000 )        // ~20 bits
```
The human compares the 6 digits on both screens. **A single mismatch aborts.** No retry with
the same session — a new session mints fresh keys/nonces.

**Why a MITM cannot inject its own key.** QR authentically fixes `PK_M, IK_M` at the real
phone, so an attacker can only tamper the phone→Mac BLE leg (`PK_P', IK_P', Np'`). To force
both humans to see the same 6 digits it must choose `Np'` to collide the SAS — but P1's
commitment forces it to commit its nonce **before** `Nm` is revealed in M2, so it cannot grind
against a known target. Success ≤ 2⁻²⁰ per session; any mismatch aborts. This is the published
Numeric-Comparison / SAS bound.

### 1.4 Key derivation + mandatory key-confirmation

After **both** humans confirm, each side computes `Z = ECDH(sk_own, PK_other)` (32-byte X,
leading zeros preserved) and binds the whole transcript:
```
ctx        = SHA-256( "repose-pair-v1 transcript" ‖ M0 ‖ PK_P ‖ IK_P ‖ deviceId ‖ Nm ‖ Np )
salt       = SHA-256( "repose-pair-v1 salt" ‖ ctx )
PRK        = HKDF-Extract-SHA-256(salt, Z)
K_confirm  = HKDF-Expand(PRK, "repose-pair-v1 confirm",       32)
K          = HKDF-Expand(PRK, "repose-pair-v1 presence-key",  32)   // the beacon HMAC key
```
Then a **mandatory** two-message key-confirmation over BLE, verified **before anything is
persisted**:
```
P→M:  HMAC-SHA256(K_confirm, "repose-pair-v1 kc phone-to-mac" ‖ ctx)
M→P:  HMAC-SHA256(K_confirm, "repose-pair-v1 kc mac-to-phone" ‖ ctx)
```
Key-confirmation upgrades the guarantee past the SAS's 2⁻²⁰: a MITM ran two separate ECDHs, so
its `Z` differs from each endpoint's and it cannot produce these MACs — the rarer SAS collision
is still caught here unless P-256 ECDH itself is broken. Failure ⇒ abort, persist nothing.

### 1.5 What each side stores

**Phone (Android):**
- `IK_P` **private key** — non-exportable in AndroidKeyStore (StrongBox when available, else
  TEE) via `SigningKeyPolicy`; used later to sign codex Responses. Never leaves hardware.
- `K` — **imported into AndroidKeyStore as a raw HMAC-SHA-256 key** (`KeyProperties`
  `HMAC_SHA256`, non-exportable) so the 256-bit secret never sits in app-readable storage; the
  rotating beacon is computed *through* Keystore, not from bytes in `SharedPreferences`.
- Paired record (app storage, non-secret): `{ MacId, deviceId, gen, IK_M(public) }`.

**Mac:**
- `IK_M` **private key** — Keychain, Secure Enclave when available, access-controlled.
- Paired record `{ deviceId, gen, IK_P(public) }` — Keychain or a root-only file. **Never
  `localStorage` (R-P7).**
- `K` — written to a **root-only `0600` file owned by the presence scanner/bridge** (e.g.
  `/var/db/repose-unlock/presence-key.<keyId>`), because the scanner runs as root and cannot
  read the login user's Keychain. `K` is the **lower-value** secret — it proves proximity only;
  a full unlock still requires the identity-key challenge/response — so a root-file placement
  is acceptable while the identity private keys stay in hardware.

`deviceId`, `MacId` = stable 16-byte opaque ids (codex `DeviceId`/`MacId`), **not** the
rotating BT address.

### 1.6 Key lifetime & rotation

- **Identity keys (`IK_M`, `IK_P`):** long-term, one per install, hardware-resident, survive
  restart. Destroyed only on "撤销设备" / "移除并还原": phone deletes the Keystore keys; Mac
  writes codex's `Revoked(gen)` tombstone (durable CAS, idempotent) so no old-generation state
  can re-initialize.
- **`pairing_generation`:** minted at first pair, **incremented on every re-pair**. A higher
  generation invalidates all older-generation state; initialization from a tombstone requires a
  strictly newer generation. Missing state fails **closed**.
- **`K`:** rotates **per pairing generation** — a fresh 256-bit key each (re)pair, from a fresh
  ECDH. Never silently re-derived. (Future, out of scope: rotate `K` in-band over the
  authenticated codex unlock channel without re-showing the SAS.)
- **SAS + session:** 3-min TTL; code, both ephemeral DH keys, and both nonces are single-use
  and discarded on confirm/abort/timeout.

---

## 2. Advertisement layout — `repose-presence-v1`

### 2.1 UUID choice: switch to a 16-bit service UUID

Legacy (BT 4.x) advertising gives two 31-byte payloads (primary + scan response); every AD
structure costs 2 bytes of overhead. A **connectable** primary packet also pays a mandatory
3-byte Flags structure that Android auto-inserts and you cannot suppress while connectable.

- **128-bit UUID (type 0x07):** 18 bytes. In a connectable primary packet (31 − 3 = 28 usable)
  that leaves **10 bytes** — Service Data 128-bit needs `18+N`, so it **does not fit at all**;
  manufacturer data would leave `N ≤ 6`, too small for even an 8-byte HMAC.
- **16-bit UUID (type 0x03):** 4 bytes → **24 bytes left** for the authenticator, right in the
  primary packet. No scan response, no active-scan round trip, no GATT.

**Decision: use `0000FFF0-0000-1000-8000-00805F9B34FB`.** Because it matches the Bluetooth Base
UUID, Android's stack automatically shortens both `addServiceUuid` and `addServiceData` to their
16-bit forms. We pick a value in the `0xFFF0–0xFFFF` member/unassigned space; a 16-bit collision
with some unrelated device costs the Mac only one wasted HMAC verification, since the token is
verified cryptographically.

The old 128-bit UUID `7265706F-7365-0001-…` is **retired** from the presence path (it may
remain only for the pairing-window connectable set / GATT server).

### 2.2 The rotating authenticator (service-data payload `N`)

Carried in **Service Data – 16-bit** (AD type 0x16) in the **primary** packet:

```
byte 0      : format/version                     0x01
byte 1      : keyId  (pairing slot / K selector; also disambiguates 16-bit UUID collisions)
bytes 2..9  : tag = truncated HMAC-SHA256(K, msg)   // 8 bytes  (16 for extra margin)
```

where
```
counter = floor(unix_time_seconds / WINDOW)          // NOT transmitted
msg     = "repose-presence-v1 beacon" ‖ keyId(1) ‖ counter(8, big-endian)
tag     = HMAC-SHA256(K, msg) [0 .. TAG_LEN)         // truncation of an HMAC is safe
```

- **`counter` is never transmitted.** The Mac recomputes it from its own clock. This costs zero
  adv bytes and removes a spoofable field.
- **`WINDOW = 30 s`** (configurable 30–60 s). Tolerant of clock skew and RPA rotation (below).
- **`TAG_LEN = 8`** → 64-bit forgery resistance per window. Use 16 for margin; both fit.

**Byte budget in the primary packet (connectable, 28 usable):**

| AD structure | Type | Bytes |
|---|---|---|
| Flags (auto, connectable only) | 0x01 | 3 |
| Complete list of 16-bit Service UUIDs → `FFF0` | 0x03 | 4 |
| Service Data – 16-bit (`FFF0` + 10-byte payload) | 0x16 | 4 + 10 |

Total **21 / 31** bytes with an 8-byte tag (**29 / 31** with a 16-byte tag). Both fit with room
to spare. **Recommendation:** make the steady-state presence beacon **non-connectable**
(`setConnectable(false)`) — reclaims the 3-byte Flags tax, lowers power, and stops random
centrals connecting; advertise a separate connectable set + the GATT server only during the
3-min pairing window.

### 2.3 Why HMAC and not ECDSA here

An ECDSA-over-the-counter beacon (Option B) would reuse codex's signer verbatim and need no
shared `K` — but a P-256 signature is **64 bytes** and does not fit beside any UUID in a 31-byte
PDU without a scan response (which forces an active-scan round trip, taxing the p95 discovery
budget the whole design is protecting). **Truncating an ECDSA signature is insecure.** HMAC is
the only construction that both fits the byte budget and is safe to truncate. Cost: one shared
symmetric `K` (minted in §1.4), which the codex prior art does not currently have — hence
`repose-pair-v1` produces it.

### 2.4 How RPA rotation is handled

Android rotates its RPA ~every 15 min; the Mac's `peripheral.identifier` changes with it, and
the phone re-emits `startAdvertising` on each rotation. **None of this touches identity** —
identity is `K` (selected by `keyId`), which is stable across RPA rotations. The Mac verifies
the tag against `K` regardless of which address it arrived from. The `±1` window tolerance
(§3.2) also absorbs the brief advertising gap around a rotation. The phone must **rebuild the
service-data payload and call `startAdvertising` at least once per WINDOW** so the tag stays
current (a `Handler`/`AlarmManager` tick every `WINDOW` seconds).

---

## 3. Mac verification algorithm

### 3.1 Read the advertisement without connecting

In `centralManager(_:didDiscover:advertisementData:rssi:)`:

1. `scanForPeripherals(withServices: [CBUUID(string: "FFF0")])` — kernel-level filter on the
   UUID-list AD (service data alone does not satisfy `withServices:`, so we keep the UUID-list
   AD in the packet). Keep `CBCentralManagerScanOptionAllowDuplicatesKey: true` so each rotated
   token yields a fresh callback.
2. Read `advertisementData[CBAdvertisementDataServiceDataKey]` → `[CBUUID: Data]`; take the
   `Data` for `CBUUID(string: "FFF0")`. That value **is** the `N`-byte payload — no `connect()`,
   no service/characteristic discovery. CoreBluetooth coalesces the primary packet into the
   first `didDiscover`, so the token arrives on the first callback with no round trip.
3. Parse: byte 0 = version (must be `0x01`), byte 1 = `keyId`, bytes 2.. = `tag`.

The old scanner path (`connect` → discover → read the hello characteristic) is **deleted** from
the presence decision.

### 3.2 Verify over current ± adjacent windows

```
now      = current unix time (seconds)
c0       = floor(now / WINDOW)
for c in [c0-1, c0, c0+1]:                       // ±1 window: skew + rotation tolerance
    msg      = "repose-presence-v1 beacon" ‖ keyId(1) ‖ c(8, big-endian)
    expected = HMAC-SHA256(K[keyId], msg) [0 .. TAG_LEN)
    if constant_time_equals(expected, tag):
        return VALID
return INVALID
```

- `K[keyId]` is loaded from the root-only presence-key file for the paired generation. Unknown
  `keyId` → INVALID (no such pairing on this Mac).
- **Constant-time compare** on the truncated tag.
- Checking `{c0-1, c0, c0+1}` tolerates ±1 window of clock skew between phone and Mac and the
  advertising gap around an RPA rotation. Do **not** widen beyond ±1 without cause — every extra
  window is extra replay surface (§4).

### 3.3 Gate the permit write on VALID authenticator **and** RSSI

The bridge (`permit-bridge.sh` + verifier) writes the root permit **only when both** hold, under
the existing hysteresis:

1. **Authenticator VALID** — a fresh, in-window tag verified against a known `K` (§3.2).
2. **RSSI ≥ threshold** — the calibrated proximity gate, unchanged from the B-spike.

Either condition false ⇒ **no permit** (the bridge writes nothing / lets the existing permit
expire). This is the crux of closing item #1: presence now means "a device holding the paired
`K` minted a currently-valid beacon **and** is physically close," not "some Android is near."
RSSI is a *proximity* gate layered on top of the *authentication* gate; neither substitutes for
the other. The daemon still consumes the permit per unlock attempt (its existing behavior),
which bounds replay (§4).

---

## 4. Residual exposure — the honest limit

### 4.1 Replay within the window

A passive sniffer can capture a valid advertisement and **rebroadcast it verbatim within the
same WINDOW** — the tag is still valid until `counter` rolls over. This is inherent to any
connectionless, advertisement-only authenticator: with no back-channel, the verifier cannot
issue a fresh challenge, so the token cannot be bound to *this* verification instant. We do not
pretend to solve it. What **bounds** it:

- **Short WINDOW (30 s).** The replay validity is ≤ 2·WINDOW (because of ±1 tolerance) ≈ 60 s,
  not indefinite.
- **RSSI gate.** The replayed beacon must also arrive strong enough — the attacker's transmitter
  must be physically close to the Mac, not merely within sniffing range of the phone.
- **Daemon per-attempt consume.** The permit is consumed per unlock attempt (codex
  `replay.rs` monotonic-counter discipline on the *unlock* path), so a captured beacon does not
  grant unbounded unlocks — it grants presence for one short window, still subject to the full
  identity-key unlock challenge/response for an actual unlock.

Note the layering: a valid beacon proves **proximity of the paired device**, which gates the
permit; it is **not** itself an unlock authorization. A full unlock still runs codex
`repose-unlock-v1` (P-256 challenge/response with Mac-chosen nonce + monotonic replay counter),
which is *not* replayable. So the replay window buys an attacker "the Mac believes my phone is
nearby for ≤ 60 s," not "the Mac is unlocked."

### 4.2 Relay (§9.3)

A relay attack — two colluding radios tunnelling the live advertisement from the real phone
(far away) to a box next to the Mac in real time — defeats the WINDOW (the token is genuinely
current) **and** can defeat the RSSI gate (the near-Mac radio transmits at close range). This is
the exposure §9.3 already acknowledges as **not fully solvable in software.** Full anti-relay
needs either a GATT challenge (forbidden by the 3 s unlock budget — the whole reason the
authenticator rides in the advertisement) or **hardware distance ranging** (UWB / 802.11mc
FTM / BLE Channel Sounding), which the current hardware does not provide. We **document** this;
we do not claim to close it. It is the honest limit of connectionless presence.

### 4.3 Relay of pairing

Out of scope: pairing is one-time and in-person, with the human comparing the SAS on both
screens. A relayed pairing session is caught by the SAS mismatch (§1.3) — the human sees two
different codes and stops ("不一致就别继续").

---

## 5. Files to change

**Constraint honored:** touch **only** `tools/ble-spike/android/*`, `tools/ble-spike/mac/*`
(scanner/bridge + verification), a **new pairing/verify module**, and `docs/plans/` +
`docs/validation/`. Do **not** touch `native/macos/minimal-auth-plugin/`,
`native/macos/permit-daemon/`, or `plugin.c` (another workflow owns those). Do **not** git
commit.

### Android (`tools/ble-spike/android/app/src/main/kotlin/ai/repose/blespike/`)
- **`SpikeContract.kt`** — add `PRESENCE_SERVICE_UUID = 0000FFF0-0000-1000-8000-00805F9B34FB`,
  `PRESENCE_VERSION = 0x01`, `WINDOW_SECONDS = 30`, `TAG_LEN = 8`, the beacon domain-separation
  label, and the `repose-pair-v1` labels. Keep the legacy 128-bit UUID only for the
  pairing-window connectable/GATT set.
- **`BleSpikeService.kt`** — replace the static-UUID advertiser with the rotating presence
  beacon: build the Service-Data-16 payload (`version‖keyId‖tag`), recompute the tag through
  the Keystore HMAC key each WINDOW, `startAdvertising` non-connectable in steady state.
  Advertise the connectable set + GATT server only during the pairing window.
- **NEW `PresenceBeacon.kt`** — computes `HMAC-SHA256(K, "…beacon"‖keyId‖counter)` via the
  AndroidKeyStore HMAC key; owns the per-WINDOW rebuild tick.
- **NEW `ReposePairing.kt`** — the `repose-pair-v1` responder: parse QR (M0), generate ephemeral
  P-256 + `IK_P` (reuse `AndroidKeyStoreSigner`/`SigningKeyPolicy`), the commit/reveal exchange
  (P1/M2/P3), SAS compute, ECDH + HKDF → `K_confirm`, `K`, key-confirmation, and import of `K`
  as a non-exportable Keystore HMAC key.
- **`PairingScreen.kt` / `AppStore.kt`** — wire the real SAS 6-digit display + confirm gesture
  and the paired-flag/record in place of the placeholder pairing code.

### Mac (`tools/ble-spike/mac/`)
- **`rssi-scan.swift`** — delete the connect-and-read path; in `didDiscover` read
  `CBAdvertisementDataServiceDataKey` for `FFF0`, parse `version‖keyId‖tag`, and emit
  `(rssi, keyId, tag, timestamp)`. Filter `withServices: [CBUUID("FFF0")]`.
- **NEW `presence-verify.swift`** (or a Swift module compiled with `rssi-scan`) — the §3.2
  verify: recompute `HMAC-SHA256(K[keyId], …)` over `{c0-1, c0, c0+1}`, constant-time compare
  the truncated tag; a `--self-test` mode that verifies the math against a committed known
  `K` + timestamp + expected-tag vector (mirror codex `protocol/fixtures/v1/` discipline).
- **`permit-bridge.sh`** — gate the permit write on **VALID authenticator AND RSSI**
  (§3.3), preserving the existing hysteresis.
- **NEW `presence-key` loader** — read `K` from the root-only `0600`
  `/var/db/repose-unlock/presence-key.<keyId>` file.

### Docs
- **`docs/plans/2026-09-10-authenticated-presence-design.md`** — this file.
- **`docs/validation/2026-09-10-authenticated-presence.md`** (tomorrow) — record the Android
  `./gradlew assembleDebug` build, the `swiftc` verifier build + `--self-test` pass, and the
  **real-BLE** result (phone advertising the rotating token, Mac reading + HMAC-verifying it
  live). The real-BLE leg needs the physical realme RMX3888 — **noted for tomorrow.**

### What we can verify in simulation (today)
- Android `./gradlew assembleDebug` builds.
- The Mac verifier compiles (`swiftc`) and its `--self-test` reproduces a known
  `K` + time-window → expected-tag vector (offline, no radio).
- Real BLE (live advertise + live read/verify) requires the phone — deferred to tomorrow.

---

## 6. Threat model

**A passive sniffer** (`nRF Connect`, an SDR) sees the 16-bit UUID `FFF0`, the `keyId`, and the
truncated HMAC tag. It learns nothing about `K` (HMAC is a PRF; 8–16 bytes of output over a
predictable counter does not reveal the 256-bit key), cannot mint a tag for a *future* window,
and cannot forge one for the current window better than 2⁻⁶⁴ (8-byte tag). It **can** replay a
captured tag for ≤ ~60 s (§4.1) — bounded by the short window, the RSSI gate, and per-attempt
permit consume, and in any case buying only "presence," never an unlock (which needs the
non-replayable identity challenge/response).

**An attacker cloning the UUID** — broadcasting `FFF0` (or the old public 128-bit UUID) with a
made-up or absent tag — is now **rejected**: the Mac requires a tag that verifies against a `K`
it holds. This is precisely the hole from item #1 that this design closes. A random/garbage tag
fails the HMAC check; no permit is written.

**An attacker with a captured advertisement** can replay it within its window (§4.1); the same
bounds apply. Outside the window the tag is dead. It cannot roll the counter forward (that needs
`K`).

**A MITM at pairing time** cannot substitute its own key: QR authenticates Mac→phone, and the
commit-before-reveal SAS + mandatory key-confirmation authenticate phone→Mac; a mismatch aborts
(≤ 2⁻²⁰ SAS bypass, then caught by key-confirmation). The human comparing the two 6-digit codes
is the trust anchor.

**A real-time relay** (§4.2) is the one adversary this design does **not** stop: colluding
radios tunnel the genuine, current advertisement next to the Mac, defeating both the WINDOW and
the RSSI gate. Software cannot close this without a challenge (budget-forbidden) or hardware
ranging (unavailable). This is the acknowledged residual, consistent with §9.3 — and the reason
a valid beacon is only a *proximity gate*, with the actual unlock still gated by the
non-relayable identity challenge/response.

**What no attacker gets from any of the above:** `K`, `IK_P`'s private key (hardware-bound,
non-exportable), or the ability to unlock without also passing the codex `repose-unlock-v1`
identity challenge/response.
