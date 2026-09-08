# Repose Unlock mobile shell

This directory contains the shared Android/iOS Flutter companion shell built
with Flutter 3.38.9. The initial Android validation target is a realme GT5 Pro
running Android 16, while the domain models, controllers, and screens remain
platform-neutral.

Android Debug now has a bounded pairing path: the user first approves the
Android Companion Device Manager (CDM) chooser, then scans the one-time QR
shown by the Mac, and a native Kotlin GATT central connects only to that
association. Dart passes the canonical pairing URI through Pigeon but receives
no raw GATT frame or
signing primitive. The matching macOS Debug app is the CoreBluetooth peripheral.
This path is for pairing interoperability only; it is not the production
presence/unlock transport.

The current Debug APK compiles this path and has been installed on an API 35
emulator. The emulator completed permission UI and CDM association setup, but
cannot use the Mac host's Bluetooth radio for GATT, so no `ACCEPTED` result was
observed. The realme phone is still ADB `unauthorized` and did not receive this
APK. Neither unlock BLE role is selected for production,
`BleRoleSelector.productionRole()` remains `Disabled`, and macOS Authorization
Services remain untouched. System-level automatic unlock remains fail-closed.

## Debug pairing contract

- The Mac QR is a canonical `repose://pair/v1/<base64url>` URI with a 120-second
  lifetime. Flutter rejects an empty/oversized value, whitespace, query,
  fragment, padding, or extra path before Kotlin performs strict byte decoding.
- The Mac advertises as `Repose Mac` with service
  `A53E0001-7A6B-4D59-9F2E-5245504F5345`. Control and status characteristics are
  `A53E0002-7A6B-4D59-9F2E-5245504F5345` and
  `A53E0003-7A6B-4D59-9F2E-5245504F5345`.
- Android writes the session-bound Debug control value and accepts only an
  exact `WAITING` or `ACCEPTED` status for that session. Confirmation before
  `ACCEPTED` remains retryable and does not install a key.
- API 31–35 foreground association/GATT compatibility is compiled only in the
  Debug source set. Release and Profile keep the API 36 production-runtime gate
  and instantiate the fail-closed host API rather than the Debug pairing host.
- A development-Mac `bluetoothd` observation saw the exact local name/service
  UUID start advertising and stop after the 120-second expiry. No Android
  discovery, connection, characteristic write, or notification was observed
  in that result.

This shell is not distributable. It uses explicit build-time hard gates:

- Android tasks containing `Release`, including `assembleRelease`, fail with a
  production-signing-required error. Debug and test builds remain available.
- Xcode Release/Archive compilation fails at a checked Swift `#error`. Debug
  compilation remains available.
- Hardware-backed signing and response persistence have limited historical
  test-UID instrumentation evidence on the GT5 Pro. The current camera/CDM/GATT
  APK has only been installed on the API 35 emulator. Presence and unlock BLE
  adapters have not passed real-radio/background validation.
- Native code shipped inside the same signed APK and Android UID is part of the
  trusted computing base: same-UID code can address the app-scoped Keystore
  alias directly. The Java carrier boundary closes public AAR and Pigeon
  construction paths; resisting a compromised same-UID dependency would
  require a separately isolated UID/service and is not claimed here.

Offline checks:

```sh
flutter pub get
flutter test
flutter analyze
flutter build apk --debug
```

The current evidence is 91/91 Flutter tests, 0 analyzer issues, 52/52 React
tests, a passing full Rust workspace, and native JVM results of 144/144 Debug,
120/120 Release, and 120/120 Profile. The Debug APK SHA-256 is
`118a13cbf5f8fb56f6e927c67d4ec7453b7407e92933ed2e830a321dad374abc`.
The iOS Podfile passes `ruby -c`, which is only a static configuration check;
no iOS build or device run is claimed.

## Brand and permission guidance

The mobile interface shares the Mac app's warm daylight/forest palettes,
Manrope typography and four-petal mark. It follows the system language (English
or Chinese) and appearance. Settings explains nearby-device access,
notifications, battery optimization and manufacturer-specific background
restrictions, and rechecks the OS when returning from system Settings.

The camera flow explains its purpose before opening the OS prompt, requests
camera access only after a user gesture, handles denial in-app, and links to
app settings after permanent denial. Manual pairing-code paste is not exposed
as a user entry point. Android declares `CAMERA`; iOS has
`NSCameraUsageDescription`, and its Podfile enables `PERMISSION_CAMERA=1`.

On `emulator-5554` (API 35), the observed foreground sequence was nearby-device
explanation → Android permission prompt → `Allowed`, followed by CDM association
`id=1` and the `READY TO PAIR` home state. `Scan Mac QR code` was the only
pairing entry. Its camera explanation and OS prompt opened the real
`mobile_scanner` page, with no text input or copy/paste control. This proves the
foreground UX only; the emulator provided no Mac GATT path.

The `ai.repose/system_settings` channel is separate from the security/Pigeon
boundary. It can read settings, open this app's system details, and request only
permissions declared by the installed feature after a user gesture. The
Android native module now declares `BLUETOOTH_SCAN`, `BLUETOOTH_CONNECT`, and
companion-presence access for the CDM/GATT Debug path. Notifications remain
undeclared and are shown as not requested. No setting or permission turns on
the unfinished production unlock transport.

Generate launcher assets with `node scripts/generate-mobile-brand.mjs` from the
repository root. The original Google Fonts Manrope variable font and SIL OFL
license live in `assets/fonts`; the bundled 400/500/600/700 instances avoid
Android's fallback to the variable file's thin default. Reproduce an instance:

```sh
uv tool run --from fonttools fonttools varLib.instancer \
  mobile/assets/fonts/Manrope.ttf wght=400 \
  -o mobile/assets/fonts/Manrope-Regular.ttf
```

See [UX design](../docs/plans/2026-09-08-mobile-brand-permission-ux.md) and
[validation](../docs/validation/mobile-ux-results.md) for the scope and evidence.
