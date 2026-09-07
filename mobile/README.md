# Repose Unlock mobile shell

This directory contains the shared Android/iOS Flutter companion shell built
with Flutter 3.38.9. The initial Android validation target is a realme GT5 Pro
running Android 16, while all domain models, controllers, and screens remain
platform-neutral.

Task 9 established the Flutter application boundary. Task 10 connects Android
debug builds to a domain-only Pigeon adapter and an API 36 companion-presence
runtime; non-Android builds still use `UnavailableNativeGateway`. Task 11 adds
offline-tested v1 phone-response and role-neutral GATT transport primitives.
Neither prototype BLE role is selected for production, and no raw GATT or
signing operation is exposed to Dart. Pairing, calibration, device mutation,
and end-to-end unlock remain fail-closed until physical-device and macOS gates
are completed.

This shell is not distributable. Task 9 uses explicit build-time hard gates:

- Android tasks containing `Release`, including `assembleRelease`, fail with a
  production-signing-required error. Debug and test builds remain available.
- Xcode Release/Archive compilation fails at a checked Swift `#error`. Debug
  compilation remains available.
- Hardware-backed signing, presence plumbing, response persistence, and both
  BLE role adapters are compile/test artifacts only; they have not run on a
  physical device. `BleRoleSelector.productionRole()` remains `Disabled`.
- Native code shipped inside the same signed APK and Android UID is part of the
  trusted computing base: same-UID code can address the app-scoped Keystore
  alias directly. The Java carrier boundary closes public AAR and Pigeon
  construction paths; resisting a compromised same-UID dependency would
  require a separately isolated UID/service and is not claimed here.

Offline checks:

```sh
flutter test --no-pub
flutter analyze --no-pub
```
