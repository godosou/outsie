# macOS Authorization Feasibility Checklist (Gate Closed)

> Status: **NOT RUN / GATE CLOSED**. Every checkbox below remains intentionally unchecked.
> The read-only host inventory in
> [`docs/validation/macos-authorization-results.md`](../../../docs/validation/macos-authorization-results.md)
> confirms that this development Mac has not met the dedicated/restoreable-host or recovery
> prerequisites. Task 7 must not be used as evidence that automatic unlock is enabled or safe.
> Run this checklist only on a dedicated, restoreable Mac—never on a developer's daily Mac.

## Current host observation (not validation evidence)

Read-only inventory was recorded on 2026-09-08. It found macOS 14.6.1 (`23G93`) on an Apple M3
MacBook Pro (`Mac15,3`), Command Line Tools 16.2 selected, and zero valid code-signing identities.
Full Xcode presence was not verified. FileVault status/recovery, a second administrator, remote
recovery, erase/restore, power/network-independent recovery, and a tested backup restore are all
unverified. Flutter 3.38.9 metadata, Android SDK API 35, ADB 35.0.2, and Java 17 are present, but the
operator-provided realme GT5 Pro / Android 16 target was not independently enumerated or tested.

This inventory does not satisfy any checkbox. No Authorization Services operation, installation,
launchd action, lock-screen interaction, password trial, latency measurement, or recovery action was
performed. Exact command outcomes and the conditions for reopening the gate are in the linked
results document.

## Hard stop before any installation

- [ ] Dedicated Mac and exact hardware identifier recorded: `________________`
- [ ] Exact macOS version/build recorded: `________________`
- [ ] A tested erase/restore path is available, with estimated recovery time: `________________`
- [ ] A second administrator account can log in locally using a password.
- [ ] Remote recovery (SSH or Screen Sharing) was tested from another machine after lock.
- [ ] FileVault recovery key was independently verified and stored offline.
- [ ] Current system backup/snapshot completed and restore was tested.
- [ ] Power and network can be removed without losing the recovery channel.
- [ ] Existing `system.login.screensaver` policy was captured through the narrow adapter as a
      root-owned `0600` checksum-bound backup container; its SHA-256 is: `________________`
- [ ] A signed, offline copy of `repose-unlockctl` and its audited uninstall/repair procedure is
      available from outside the system volume.

If any item above is unchecked, stop. Do not run an apply command.

## Task 8 production-gate prerequisites

The Task 7 package is ad-hoc signed and **plan-only**. Its local `SHA256SUMS` proves consistency,
not publisher authenticity. Before opening the apply gate, record evidence for every item:

- [ ] Notarized installer/package signature and pinned Developer ID designated requirement.
- [ ] One signed manifest binds the complete plugin, service, launchd-template digest set,
      plugin/service signing identifiers, IPC protocol/package generation, minimum OS, CPU
      architecture, and the permanent `deny-only` build kind.
- [ ] Installer rejects downgrade, mixed generations, and a loaded service image whose inode/hash
      differs from the committed generation.
- [ ] Receipt persistence returns one exact target fingerprint; the journal durably binds it before
      named/policy activation, and recovery rejects a missing or different target fingerprint.
- [ ] Source is copied with fd-relative `openat`/`O_NOFOLLOW` traversal into root-only, same-volume
      staging; only the sealed snapshot is then hashed and signature-checked.
- [ ] Root-owned staging and destinations reject special mode bits, nontrivial ACLs, unexpected
      xattrs/resource forks, quarantine metadata, and immutable file flags before activation.
- [ ] Launchd plist is byte-identical to the embedded template and contains no extra trigger,
      environment, working-directory, stdout/stderr, `RunAtLoad`, or `KeepAlive` keys.
- [ ] The helper's fixed signed measurement independently proves the permanent deny-only build.
      `--health-check-deny-only` is used only as post-install liveness sanity, never attestation.
- [ ] The installer journal and rollback material survive a forced power loss at every phase.
- [ ] A dedicated maintenance window is active and all other privileged Authorization Services
      writers/installers are excluded. Last-moment read plus structural readback is recorded as
      best-effort drift detection, not compare-and-swap.
- [ ] Operation-by-syscall fault injection covers short writes, atomic rename, file and directory
      fsync, `EXDEV`, receipt replace/delete, quarantine resume, and backup-name collisions.
- [ ] Launchctl timeout, permission failure, ambiguous output, and non-authoritative not-found all
      retain the loaded job and components; only exact success or a separately authoritative
      absence proof may advance teardown.

Until all items are checked with attached evidence, every production mutation (`install --apply`,
`uninstall --apply`, and `repair --apply`) must fail closed before backend construction, lock,
journal, Authorization Services, filesystem, service, or launchd mutation.

