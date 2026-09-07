//! Concrete macOS read/status/uninstall adapter.
//!
//! Production installation remains deliberately gated: Task 8 has not pinned
//! a Developer ID requirement and immutable deny-only service measurement, and
//! the root-only sealed-staging copier still needs ACL/xattr/flags validation.
//! Consequently every mutating command fails before backend construction,
//! lock/journal creation, component access, or Authorization Services calls.
//! Only status and plan inspection are usable in Task 7.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use plist::{Dictionary, Value};
use repose_authdb_policy::ScreenSaverPolicy;
use sha2::{Digest, Sha256};

use crate::artifact_verify::{
    ArtifactAttestor, ArtifactError, InspectedDevelopmentPackage, ProductionSealedArtifacts,
    SignaturePolicy, inspect_development_package, verify_apply_artifacts,
};
use crate::authdb::AuthorizationDb;
use crate::install_transaction::{
    BackendError, Component, ComponentState, DurableJournal, InstallBackend,
    InstallReceiptFingerprint, InstallReceiptState, InstallRequest, JournalOperation, JournalPhase,
    ServiceRemovalState, Status, inspect_status, install, repair_policy, uninstall,
    verify_named_rule_v1,
};

pub const PLUGIN_PATH: &str = "/Library/Security/SecurityAgentPlugins/ReposeUnlock.bundle";
pub const SERVICE_PATH: &str = "/Library/PrivilegedHelperTools/ai.repose.unlockd";
pub const LAUNCHD_PATH: &str = "/Library/LaunchDaemons/ai.repose.unlockd.plist";
pub const SOCKET_DIRECTORY: &str = "/var/run/ai.repose.unlockd";
pub const SOCKET_PATH: &str = "/var/run/ai.repose.unlockd/consume.sock";
const STATE_DIRECTORY: &str = "/private/var/db/ai.repose.unlockd";
const LOCK_PATH: &str = "/private/var/db/ai.repose.unlockd/install.lock";
const JOURNAL_PATH: &str = "/private/var/db/ai.repose.unlockd/transaction.plist";
const INSTALL_RECEIPT_PATH: &str = "/private/var/db/ai.repose.unlockd/install-receipt.plist";
const MAX_POLICY_BYTES: usize = 1024 * 1024;
const MAX_JOURNAL_BYTES: usize = 3 * 1024 * 1024;
const MAX_RECEIPT_BYTES: usize = 64 * 1024;
const MAX_COMPONENT_BYTES: usize = 128 * 1024 * 1024;
const ROOT_DIRECTORY_MODE: u32 = 0o700;
const ROOT_FILE_MODE: u32 = 0o600;
const LAUNCHD_LABEL: &str = "system/ai.repose.unlockd";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ProductionBackend {
    authdb: AuthorizationDb,
    lock: Option<File>,
    journal: Option<JournalRecord>,
}

impl ProductionBackend {
    #[must_use]
    fn new() -> Self {
        Self {
            authdb: AuthorizationDb::new(),
            lock: None,
            journal: None,
        }
    }

    fn require_root(&self) -> Result<(), BackendError> {
        if effective_uid() == 0 {
            Ok(())
        } else {
            Err(BackendError::new("--apply requires effective uid 0"))
        }
    }

    fn require_mutation_gate(&self) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn journal_record(&self) -> Result<&JournalRecord, BackendError> {
        self.journal
            .as_ref()
            .ok_or_else(|| BackendError::new("durable transaction journal is missing"))
    }

    fn read_journal_from_disk(&self) -> Result<Option<JournalRecord>, BackendError> {
        let path = Path::new(JOURNAL_PATH);
        let Some(bytes) = read_optional_root_file(path, MAX_JOURNAL_BYTES)? else {
            return Ok(None);
        };
        JournalRecord::decode(&bytes).map(Some)
    }

    fn persist_journal(&self, record: &JournalRecord) -> Result<(), BackendError> {
        write_atomic_root_file(Path::new(JOURNAL_PATH), &record.encode()?)
    }
}

impl Default for ProductionBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InstallBackend for ProductionBackend {
    type VerifiedArtifacts = ProductionSealedArtifacts;

