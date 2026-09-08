# realme GT5 Pro / Android 16 BLE validation

Date: 2026-09-08

Status: **PARTIAL DEVICE VALIDATION — GATE CLOSED**

Production BLE role: **Disabled**

This record intentionally separates automated transport checks from physical-device evidence. A connected device independently reported model `RMX3888`, Android 16 / API 36, and build `RMX3888_16.0.10.500(CN01)`. The Debug app installed and launched successfully, and its UI remained fail-closed. The test-only instrumentation package passed 5/5 methods. No matching macOS BLE peer was available, so neither prototype role is selected and the three-second unlock target remains **UNASSESSED**.

## Target reported by operator

| Field | Reported value | Independent evidence |
|---|---|---|
| Device | realme GT5 Pro | `ro.product.manufacturer=realme`; `ro.product.model=RMX3888` |
| OS | Android 16 / realme UI 7.0 | Android 16; build `RMX3888_16.0.10.500(CN01)` |
| API level | API 36 | `ro.build.version.sdk=36` |
| BLE / Companion features | Required for the planned prototype | Package manager advertises Bluetooth, BLE, Companion Device Setup, hardware Keystore and StrongBox Keystore features |

## Automatic evidence

The native module has deterministic JVM coverage for the unauthenticated outer GATT envelope and role-neutral transport reducer. The checks cover exact v1 Challenge (249 bytes) and Response (390 bytes) reassembly, every two-fragment split, malformed/empty/oversized/truncated/duplicate/out-of-order fragments, callback identity tokens, reconnection, Bluetooth-off reset, process-runtime replacement, stale callbacks/timers, and bounded retry/cooldown without sleeping.

The native-only responder additionally covers trusted paired-Mac challenge authentication, Rust fixture-compatible ECDH/HKDF/AES-GCM/low-S response bytes, exact retransmission, unsigned counter limits, atomic pairing/revocation/cache CAS, corrupted durable records, caller-buffer mutation, bounded cross-process ephemeral-key leases, and crash-leftover alias sweeping. Its production composition requires a non-exportable TEE/StrongBox ECDH key and otherwise fails closed. The final combined run passed 94/94 JVM tests (including 26 transport and 24 responder tests), lint, the app Debug APK, the plugin Debug AAR, and app/plugin instrumentation APK compilation.

On the RMX3888, 5/5 instrumentation methods across three classes passed: two `AndroidKeyStoreSignerTest` methods, two `AndroidEphemeralAgreementTest` methods, and one `NoBackupResponderStoreTest` method. They cover non-exportable hardware P-256 signing and trusted-marker failure closure, independent ephemeral agreement slots and crash-leftover cleanup, and a shared durable CAS domain in the no-backup directory. The passing assertion only establishes that `KeyInfo.securityLevel` was in the accepted TEE/StrongBox set; the run did not record which specific level the provider returned.

The instrumentation run demonstrates Keystore and SQLite behavior only under the test package UID. It does not demonstrate the production app UID, Companion Presence, GATT/radio behavior, background wake-up, RSSI ownership, OEM behavior, latency, or battery use. A successful Debug install/launch and fail-closed UI do not exercise those paths. JCA provider-internal key copies also cannot be proven immediately erased through public APIs; explicitly owned secret arrays are wiped and provider objects are kept narrowly scoped. Both role adapters are prototypes behind `BleRoleSelector.productionRole() == Disabled`.

## Physical-device matrix

| Scenario | Mac central / phone peripheral | Mac peripheral / phone central |
|---|---|---|
| Manufacturer, model, OS and API queried | PASS — role-neutral RMX3888 / Android 16 / API 36 / recorded build | PASS — same role-neutral evidence |
| Bluetooth, BLE, Companion and Keystore features queried | PASS — advertised by package manager; transport behavior not exercised | PASS — same role-neutral evidence |
| Debug APK install, launch and fail-closed UI | PASS — role-neutral shell validation | PASS — same role-neutral evidence |
| Test-UID Keystore / ephemeral agreement / SQLite CAS | PASS — 5/5 instrumentation methods | PASS — same role-neutral evidence |
| Screen off | NOT RUN | NOT RUN |
| Flutter UI closed | NOT RUN | NOT RUN |
| Process killed with `am kill` | NOT RUN | NOT RUN |
| Force-stop; password fallback expected | NOT RUN | NOT RUN |
| Doze | NOT RUN | NOT RUN |
| Bluetooth off/on | NOT RUN | NOT RUN |
| Reboot before first unlock | NOT RUN | NOT RUN |
| Reboot after first unlock | NOT RUN | NOT RUN |
| Calibration in pocket/bag | NOT RUN | NOT RUN |
| 30 leave/return cycles | NOT RUN | NOT RUN |

## Required measurements before role selection

| Measurement | Mac central / phone peripheral | Mac peripheral / phone central |
|---|---|---|
| RSSI owner and sampling continuity | NOT MEASURED | NOT MEASURED |
| Background wake latency | NOT MEASURED | NOT MEASURED |
| Challenge latency p50 / p95 | NOT MEASURED | NOT MEASURED |
| Three-second target | UNASSESSED | UNASSESSED |
| Battery impact | NOT MEASURED | NOT MEASURED |
| False-near events | NOT MEASURED | NOT MEASURED |
| Required OEM settings | NOT DETERMINED | NOT DETERMINED |

## Gate

Keep production role selection disabled until the complete matrix is run against the reported GT5 Pro and a compatible Mac peer, the raw observations are recorded, and one role meets both stable distance sampling and unattended challenge wake-up requirements. Automatic unit tests alone cannot open this gate.
