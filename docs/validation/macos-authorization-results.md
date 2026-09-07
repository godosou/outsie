# macOS Authorization Feasibility Results

> Gate status: **NOT RUN / GATE CLOSED**
>
> Initial host observation: 2026-09-08 00:31 CST; mobile toolchain evidence was refreshed during
> the same Task 14 verification run. No package was installed, no Authorization Services right was read or
> changed, no launchd job was loaded or removed, and no login, lock-screen, password, keychain,
> latency, power-loss, repair, or uninstall experiment was run.

## Decision

This host is not qualified as the dedicated, restoreable test Mac required by Task 8. It is an
active development Mac with no recorded erase/restore drill, second-administrator login test,
remote recovery test, independently verified FileVault recovery key, or tested backup restore.
The production mutation gate therefore remains closed.

Task 7's automated tests and unsigned/ad-hoc package checks are development evidence only. They do
not constitute real-machine authorization evidence and do not authorize `install --apply`,
`uninstall --apply`, or `repair --apply`.

## Local validation environment

Personal host and device inventory is omitted from the public source.
Automated checks do not establish real-device authorization readiness.

## External safety prerequisites

Every prerequisite is unresolved. No checkbox in the real-machine checklist is satisfied by this
inventory.

| Prerequisite | Status | Exact condition required to clear it |
|---|---|---|
| Dedicated Mac | **NOT VERIFIED** | Assign a Mac used only for this test and record its unique asset/hardware identifier. |
| Dedicated-host OS/build | **NOT VERIFIED** | On that assigned Mac, record exact `sw_vers` product version and build, and confirm that combination is in the reviewed validation matrix. |
| Erase/restore path | **NOT VERIFIED** | Perform and document an erase/restore rehearsal, including measured recovery time. |
| Second administrator | **NOT VERIFIED** | Demonstrate a distinct administrator can log in locally with a password after lock. |
| Remote recovery | **NOT VERIFIED** | Demonstrate SSH or Screen Sharing recovery from another machine after lock. |
| FileVault recovery | **NOT VERIFIED** | Independently verify the recovery key offline without recording the secret in this repository. |
| Backup restore | **NOT VERIFIED** | Complete a current backup and prove restoration on the dedicated Mac. |
| Power/network-independent recovery | **NOT VERIFIED** | Demonstrate that removing normal power and network access does not remove the documented recovery channel. |
| Offline repair media | **NOT AVAILABLE** | Provide a signed offline `repose-unlockctl`, verified uninstall/repair instructions, and their hashes. |
| Authorization backup | **NOT CAPTURED** | On the qualified test Mac only, capture the screensaver right through the narrow adapter into the checksum-bound backup container before mutation. |

Until every condition is independently reviewed, the correct action is to stop before any apply
command.

## Task 7 development evidence (not real-machine evidence)

The following evidence was produced without changing macOS authorization state:

- Task 7 implementation commit: `7bd947df133ab6a757c42ec0c1ee488b2f7d8100`.
- `cargo test --offline --workspace` passed, including 53 transaction failure/recovery tests in
  `repose-unlockctl` and the Objective-C adapter linked against fake Authorization Services symbols.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`, formatting checks, shell syntax
  checks, and Git diff checks passed before the Task 7 commit.
- The unsigned package script and static verifier exercised the fixed package tree, hashes,
  permissions, launchd template, and ad-hoc signatures. The verifier did not execute a
  caller-supplied helper.
- The package remains `unsigned-plan-only`; local `SHA256SUMS` proves consistency, not publisher
  authenticity.

These results exercise fake adapters and temporary artifacts. They provide no evidence about
password fallback timing, AuthorizationHost behavior, loginwindow re-evaluation, keychain effects,
launchd behavior on a real installed generation, or power-loss recovery on a Mac.

## Real-machine result matrix

| Validation area | Result | Evidence |
|---|---|---|
| Safety prerequisite rehearsal | **NOT RUN / GATE CLOSED** | None |
| Signed and notarized production package | **NOT RUN / GATE CLOSED** | No valid signing identity; Task 7 package is plan-only |
| Authorization policy baseline | **NOT RUN / GATE CLOSED** | No Authorization Services query was made |
| Deny-only installation | **NOT RUN / GATE CLOSED** | No installation or launchd action was taken |
| 20 password fallback trials | **NOT RUN / GATE CLOSED** | No lock-screen interaction was performed |
| No-permit latency p95/max | **NOT RUN / GATE CLOSED** | No samples; no timing values are claimed |
| Ready-before-invoke permit path | **NOT RUN / GATE CLOSED** | None |
| Visible-password-UI interrupt path | **NOT RUN / GATE CLOSED** | None |
| Sleep/wake and unattended re-evaluation | **NOT RUN / GATE CLOSED** | None |
| Login keychain and credential regression | **NOT RUN / GATE CLOSED** | None |
| Reboot/logout/FileVault/Guest/FUS/remote scope | **NOT RUN / GATE CLOSED** | None |
| Kill/hang/socket/launchd fault fallback | **NOT RUN / GATE CLOSED** | None |
| Forced power-loss recovery | **NOT RUN / GATE CLOSED** | None |
| Repair and uninstall | **NOT RUN / GATE CLOSED** | None |

## Conditions for reopening Task 8

Before any production gate can open, every item in the checklist's **Hard stop before any
installation** section must be checked with reviewable evidence on the dedicated Mac. Every
production-gate prerequisite in the next checklist section must also be checked. A subset of either
section is not sufficient. In particular:

1. Provide a notarized package and pin the Developer ID/designated requirements, complete signed
   manifest, component identities/digests, protocol/package generation, OS/architecture bounds, and
   permanent deny-only measurement.
2. Complete fd-stable sealed staging plus ACL, xattr/resource-fork, special-bit, file-flag, downgrade,
   mixed-generation, receipt, and loaded-image enforcement.
3. Complete operation-by-syscall fault injection and authoritative launchctl outcome handling.
4. Establish a dedicated maintenance window and exclusive privileged-writer coordination. The
   Authorization Services read-to-write window has no compare-and-swap guarantee.
5. Obtain an explicit human go/no-go review only after all Hard stop evidence and all production
   security-gate evidence are complete.

Opening the gate still does not itself count as validation. The remaining installation, fallback,
scope, fault, power-loss, repair, and uninstall checklist sections must then be executed and recorded
with exact logs, hashes, and timings. Any missing evidence or password/keychain regression keeps
ordinary installation disabled.