    fn acquire_exclusive_lock(&mut self) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        self.require_root()?;
        ensure_state_directory()?;
        let lock = acquire_owned_lock(Path::new(LOCK_PATH), 0, 0)?;
        self.lock = Some(lock);
        Ok(())
    }

    fn release_exclusive_lock(&mut self) {
        if let Some(lock) = self.lock.take() {
            // SAFETY: best-effort unlock of this process-owned descriptor.
            let _ = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) };
        }
    }

    fn load_journal(&mut self) -> Result<Option<DurableJournal>, BackendError> {
        self.journal = self.read_journal_from_disk()?;
        Ok(self.journal.as_ref().map(JournalRecord::durable_view))
    }

    fn start_journal(
        &mut self,
        operation: JournalOperation,
        policy: &[u8],
        named_rule: Option<&[u8]>,
        repair_base: Option<&[u8]>,
    ) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        self.require_root()?;
        if self.read_journal_from_disk()?.is_some() {
            return Err(BackendError::new(
                "an unfinished transaction journal exists",
            ));
        }
        let receipt = read_install_receipt_snapshot()?;
        let prior_receipt = receipt
            .as_ref()
            .map_or(InstallReceiptState::Missing, |(bytes, _)| {
                let digest: [u8; 32] = Sha256::digest(bytes).into();
                InstallReceiptState::Trusted(InstallReceiptFingerprint::new(digest))
            });
        let identities = receipt
            .as_ref()
            .map_or([None; 4], |(_, receipt)| receipt.identities());
        let record = JournalRecord {
            operation,
            phase: JournalPhase::Prepared,
            policy: policy.to_vec(),
            named_rule: named_rule.map(<[u8]>::to_vec),
            repair_base: repair_base.map(<[u8]>::to_vec),
            prior_receipt,
            target_receipt: None,
            identities,
        };
        self.persist_journal(&record)?;
        self.journal = Some(record);
        Ok(())
    }

    fn record_journal_phase(&mut self, phase: JournalPhase) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        let mut record = self.journal_record()?.clone();
        record.phase = phase;
        self.persist_journal(&record)?;
        self.journal = Some(record);
        Ok(())
    }

    fn record_target_receipt(
        &mut self,
        fingerprint: InstallReceiptFingerprint,
    ) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        let mut record = self.journal_record()?.clone();
        record.target_receipt = Some(fingerprint);
        self.persist_journal(&record)?;
        self.journal = Some(record);
        Ok(())
    }

    fn finish_journal(&mut self) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        let path = Path::new(JOURNAL_PATH);
        if let Some(metadata) = symlink_metadata_optional(path)? {
            validate_root_file(path, &metadata)?;
            fs::remove_file(path).map_err(backend_io)?;
            sync_directory(Path::new(STATE_DIRECTORY))?;
        }
        self.journal = None;
        Ok(())
    }

    fn verify_apply_artifacts(
        &mut self,
        path: &Path,
    ) -> Result<Self::VerifiedArtifacts, BackendError> {
        verify_apply_artifacts(path).map_err(|error| BackendError::new(error.to_string()))
    }

    fn read_screensaver_rule(&mut self) -> Result<Vec<u8>, BackendError> {
        self.authdb.read_screensaver()
    }

    fn set_screensaver_rule(&mut self, bytes: &[u8]) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        self.require_root()?;
        let policy = ScreenSaverPolicy::parse(bytes)
            .map_err(|error| BackendError::new(error.to_string()))?;
        self.authdb.write_screensaver(&policy)
    }

    fn read_named_rule(&mut self) -> Result<Option<Vec<u8>>, BackendError> {
        self.authdb.read_named_v1()
    }

    fn set_named_rule(&mut self, bytes: &[u8]) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        self.require_root()?;
        verify_named_rule_v1(bytes).map_err(|error| BackendError::new(error.to_string()))?;
        self.authdb.install_named_v1()
    }

    fn remove_named_rule(&mut self) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        self.require_root()?;
        self.authdb.remove_named_v1()
    }

    fn persist_install_backup(&mut self, bytes: &[u8]) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        self.require_root()?;
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(BackendError::new("policy backup is too large"));
        }
        let record = BackupRecord::new(bytes).encode()?;
        write_unique_owned_file(Path::new(STATE_DIRECTORY), "install-backup", &record, 0, 0)
            .map(|_| ())
    }

    fn read_validated_backup(&mut self, path: &Path) -> Result<Vec<u8>, BackendError> {
        self.require_root()?;
        if !path.is_absolute() || path == Path::new("/") {
            return Err(BackendError::new(
                "backup path must be explicit and absolute",
            ));
        }
        let container = read_required_root_file(path, MAX_POLICY_BYTES + 4096)?;
        let bytes = BackupRecord::decode(&container)?.policy;
        let policy = ScreenSaverPolicy::parse(&bytes)
            .map_err(|error| BackendError::new(error.to_string()))?;
        if !policy.has_exactly_one_password_fallback() {
            return Err(BackendError::new("backup has no unique password fallback"));
        }
        Ok(bytes)
    }

    fn begin_component_update(&mut self, _: &Self::VerifiedArtifacts) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn stage_component(
        &mut self,
        _: Component,
        _: &Self::VerifiedArtifacts,
    ) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn verify_staged_components(&mut self) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn verify_staged_deny_only_health(&mut self) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn commit_component_update(&mut self) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn activate_installed_service(&mut self) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn verify_active_service_and_components(
        &mut self,
        _: Option<InstallReceiptFingerprint>,
    ) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn persist_install_receipt(&mut self) -> Result<InstallReceiptFingerprint, BackendError> {
        Err(production_gate_error())
    }

    fn verify_install_receipt(&mut self) -> Result<InstallReceiptState, BackendError> {
        let snapshot = read_install_receipt_snapshot()?;
        let Some((bytes, receipt)) = snapshot else {
            let occupants_present = Component::INSTALL_ORDER
                .into_iter()
                .map(component_existing_path)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|path| path.is_some());
            return if occupants_present {
                Err(BackendError::new(
                    "fixed component path is occupied without an install receipt",
                ))
            } else {
                Ok(InstallReceiptState::Missing)
            };
        };
        for (index, component) in Component::INSTALL_ORDER.into_iter().enumerate() {
            if let Some(path) = component_existing_path(component)? {
                let metadata = fs::symlink_metadata(&path).map_err(backend_io)?;
                verify_component_measurement(
                    &path,
                    &metadata,
                    component,
                    &receipt.components[index],
                    0,
                    0,
                )?;
            }
        }
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        Ok(InstallReceiptState::Trusted(
            InstallReceiptFingerprint::new(digest),
        ))
    }

    fn rollback_component_update(&mut self) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn finish_component_update(&mut self) -> Result<(), BackendError> {
        Err(production_gate_error())
    }

    fn component_state(&mut self) -> Result<ComponentState, BackendError> {
        let present = Component::INSTALL_ORDER
            .map(component_existing_path)
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        if present.iter().all(Option::is_none) {
            Ok(ComponentState::Missing)
        } else {
            // No production measurement/loaded-image proof exists before the
            // Task 8 gate, so presence must never be reported healthy.
            Ok(ComponentState::Drifted)
        }
    }

    fn service_removal_state(
        &mut self,
        _: InstallReceiptFingerprint,
    ) -> Result<ServiceRemovalState, BackendError> {
        // Opening this requires Task 8's loaded pid/inode/signing measurement
        // adapter. A launchd label alone is never treated as ownership.
        Err(production_gate_error())
    }

    fn stop_service(&mut self, _: InstallReceiptFingerprint) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        self.require_root()?;
        let status = fixed_command("/bin/launchctl", &["bootout", LAUNCHD_LABEL])?;
        let socket_present = symlink_metadata_optional(Path::new(SOCKET_PATH))?.is_some();
        classify_bootout_result(status.success(), socket_present)
    }

    fn remove_component(
        &mut self,
        component: Component,
        _: InstallReceiptFingerprint,
    ) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        self.require_root()?;
        let receipt = read_optional_root_file(Path::new(INSTALL_RECEIPT_PATH), MAX_RECEIPT_BYTES)?;
        quarantine_verify_and_remove(
            self.journal_record()?,
            receipt.as_deref(),
            component,
            component_path(component),
            0,
            0,
        )
    }

    fn finish_component_removal(
        &mut self,
        _: Option<InstallReceiptFingerprint>,
    ) -> Result<(), BackendError> {
        self.require_mutation_gate()?;
        self.require_root()?;
        if Component::INSTALL_ORDER
            .into_iter()
            .map(component_existing_path)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|path| path.is_some())
        {
            return Err(BackendError::new(
                "cannot remove install receipt while components remain",
            ));
        }
        let receipt = Path::new(INSTALL_RECEIPT_PATH);
        if let Some(bytes) = read_optional_root_file(receipt, MAX_RECEIPT_BYTES)? {
            let expected = self.journal_record()?.prior_receipt;
            let digest: [u8; 32] = Sha256::digest(&bytes).into();
            if expected != InstallReceiptState::Trusted(InstallReceiptFingerprint::new(digest)) {
                return Err(BackendError::new(
                    "install receipt changed before final removal",
                ));
            }
            InstallReceipt::decode(&bytes)?;
            fs::remove_file(receipt).map_err(backend_io)?;
            sync_directory(Path::new(STATE_DIRECTORY))?;
        }
        Ok(())
    }
}

mod attestation;
mod components;
mod fs_state;
mod journal;
mod plugin_fs;
mod process;
mod receipt;
#[cfg(test)]
mod tests;

pub use attestation::{
    SystemAttestor, apply_production_install, apply_production_repair, apply_production_uninstall,
    inspect_production_status, verify_apply_package_before_mutation, verify_plan_only_package,
    verify_production_mutation_gate,
};
use components::*;
use fs_state::*;
use journal::*;
use plugin_fs::*;
use process::*;
use receipt::*;

fn production_gate_error() -> BackendError {
    BackendError::new(
        "production apply gate is closed until Task 8 pins signed measurements and sealed staging",
    )
}

fn backend_io(error: std::io::Error) -> BackendError {
    BackendError::new(format!("installer I/O failed: {error}"))
}

#[must_use]
pub fn effective_uid() -> u32 {
    // SAFETY: geteuid has no arguments or memory safety preconditions.
    unsafe { libc::geteuid() }
}