## Baseline and plan-only checks

- [ ] `repose-unlockctl status` output captured: `________________`
- [ ] `repose-unlockctl plan-install --artifacts <package>` output captured: `________________`
- [ ] Plan output says `package-mode=unsigned-plan-only` and the apply gate is closed.
- [ ] Status and plan output state `authorization-writes=no-cas-maintenance-window-required`.
- [ ] Package tree, fixed launchd bytes, hashes, permissions, and ad-hoc signatures pass static
      verification without executing the package's service.
- [ ] `system.login.console` does not occur in the adapter binary/source or any requested right.

## Deny-only installation sequence (Task 8 validation build only)

- [ ] Journal is durable before the first mutation.
- [ ] If upgrading, the existing Repose screensaver candidate is surgically removed and read back
      first, leaving one password fallback with `k-of-n = 1`.
- [ ] Plugin, helper, socket directory (`root:wheel 0700`), and launchd plist are installed from the
      sealed generation using atomic rename plus file and parent-directory fsync.
- [ ] Launchd loads exactly the committed helper inode/hash and creates a `0600` socket.
- [ ] Permanently deny-only measurement is verified, then bounded liveness health passes.
- [ ] Exact named rule `ai.repose.unlock` is set and read back structurally.
- [ ] `system.login.screensaver` is modified last, with exactly one adjacent
      `ai.repose.unlock`/`use-login-window-ui` fallback and `k-of-n = 1`.
- [ ] Final live dependency closure (policy, named rule, every component, loaded image, socket, and
      deny-only response) is re-read before success.

## Password fallback and fault matrix

- [ ] At least 20 password unlocks succeed with no permit.
- [ ] No-permit added latency p95 is below 250 ms and every sample is below 500 ms.
- [ ] Password fallback succeeds after helper kill.
- [ ] Password fallback succeeds after helper hang/timeout.
- [ ] Password fallback succeeds with the socket absent or malformed.
- [ ] Password fallback succeeds with launchd bootout.
- [ ] Password fallback succeeds with plugin/service/launchd signature or hash drift.
- [ ] Password fallback succeeds across sleep/wake and password UI already visible.
- [ ] A permit ready before invocation is consumed once.
- [ ] A later permit causes at most one bounded `RequestInterrupt`, then is consumed once.
- [ ] Reboot, logout, FileVault preboot, Guest, fast-user-switching, and remote login never use the
      Repose path.
- [ ] Automatic unlock preserves login keychain, saved passwords, SSH keys, and browser credentials.

## Crash, concurrency, repair, and uninstall

- [ ] Forced power loss tested before and after every durable phase marker and persistent side
      effect; recovery never leaves an active policy with missing/mixed dependencies.
- [ ] Injected third-party changes before the last read and changes observable at readback survive or
      stop safely; evidence explicitly acknowledges the uncloseable privileged-writer race between
      Authorization Services read and write.
- [ ] A foreign definition observed by the last-moment read or structural readback is retained and
      stops the transaction. The unavoidable read-to-write window is explicitly covered by the
      exclusive maintenance precondition; this checklist does not claim Authorization Services CAS.
- [ ] Malformed live policy requires the explicit validated repair backup and retains dependencies
      whenever inactivity cannot be proven.
- [ ] Uninstall removes and reads back the screensaver reference, then the exact named rule, before
      bootout or component removal.
- [ ] Bootout and every component quarantine operation revalidate the journaled prior fingerprint
      internally; a generation swap after an outer receipt read causes no stop or unlink.
- [ ] Candidate reappearance after bootout or any unlink is surgically disabled and teardown pauses.
- [ ] Socket directory is removed last only when its journaled identity is unchanged and it is empty.
- [ ] A second uninstall after interruption completes idempotently without deleting foreign inodes.

## Evidence record

| Item | Result | Artifact/log/hash | Reviewer |
|---|---|---|---|
| Safety prerequisites | `NOT RUN / GATE CLOSED` | `________________` | `________________` |
| Signed-measurement gate | `NOT RUN / GATE CLOSED` | `________________` | `________________` |
| Password fallback | `NOT RUN / GATE CLOSED` | `________________` | `________________` |
| Permit timing | `NOT RUN / GATE CLOSED` | `________________` | `________________` |
| Fault injection | `NOT RUN / GATE CLOSED` | `________________` | `________________` |
| Power-loss recovery | `NOT RUN / GATE CLOSED` | `________________` | `________________` |
| Repair/uninstall | `NOT RUN / GATE CLOSED` | `________________` | `________________` |

Task 8 copies exact evidence into `docs/validation/macos-authorization-results.md`. A missing result,
unattended re-evaluation failure, password/keychain regression, or unverifiable dependency closure
means the feasibility gate fails and ordinary installation remains disabled.
