# iOS background unlock validation

Date: 2026-09-08

Status: **BLOCKED / NOT RUN / GATE CLOSED**

## Host prerequisite evidence

The active developer toolchain cannot provide the required iOS build environment:

- macOS: `14.6.1` (`23G93`)
- active developer directory: `/Library/Developer/CommandLineTools`
- `xcodebuild -version`: unavailable because the selected directory is the
  Command Line Tools installation, not a full Xcode toolchain
- `xcrun --sdk iphoneos --show-sdk-version`: failed because the `iphoneos`
  SDK cannot be located
- available Swift compiler: Swift `6.0.3`, targeting macOS only

Task 12 requires selecting a supported full Xcode installation containing the iOS 26
SDK before any production Swift runtime is written. Whether another Xcode bundle exists outside
the selected developer directory was not established as qualification evidence. Creating an unbuildable
AccessorySetupKit/Core Bluetooth implementation on this host would not satisfy
the project's test-first gate, so no iOS production files were added.

## Required validation matrix

All rows remain **NOT RUN**:

- simulator debug build and unit tests
- foreground pairing through AccessorySetupKit
- background and suspended Core Bluetooth handling
- system termination and state restoration
- user force-quit fallback
- Bluetooth-off fallback
- reboot before first unlock
- reboot after first unlock
- Secure Enclave/Keychain accessibility and non-exportability
- end-to-end Mac challenge latency and password fallback

## Release consequence

The existing iOS Release/Archive compile-time gate remains enabled. iOS phone
unlock must not be advertised or distributed until this document is replaced
with evidence from the supported Xcode host and a physical iPhone matrix.
