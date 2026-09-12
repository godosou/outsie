# Issue 0001 · Per-OEM background keep-alive guidance (Android Phone Key)

- **Status:** open · deferred (do not implement yet)
- **Priority:** low — reliability enhancement, not a blocker for the feature working
- **Area:** Android app · the battery-exemption row on the home screen (`HomeScreen.kt`: `isBatteryExempt` / `requestBatteryExempt`); the former standalone "保持后台 / Keep-alive" screen (screen 3 of the Phone Key interaction spec) was folded into it and removed
- **Filed:** 2026-09-09
- **Tracking note:** this repo has no git remote yet; filed as a local issue doc. Migrate to a real
  issue tracker (GitHub Issues) once the repo is pushed.

## Context

For the phone to be recognized by the Mac at lock time, the Android Phone Key app must keep its BLE
advertiser alive in the background. Android OEMs bury "disable battery optimization", "autostart /
allow background activity", and "lock the app in recents" in different places, and several run
aggressive background killers that freeze the app after a while — which makes the Mac fail to see the
phone and the user falls back to typing a password.

**v1 ships a generic path only:** `ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS` + the app-details
settings intent, with copy explaining what happens if the user skips it. That is enough for the
feature to work, but reliability varies a lot by OEM.

## Requirement

Detect the device manufacturer/model at runtime and show tailored, step-by-step keep-alive guidance
for the popular OEMs, with working deep-links where the OEM exposes them and a clear manual path
where it does not. Target set (by market share across the app's regions):

- **Xiaomi / Redmi / POCO** (MIUI / HyperOS) — autostart + battery saver allowlist + "lock in recents"
- **OPPO / realme / OnePlus** (ColorOS / OxygenOS) — autostart + battery optimization + background freeze
- **vivo / iQOO** (Funtouch OS / OriginOS) — high background power consumption + autostart
- **Samsung** (One UI) — "never sleeping apps" + remove from sleeping apps + unrestricted battery
- **Google Pixel** (stock Android) — battery-unrestricted (mostly the generic path already works)
- **Huawei / Honor** (EMUI / MagicOS) — manage manually / launch + secondary launch + run in background
- **Generic fallback** — the current generic battery-optimization + app-details intents, for any
  unlisted model.

Each entry provides: the correct localized copy naming the OEM's actual menu labels, and either a
direct deep-link intent (e.g. an OEM autostart activity) or an explicit "Settings → … → …" path when
no deep-link exists. Prefer official intents; guard OEM-specific `Intent`/`ComponentName` launches
in try/catch and fall back to the generic path if the activity is absent.

## Acceptance criteria

- On each listed OEM family, the keep-alive screen shows correct guidance and a working action
  (deep-link opens the right screen, or a manual path is shown when no deep-link exists).
- Unlisted models get the generic fallback and never crash on a missing OEM activity.
- Verified on at least the top OEMs by the app's user base; the rest covered by the fallback.

## Out of scope / why deferred now

The feature works with the generic path; the per-OEM matrix is a polish/reliability item. Deferring
until the core Phone Key flow (pairing → guarding → unlock) is proven end-to-end on real devices.

## References

- Interaction spec: `docs/plans/2026-09-09-unlock-interaction-design.md` (F12 手机端被停 / R5)
- A3 plan (Android side): `docs/plans/2026-09-09-a3-app-integration.md`
- Observed during B1: the ColorOS (realme) battery-whitelist requirement — see
  `docs/validation/2026-09-09-b2-real-ble.md`.
