# Issue 0002 · Authenticated presence (only the paired phone can unlock)

- **Status:** open · design done, prototype exists (unsafe, not wired), needs production implementation
- **Priority:** HIGH (security) — a distribution gate
- **Area:** Android (pairing + beacon) + Mac (verifier + bridge gate) + pairing key store
- **Filed:** 2026-09-10

## Problem

Today ANY Android broadcasting the public service UUID `7265706F-7365-0001-8000-00805F9B34FB`
counts as "present" and can unlock the paired Mac — there is **no authentication**. Only the
*paired* phone must count.

## Where things stand

- **Design: done + authoritative** — `docs/plans/2026-09-10-authenticated-presence-design.md`.
  SAS-guarded ECDH pairing (`repose-pair-v1`) → HKDF → a non-exportable HMAC presence key `K`;
  the phone broadcasts `version‖keyId‖truncated-HMAC-SHA256(K, floor(unix_time/WINDOW))` (8 bytes)
  in the advertisement service-data under a 16-bit UUID; the Mac verifies over current ±1 windows
  (WINDOW=30s) with no GATT connection, and the bridge writes the permit only on VALID + RSSI.
- **Prototype: exists but UNSAFE and unwired** — `docs/prototypes/authenticated-presence/` (moved
  out of the build). The `crypto-paired-presence` workflow's own adversarial review found must-fix
  defects; it is not in the app.

## Must-fix before production (from the adversarial crypto review)

- **D1 (HIGH):** the MITM-resistant pairing is not actually wired — `ReposePairing.preparePairing()`
  plays both the Mac and phone sides in one process (SAS always matches) and was called from the
  production `PairingScreen`, so it would "pair" unconditionally. Wire the real QR(M0)/GATT(P1/M2/P3/kc)
  transport between two devices; gate any in-process helper behind a dev flag; then test MITM
  resistance with two devices + an adversarial third that substitutes keys on the BLE leg (confirm
  the SAS diverges and key-confirmation aborts).
- **D2 (MEDIUM):** enforce the 3-minute pairing/QR TTL (parsed but never checked) — stamp session
  creation, reject expired QRs/sessions.
- **D3 (MEDIUM):** the Mac presence-key file must be verified `root:wheel 0600` before trust (R-P7),
  not read from any path.
- Fix the Swift compile errors (`PresenceVerify` scope / `@main` in a top-level file).
- Make the bridge auth-gate opt-in (`REPOSE_REQUIRE_AUTH`) until pairing+beacon are real, so the
  working file-permit flow isn't broken in the meantime.

## Residual (honest, by design)

A real-time **relay** tunnels a genuine current beacon next to the Mac, defeating the short WINDOW
and the RSSI gate. Software can't close it without a GATT challenge (blows the 3s budget) or hardware
ranging (unavailable). The beacon is therefore a *proximity gate*, not proof of the phone — an actual
unlock still requires the non-replayable identity challenge/response, and within-window replay is
bounded by WINDOW + RSSI + the daemon's per-attempt consume (issue #? / gap #3, done).

## Acceptance

Only the paired phone's live beacon (VALID over the air) + close RSSI causes an unlock, verified on
the real phone + Mac; MITM at pairing is defeated (two-device + adversarial-third test); an
independent crypto review has signed off. Until then, ship with authentication OFF is NOT acceptable
for distribution (it's the item this issue exists to close).

## Needs

Real hardware (the realme RMX3888 + a Mac) and an **independent crypto review** before trust.
