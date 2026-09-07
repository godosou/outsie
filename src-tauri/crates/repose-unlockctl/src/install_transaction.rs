//! Transaction ordering for the fixed macOS screensaver authorization path.
//!
//! The backend surface deliberately has no authorization-right parameter. Its
//! only policy operations are the fixed screensaver right and Repose's fixed
//! named rule. A production backend must serialize mutations with a root-owned,
//! no-follow lock and implement component staging as an on-volume journaled
//! rename transaction.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use repose_authdb_policy::{PolicyError, PolicySpec, ScreenSaverPolicy};

pub const NAMED_RULE_NAME: &str = "ai.repose.unlock";
pub const PLUGIN_MECHANISM: &str = "ReposeUnlock:unlock,privileged";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Component {
    Plugin,
    Service,
    LaunchdPlist,
    SocketDirectory,
}

impl Component {
    pub const INSTALL_ORDER: [Self; 4] = [
        Self::Plugin,
        Self::Service,
        Self::SocketDirectory,
        Self::LaunchdPlist,
    ];
    pub const REMOVE_ORDER: [Self; 4] = [
        Self::LaunchdPlist,
        Self::Service,
        Self::Plugin,
        Self::SocketDirectory,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentState {
    Missing,
    HealthyDenyOnly,
    Drifted,
}

/// Result of validating the root-owned receipt for an installed generation.
/// `Trusted` binds every component that is still present to the receipt; the
/// active-service verifier separately proves completeness and runtime health.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstallReceiptFingerprint([u8; 32]);

impl InstallReceiptFingerprint {
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallReceiptState {
    Missing,
    Trusted(InstallReceiptFingerprint),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceRemovalState {
    RunningTrusted,
    Stopped,
}

impl InstallReceiptState {
    #[must_use]
    pub const fn is_trusted(self) -> bool {
        matches!(self, Self::Trusted(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    NotInstalled,
    InstalledDenyOnly,
    Drifted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalOperation {
    Install,
    Uninstall,
    Repair,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalPhase {
    Prepared,
    PolicyInactive,
    ComponentsCommittedHealthy,
    NamedReady,
    PolicyActive,
    Committed,
    ComponentsRemoved,
    /// A failed repair reached a proven password-only terminal state. Recovery
    /// may only finish clearing this journal; it must never roll forward.
    Aborted,
}

/// Security-relevant, backend-independent view of the durable journal.
///
/// Filesystem identities and rollback inode handles remain backend-owned, but
/// recovery ordering is intentionally implemented once in this module so the
/// fake and production adapters cannot disagree about authorization safety.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableJournal {
    pub operation: JournalOperation,
    pub phase: JournalPhase,
    pub policy: Vec<u8>,
    pub named_rule: Option<Vec<u8>>,
    /// Validated password-fallback source for policy-only repair. This lets a
    /// fresh process recover even when the live right is malformed.
    pub repair_base: Option<Vec<u8>>,
    /// Exact fingerprint of the receipt that existed before this transaction.
    /// Recovery must restore this generation—not merely any valid receipt—
    /// before reactivating an old authorization path.
    pub prior_receipt: InstallReceiptState,
    /// Exact receipt created for this install attempt. It is durably recorded
    /// before the named rule or screensaver policy may reference the new
    /// generation. `None` is never authority to roll an install forward.
    pub target_receipt: Option<InstallReceiptFingerprint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallRequest {
    artifacts: PathBuf,
}

impl InstallRequest {
    #[must_use]
    pub fn new(artifacts: PathBuf) -> Self {
        Self { artifacts }
    }

    #[must_use]
    pub fn artifacts(&self) -> &Path {
        &self.artifacts
    }
}

pub trait InstallBackend {
    /// An opaque, backend-owned source capability. Production keeps verified
    /// descriptors here so later staging does not reopen attacker-controlled
    /// paths by name.
    type VerifiedArtifacts;

    fn acquire_exclusive_lock(&mut self) -> Result<(), BackendError>;
    fn release_exclusive_lock(&mut self);
    fn load_journal(&mut self) -> Result<Option<DurableJournal>, BackendError>;
    fn start_journal(
        &mut self,
        operation: JournalOperation,
        policy: &[u8],
        named_rule: Option<&[u8]>,
        repair_base: Option<&[u8]>,
    ) -> Result<(), BackendError>;
    fn record_journal_phase(&mut self, phase: JournalPhase) -> Result<(), BackendError>;
    fn record_target_receipt(
        &mut self,
        fingerprint: InstallReceiptFingerprint,
    ) -> Result<(), BackendError>;
    fn finish_journal(&mut self) -> Result<(), BackendError>;

    fn verify_apply_artifacts(
        &mut self,
        path: &Path,
    ) -> Result<Self::VerifiedArtifacts, BackendError>;

    fn read_screensaver_rule(&mut self) -> Result<Vec<u8>, BackendError>;
    fn set_screensaver_rule(&mut self, bytes: &[u8]) -> Result<(), BackendError>;
    fn read_named_rule(&mut self) -> Result<Option<Vec<u8>>, BackendError>;
    fn set_named_rule(&mut self, bytes: &[u8]) -> Result<(), BackendError>;
    fn remove_named_rule(&mut self) -> Result<(), BackendError>;

    fn persist_install_backup(&mut self, bytes: &[u8]) -> Result<(), BackendError>;
    fn read_validated_backup(&mut self, path: &Path) -> Result<Vec<u8>, BackendError>;

    fn begin_component_update(
        &mut self,
        artifacts: &Self::VerifiedArtifacts,
    ) -> Result<(), BackendError>;
    fn stage_component(
        &mut self,
        component: Component,
        artifacts: &Self::VerifiedArtifacts,
    ) -> Result<(), BackendError>;
    fn verify_staged_components(&mut self) -> Result<(), BackendError>;
    /// Runs only a root-owned, staged service after hashes and signatures have
    /// been revalidated. Source artifact directories are never executed.
    fn verify_staged_deny_only_health(&mut self) -> Result<(), BackendError>;
    fn commit_component_update(&mut self) -> Result<(), BackendError>;
    fn activate_installed_service(&mut self) -> Result<(), BackendError>;
    /// Includes an exact loaded-image identity check, socket health, and the
    /// permanent deny-only response—not merely an on-disk file comparison.
    fn verify_active_service_and_components(
        &mut self,
        expected: Option<InstallReceiptFingerprint>,
    ) -> Result<(), BackendError>;
    /// Atomically binds the newly installed generation's component
    /// measurements and signing identities before an authorization rule can
    /// reference it.
    fn persist_install_receipt(&mut self) -> Result<InstallReceiptFingerprint, BackendError>;
    /// Validates an existing root-owned receipt. Implementations must never
    /// learn ownership from whatever happens to occupy a fixed path now.
    fn verify_install_receipt(&mut self) -> Result<InstallReceiptState, BackendError>;
    /// Restores every prior inode, restarts it, and verifies its loaded-image
    /// identity and health before returning success. This also restores the
    /// prior generation receipt atomically with the components.
    fn rollback_component_update(&mut self) -> Result<(), BackendError>;
    fn finish_component_update(&mut self) -> Result<(), BackendError>;

    fn component_state(&mut self) -> Result<ComponentState, BackendError>;
    /// Distinguishes an already stopped service from a running process whose
    /// loaded image is exactly bound to the install receipt. A foreign loaded
    /// job is an error, never an instruction to bootout by label.
    fn service_removal_state(
        &mut self,
        expected: InstallReceiptFingerprint,
    ) -> Result<ServiceRemovalState, BackendError>;
    /// Revalidates `expected` in the same backend operation that stops the
    /// loaded service. A stale outer receipt read must never authorize bootout.
    fn stop_service(&mut self, expected: InstallReceiptFingerprint) -> Result<(), BackendError>;
    /// Revalidates `expected` inside the quarantine/remove operation, after
    /// resolving the current occupant but before any destructive effect.
    fn remove_component(
        &mut self,
        component: Component,
        expected: InstallReceiptFingerprint,
    ) -> Result<(), BackendError>;
    /// Deletes the receipt only after every component is durably absent.
    fn finish_component_removal(
        &mut self,
        expected: Option<InstallReceiptFingerprint>,
    ) -> Result<(), BackendError>;
}

mod install;
mod policy;
mod recovery;
mod repair;
mod uninstall;

pub use install::install;
pub use policy::{named_rule_v1, verify_named_rule_v1};
pub use recovery::recover_pending;
pub use repair::repair_policy;
pub use uninstall::uninstall;

use policy::{
    deactivate_policy_for_repair, live_policy_is_inactive, restore_named_if_unchanged,
    restore_repose_on_latest_live_policy, surgically_remove_live_repose,
};
use policy::{policies_equivalent, same_named_definition, verify_repose_absent};
use recovery::recover_loaded_journal;
use uninstall::continue_uninstall;

pub fn inspect_status<B: InstallBackend>(backend: &mut B) -> Result<Status, TransactionError> {
    if backend
        .load_journal()
        .map_err(TransactionError::primary)?
        .is_some()
    {
        return Ok(Status::Drifted);
    }
    let components = backend
        .component_state()
        .map_err(TransactionError::primary)?;
    let policy = backend
        .read_screensaver_rule()
        .map_err(TransactionError::primary)?;
    let policy = match ScreenSaverPolicy::parse(&policy) {
        Ok(policy) => policy,
        Err(_) => return Ok(Status::Drifted),
    };
    let active = policy.repose_candidate_index().is_some();
    let policy_valid = policy.verify_installed(&PolicySpec::v1()).is_ok();
    let named = backend
        .read_named_rule()
        .map_err(TransactionError::primary)?;
    let named_valid = named
        .as_deref()
        .is_some_and(|bytes| verify_named_rule_v1(bytes).is_ok());
    let receipt = match backend.verify_install_receipt() {
        Ok(receipt) => receipt,
        Err(_) => return Ok(Status::Drifted),
    };
    Ok(
        match (components, receipt, active, named.as_ref(), named_valid) {
            (ComponentState::Missing, InstallReceiptState::Missing, false, None, _) => {
                Status::NotInstalled
            }
            (
                ComponentState::HealthyDenyOnly,
                InstallReceiptState::Trusted(fingerprint),
                true,
                Some(_),
                true,
            ) if policy_valid
                && backend
                    .verify_active_service_and_components(Some(fingerprint))
                    .is_ok() =>
            {
                Status::InstalledDenyOnly
            }
            _ => Status::Drifted,
        },
    )
}

/// Policy-only repair. Component repair requires a separately verified package
/// and therefore uses `install`; this command never sources executables from a
/// policy backup. A stale backup is used only if the live policy is malformed.
fn active_dependency_closure<B: InstallBackend>(
    backend: &mut B,
    expected_receipt: InstallReceiptState,
) -> Result<bool, BackendError> {
    let InstallReceiptState::Trusted(expected_fingerprint) = expected_receipt else {
        return Ok(false);
    };
    let policy = backend.read_screensaver_rule()?;
    let policy_valid = ScreenSaverPolicy::parse(&policy)
        .is_ok_and(|policy| policy.verify_installed(&PolicySpec::v1()).is_ok());
    if !policy_valid {
        return Ok(false);
    }
    let named_valid = backend
        .read_named_rule()?
        .as_deref()
        .is_some_and(|bytes| verify_named_rule_v1(bytes).is_ok());
    if !named_valid
        || backend.component_state()? != ComponentState::HealthyDenyOnly
        || backend.verify_install_receipt()? != expected_receipt
    {
        return Ok(false);
    }
    Ok(backend
        .verify_active_service_and_components(Some(expected_fingerprint))
        .is_ok())
}

struct RollbackState {
    #[allow(dead_code)]
    initial_policy: Vec<u8>,
    initial_named: Option<Vec<u8>>,
    initially_active: bool,
    policy_was_deactivated: bool,
    final_policy_attempted: bool,
    final_policy_expected: Option<Vec<u8>>,
    named_attempted: bool,
    components_started: bool,
    initial_receipt: InstallReceiptState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendError {
    detail: String,
}

impl BackendError {
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl Display for BackendError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for BackendError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionError {
    primary: BackendError,
    rollback_failures: Vec<String>,
}

impl TransactionError {
    fn primary(primary: BackendError) -> Self {
        Self {
            primary,
            rollback_failures: Vec::new(),
        }
    }

    #[must_use]
    pub fn rollback_failures(&self) -> &[String] {
        &self.rollback_failures
    }
}

impl Display for TransactionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.primary)?;
        if !self.rollback_failures.is_empty() {
            write!(
                formatter,
                "; rollback: {}",
                self.rollback_failures.join("; ")
            )?;
        }
        Ok(())
    }
}

impl Error for TransactionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.primary)
    }
}

fn policy_error(error: PolicyError) -> TransactionError {
    TransactionError::primary(BackendError::new(error.to_string()))
}
