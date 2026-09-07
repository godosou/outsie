# realme GT5 Pro / Android 16 BLE validation

Date: 2026-09-08

Status: **NOT RUN — GATE CLOSED**

Production BLE role: **Disabled**

This record intentionally separates automated transport checks from physical-device evidence. The target information below was supplied by the operator; it was not independently read from an attached device. `adb devices -l` completed with exit 0 but returned an empty device list, and no matching macOS BLE peer was available. Therefore neither prototype role is selected and the three-second unlock target is **UNASSESSED**.

## Target reported by operator

| Field | Reported value | Independent evidence |
|---|---|---|
| Device | realme GT5 Pro | NOT RUN |
| OS | Android 16 / realme UI 7.0 | NOT RUN |
| API level | Expected API 36 from the reported OS | NOT RUN; not queried from hardware |
| BLE / Companion features | Unknown | NOT RUN |

## Automatic evidence

The native module has deterministic JVM coverage for the unauthenticated outer GATT envelope and role-neutral transport reducer. The checks cover exact v1 Challenge (249 bytes) and Response (390 bytes) reassembly, every two-fragment split, malformed/empty/oversized/truncated/duplicate/out-of-order fragments, callback identity tokens, reconnection, Bluetooth-off reset, process-runtime replacement, stale callbacks/timers, and bounded retry/cooldown without sleeping.

The native-only responder additionally covers trusted paired-Mac challenge authentication, Rust fixture-compatible ECDH/HKDF/AES-GCM/low-S response bytes, exact retransmission, unsigned counter limits, atomic pairing/revocation/cache CAS, corrupted durable records, caller-buffer mutation, bounded cross-process ephemeral-key leases, and crash-leftover alias sweeping. Its production composition requires a non-exportable TEE/StrongBox ECDH key and otherwise fails closed. The final combined run passed 94/94 JVM tests (including 26 transport and 24 responder tests), lint, the app Debug APK, the plugin Debug AAR, and app/plugin instrumentation APK compilation. The three responder instrumentation tests were compiled but not executed.

These tests do not demonstrate AndroidKeyStore/SQLite behavior on the GT5 Pro, radio availability, background wake-up, RSSI ownership, OEM behavior, latency, or battery use. JCA provider-internal key copies also cannot be proven immediately erased through public APIs; explicitly owned secret arrays are wiped and provider objects are kept narrowly scoped. Both role adapters are prototypes behind `BleRoleSelector.productionRole() == Disabled`.

## Physical-device matrix

| Scenario | Mac central / phone peripheral | Mac peripheral / phone central |
|---|---|---|
| Manufacturer, model, API and BLE features queried | NOT RUN | NOT RUN |
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
