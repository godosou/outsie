# Issue 0002 · Authenticated presence (only the paired phone can unlock)

- **Status:** open · **beacon half built and wired (2026-09-10); not yet demonstrated on the
  radio.** The pairing half is still not built — `K` currently arrives over a labelled USB
  development channel, so this issue does not close. See
  [验证记录](../validation/2026-09-10-authenticated-presence.md) for what is and is not proven.
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
  **Done** — the shipped verifier is `tools/ble-spike/mac/presence-verify.swift`, built and
  self-tested against OpenSSL-generated vectors.

**Withdrawn:** "make the bridge auth-gate opt-in (`REPOSE_REQUIRE_AUTH`) until pairing+beacon are
real, so the working file-permit flow isn't broken in the meantime." Two things were wrong with
it. The file-permit flow is a *different* presence source — the VM harness writes the permit over
ssh and never goes through the bridge — so the gate breaks nothing. And a safeguard with an
opt-out defaults to whichever state someone last forgot to set; on this project, that is exactly
how three artifacts came to describe protections the code did not have. The gate is
unconditional, and `permit-bridge-test.sh` asserts that no bypass variable exists.

## Residual (honest, by design)

A real-time **relay** tunnels a genuine current beacon next to the Mac, defeating the short WINDOW
and the RSSI gate. Software can't close it without a GATT challenge (blows the 1.5s permit budget)
or hardware ranging (unavailable).

**Correction.** This used to end "an actual unlock still requires the non-replayable identity
challenge/response." No such challenge/response exists in this pipeline. A verified beacon *is*
what lets an empty password through, so a successful relay — or a replay inside its window —
unlocks the Mac. WINDOW and RSSI are the only bounds; there is nothing behind them.

## Acceptance

Only the paired phone's live beacon (VALID over the air) + close RSSI causes an unlock, verified on
the real phone + Mac; MITM at pairing is defeated (two-device + adversarial-third test); an
independent crypto review has signed off. Until then, ship with authentication OFF is NOT acceptable
for distribution (it's the item this issue exists to close).

## Needs

Real hardware (the realme RMX3888 + a Mac) and an **independent crypto review** before trust.
