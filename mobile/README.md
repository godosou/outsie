# Repose Unlock mobile shell

This directory contains the shared Android/iOS Flutter companion shell built
with Flutter 3.38.9. The initial Android validation target is a realme GT5 Pro
running Android 16, while all domain models, controllers, and screens remain
platform-neutral.

Task 9 deliberately ships only the Flutter application boundary. The default
`UnavailableNativeGateway` has no Bluetooth, secure-key, or background-service
implementation. It rejects native operations and keeps pairing, calibration,
and device mutation fail-closed until reviewed platform adapters are supplied.

This shell is not distributable. Task 9 uses explicit build-time hard gates:

- Android tasks containing `Release`, including `assembleRelease`, fail with a
  production-signing-required error. Debug and test builds remain available.
- Xcode Release/Archive compilation fails at a checked Swift `#error`. Debug
  compilation remains available.
- Native phone-key adapters and their security review are deferred to the next
  implementation task.

Offline checks:

```sh
flutter test --no-pub
flutter analyze --no-pub
```
