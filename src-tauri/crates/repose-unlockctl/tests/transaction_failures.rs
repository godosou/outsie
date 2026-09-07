use std::path::{Path, PathBuf};

use plist::Value;
use repose_authdb_policy::{PolicySpec, ScreenSaverPolicy};
use repose_unlockctl::install_transaction::{
    BackendError, Component, ComponentState, DurableJournal, InstallBackend,
    InstallReceiptFingerprint, InstallReceiptState, InstallRequest, JournalOperation, JournalPhase,
    ServiceRemovalState, Status, inspect_status, install, recover_pending, repair_policy,
    uninstall,
};

const THIRD_PARTY_POLICY: &[u8] =
    include_bytes!("../../repose-authdb-policy/tests/fixtures/authorizationdb/third-party.plist");

#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot {
    policy: Vec<u8>,
    named_rule: Option<Vec<u8>>,
    components: [u64; 4],
    loaded_service: u64,
    receipt_generation: Option<u64>,
}

struct FakeBackend {
    policy: Vec<u8>,
    named_rule: Option<Vec<u8>>,
    components: [u64; 4],
    loaded_service: u64,
    receipt_generation: Option<u64>,
    component_snapshot: Option<([u64; 4], u64, Option<u64>)>,
    staged: [u64; 4],
    calls: Vec<&'static str>,
    fail_at: Option<usize>,
    second_fail_at: Option<usize>,
    fail_phase_before_effect: Option<JournalPhase>,
    failed: bool,
    second_failed: bool,
    backups: Vec<Vec<u8>>,
    policy_reads: usize,
    replace_policy_on_read: Option<(usize, Vec<u8>)>,
    inject_policy_after_read_return: Option<(usize, Vec<u8>)>,
    named_reads: usize,
    replace_named_on_read: Option<(usize, Option<Vec<u8>>)>,
    inject_named_after_read_return: Option<(usize, Option<Vec<u8>>)>,
    inject_policy_after_call: Option<(&'static str, Vec<u8>)>,
    inject_named_after_call: Option<(&'static str, Option<Vec<u8>>)>,
    inject_loaded_after_call: Option<(&'static str, u64)>,
    inject_installed_generation_after_call: Option<(&'static str, u64)>,
    inject_installed_generation_before_remove: Option<(Component, u64)>,
    component_state_override: Option<ComponentState>,
    leave_new_receipt_on_rollback: bool,
    component_mutation_while_policy_active: bool,
    journal: Option<FakeJournal>,
    power_snapshots: Vec<(JournalPhase, Snapshot, FakeJournal)>,
    effect_snapshots: Vec<EffectSnapshot>,
}

#[derive(Clone)]
struct FakeJournal {
    record: DurableJournal,
    snapshot: Snapshot,
}

#[derive(Clone)]
struct EffectSnapshot {
    label: &'static str,
    state: Snapshot,
    staged: [u64; 4],
    component_snapshot: Option<([u64; 4], u64, Option<u64>)>,
    journal: FakeJournal,
}

impl FakeBackend {
    fn receipt_state(&self) -> InstallReceiptState {
        match self.receipt_generation {
            Some(generation) => {
                let mut fingerprint = [0_u8; 32];
                fingerprint[..8].copy_from_slice(&generation.to_be_bytes());
                InstallReceiptState::Trusted(InstallReceiptFingerprint::new(fingerprint))
            }
            None => InstallReceiptState::Missing,
        }
    }

    fn stock() -> Self {
        Self {
            policy: THIRD_PARTY_POLICY.to_vec(),
            named_rule: None,
            components: [0; 4],
            loaded_service: 0,
            receipt_generation: None,
            component_snapshot: None,
            staged: [0; 4],
            calls: Vec::new(),
            fail_at: None,
            second_fail_at: None,
            fail_phase_before_effect: None,
            failed: false,
            second_failed: false,
            backups: Vec::new(),
            policy_reads: 0,
            replace_policy_on_read: None,
            inject_policy_after_read_return: None,
            named_reads: 0,
            replace_named_on_read: None,
            inject_named_after_read_return: None,
            inject_policy_after_call: None,
            inject_named_after_call: None,
            inject_loaded_after_call: None,
            inject_installed_generation_after_call: None,
            inject_installed_generation_before_remove: None,
            component_state_override: None,
            leave_new_receipt_on_rollback: false,
            component_mutation_while_policy_active: false,
            journal: None,
            power_snapshots: Vec::new(),
            effect_snapshots: Vec::new(),
        }
    }

    fn installed() -> Self {
        let mut value = Self::stock();
        value.policy = ScreenSaverPolicy::parse(&value.policy)
            .unwrap()
            .install(&PolicySpec::v1())
            .unwrap()
            .to_bytes()
            .unwrap();
        value.named_rule = Some(repose_unlockctl::install_transaction::named_rule_v1());
        value.components = [1; 4];
        value.loaded_service = 1;
        value.receipt_generation = Some(1);
        value
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            policy: self.policy.clone(),
            named_rule: self.named_rule.clone(),
            components: self.components,
            loaded_service: self.loaded_service,
            receipt_generation: self.receipt_generation,
        }
    }

    fn complete(&mut self, name: &'static str) -> Result<(), BackendError> {
        self.calls.push(name);
        if self
            .inject_policy_after_call
            .as_ref()
            .is_some_and(|(call, _)| *call == name)
        {
            self.policy = self
                .inject_policy_after_call
                .take()
                .expect("checked policy injection")
                .1;
        }
        if self
            .inject_named_after_call
            .as_ref()
            .is_some_and(|(call, _)| *call == name)
        {
            self.named_rule = self
                .inject_named_after_call
                .take()
                .expect("checked named injection")
                .1;
        }
        if self
            .inject_loaded_after_call
            .as_ref()
            .is_some_and(|(call, _)| *call == name)
        {
            self.loaded_service = self
                .inject_loaded_after_call
                .take()
                .expect("checked loaded-service injection")
                .1;
        }
        if self
            .inject_installed_generation_after_call
            .as_ref()
            .is_some_and(|(call, _)| *call == name)
        {
            let generation = self
                .inject_installed_generation_after_call
                .take()
                .expect("checked installed-generation injection")
                .1;
            self.components = [generation; 4];
            self.loaded_service = generation;
            self.receipt_generation = Some(generation);
        }
        if !self.failed && self.fail_at == Some(self.calls.len()) {
            self.failed = true;
            return Err(BackendError::new(format!("injected after {name}")));
        }
        if !self.second_failed && self.second_fail_at == Some(self.calls.len()) {
            self.second_failed = true;
            return Err(BackendError::new(format!("injected again after {name}")));
        }
        Ok(())
    }

    fn component_index(component: Component) -> usize {
        match component {
            Component::Plugin => 0,
            Component::Service => 1,
            Component::LaunchdPlist => 2,
            Component::SocketDirectory => 3,
        }
    }

    fn capture_effect(&mut self, label: &'static str) {
        if let Some(journal) = self.journal.clone() {
            self.effect_snapshots.push(EffectSnapshot {
                label,
                state: self.snapshot(),
                staged: self.staged,
                component_snapshot: self.component_snapshot,
                journal,
            });
        }
    }
}

fn assert_snapshot_equivalent(actual: &Snapshot, expected: &Snapshot, context: &str) {
    let actual_policy = ScreenSaverPolicy::parse(&actual.policy).unwrap();
    let expected_policy = ScreenSaverPolicy::parse(&expected.policy).unwrap();
    assert!(
        actual_policy.structurally_equivalent(&expected_policy),
        "policy differs: {context}"
    );
    assert_eq!(
        actual.named_rule, expected.named_rule,
        "named rule: {context}"
    );
    assert_eq!(
        actual.components, expected.components,
        "components: {context}"
    );
    assert_eq!(
        actual.loaded_service, expected.loaded_service,
        "loaded service: {context}"
    );
    assert_eq!(
        actual.receipt_generation, expected.receipt_generation,
        "generation receipt: {context}"
    );
}

impl InstallBackend for FakeBackend {
    type VerifiedArtifacts = PathBuf;

    fn acquire_exclusive_lock(&mut self) -> Result<(), BackendError> {
        self.complete("acquire-exclusive-lock")
    }

    fn release_exclusive_lock(&mut self) {
        self.calls.push("release-exclusive-lock");
    }

    fn load_journal(&mut self) -> Result<Option<DurableJournal>, BackendError> {
        let journal = self.journal.as_ref().map(|journal| journal.record.clone());
        self.complete(if journal.is_some() {
            "load-journal"
        } else {
            "load-no-journal"
        })?;
        Ok(journal)
    }

    fn start_journal(
        &mut self,
        operation: JournalOperation,
        policy: &[u8],
        named_rule: Option<&[u8]>,
        repair_base: Option<&[u8]>,
    ) -> Result<(), BackendError> {
        self.journal = Some(FakeJournal {
            record: DurableJournal {
                operation,
                phase: JournalPhase::Prepared,
                policy: policy.to_vec(),
                named_rule: named_rule.map(<[u8]>::to_vec),
                repair_base: repair_base.map(<[u8]>::to_vec),
                prior_receipt: self.receipt_state(),
                target_receipt: None,
            },
            snapshot: self.snapshot(),
        });
        self.capture_effect("start-journal-effect");
        self.complete("start-journal")
    }

    fn record_journal_phase(&mut self, phase: JournalPhase) -> Result<(), BackendError> {
        if self.fail_phase_before_effect == Some(phase) {
            self.fail_phase_before_effect = None;
            self.calls.push("record-journal-phase-before-effect");
            return Err(BackendError::new(format!(
                "injected before journal phase {phase:?}"
            )));
        }
        if let Some(journal) = &mut self.journal {
            journal.record.phase = phase;
        } else {
            return Err(BackendError::new("journal is missing"));
        }
        let journal = self.journal.clone().expect("checked journal");
        self.power_snapshots.push((phase, self.snapshot(), journal));
        self.complete("record-journal-phase")
    }

    fn record_target_receipt(
        &mut self,
        fingerprint: InstallReceiptFingerprint,
    ) -> Result<(), BackendError> {
        if let Some(journal) = &mut self.journal {
            journal.record.target_receipt = Some(fingerprint);
        } else {
            return Err(BackendError::new("journal is missing"));
        }
        self.capture_effect("record-target-receipt-effect");
        self.complete("record-target-receipt")
    }

    fn finish_journal(&mut self) -> Result<(), BackendError> {
        self.complete("finish-journal")?;
        self.journal = None;
        Ok(())
    }

    fn verify_apply_artifacts(
        &mut self,
        path: &Path,
    ) -> Result<Self::VerifiedArtifacts, BackendError> {
        let verified = path.to_path_buf();
        self.complete("verify-apply-artifacts")?;
        Ok(verified)
    }

    fn read_screensaver_rule(&mut self) -> Result<Vec<u8>, BackendError> {
        self.policy_reads += 1;
        if self
            .replace_policy_on_read
            .as_ref()
            .is_some_and(|(read, _)| *read == self.policy_reads)
        {
            self.policy = self
                .replace_policy_on_read
                .take()
                .expect("checked replacement")
                .1;
        }
        let value = self.policy.clone();
        self.complete("read-screensaver-rule")?;
        if self
            .inject_policy_after_read_return
            .as_ref()
            .is_some_and(|(read, _)| *read == self.policy_reads)
        {
            self.policy = self
                .inject_policy_after_read_return
                .take()
                .expect("checked post-read injection")
                .1;
        }
        Ok(value)
    }

    fn set_screensaver_rule(&mut self, bytes: &[u8]) -> Result<(), BackendError> {
        self.policy = bytes.to_vec();
        self.capture_effect("set-screensaver-effect");
        self.complete("set-screensaver-rule")
    }

    fn read_named_rule(&mut self) -> Result<Option<Vec<u8>>, BackendError> {
        self.named_reads += 1;
        if self
            .replace_named_on_read
            .as_ref()
            .is_some_and(|(read, _)| *read == self.named_reads)
        {
            self.named_rule = self
                .replace_named_on_read
                .take()
                .expect("checked named replacement")
                .1;
        }
        let value = self.named_rule.clone();
        self.complete("read-named-rule")?;
        if self
            .inject_named_after_read_return
            .as_ref()
            .is_some_and(|(read, _)| *read == self.named_reads)
        {
            self.named_rule = self
                .inject_named_after_read_return
                .take()
                .expect("checked post-read named injection")
                .1;
        }
        Ok(value)
    }

    fn set_named_rule(&mut self, bytes: &[u8]) -> Result<(), BackendError> {
        self.named_rule = Some(bytes.to_vec());
        self.capture_effect("set-named-effect");
        self.complete("set-named-rule")
    }

    fn remove_named_rule(&mut self) -> Result<(), BackendError> {
        self.named_rule = None;
        self.capture_effect("remove-named-effect");
        self.complete("remove-named-rule")
    }

    fn persist_install_backup(&mut self, bytes: &[u8]) -> Result<(), BackendError> {
        self.backups.push(bytes.to_vec());
        self.complete("persist-install-backup")
    }

    fn begin_component_update(&mut self, _: &Self::VerifiedArtifacts) -> Result<(), BackendError> {
        self.component_mutation_while_policy_active = ScreenSaverPolicy::parse(&self.policy)
            .is_ok_and(|policy| policy.repose_candidate_index().is_some());
        self.component_snapshot = Some((
            self.components,
            self.loaded_service,
            self.receipt_generation,
        ));
        self.staged = self.components;
        self.capture_effect("begin-components-effect");
        self.complete("begin-component-update")
    }

    fn stage_component(
        &mut self,
        component: Component,
        _: &Self::VerifiedArtifacts,
    ) -> Result<(), BackendError> {
        self.staged[Self::component_index(component)] = 2;
        self.capture_effect(match component {
            Component::Plugin => "stage-plugin-effect",
            Component::Service => "stage-service-effect",
            Component::LaunchdPlist => "stage-launchd-plist-effect",
            Component::SocketDirectory => "stage-socket-directory-effect",
        });
        self.complete(match component {
            Component::Plugin => "stage-plugin",
            Component::Service => "stage-service",
            Component::LaunchdPlist => "stage-launchd-plist",
            Component::SocketDirectory => "stage-socket-directory",
        })
    }

    fn commit_component_update(&mut self) -> Result<(), BackendError> {
        self.components = self.staged;
        self.capture_effect("commit-components-effect");
        self.complete("commit-component-update")
    }

    fn verify_staged_components(&mut self) -> Result<(), BackendError> {
        if self.staged != [2; 4] {
            return Err(BackendError::new("staged component mismatch"));
        }
        self.complete("verify-staged-components")
    }

    fn verify_staged_deny_only_health(&mut self) -> Result<(), BackendError> {
        self.complete("verify-staged-deny-only-health")
    }

    fn activate_installed_service(&mut self) -> Result<(), BackendError> {
        self.loaded_service = self.components[Self::component_index(Component::Service)];
        self.capture_effect("activate-service-effect");
        self.complete("activate-installed-service")
    }

    fn verify_active_service_and_components(
        &mut self,
        expected: Option<InstallReceiptFingerprint>,
    ) -> Result<(), BackendError> {
        if let Some(expected) = expected
            && self.receipt_state() != InstallReceiptState::Trusted(expected)
        {
            return Err(BackendError::new(
                "active receipt differs from expected generation",
            ));
        }
        let [plugin, service, launchd, socket] = self.components;
        if plugin == 0
            || [plugin; 4] != [plugin, service, launchd, socket]
            || self.loaded_service != plugin
        {
            return Err(BackendError::new("active component identity mismatch"));
        }
        self.complete("verify-active-service-and-components")
    }

    fn persist_install_receipt(&mut self) -> Result<InstallReceiptFingerprint, BackendError> {
        let generation = self.components[Self::component_index(Component::Plugin)];
        if generation == 0 || self.components != [generation; 4] {
            return Err(BackendError::new(
                "cannot receipt an incomplete component generation",
            ));
        }
        self.receipt_generation = Some(generation);
        let InstallReceiptState::Trusted(fingerprint) = self.receipt_state() else {
            unreachable!("receipt generation was just installed")
        };
        self.capture_effect("persist-install-receipt-effect");
        self.complete("persist-install-receipt")?;
        Ok(fingerprint)
    }

    fn verify_install_receipt(&mut self) -> Result<InstallReceiptState, BackendError> {
        let state = match self.receipt_generation {
            None if self.components == [0; 4] => InstallReceiptState::Missing,
            None => {
                return Err(BackendError::new(
                    "unreceipted component occupies a fixed path",
                ));
            }
            Some(generation)
                if self
                    .components
                    .iter()
                    .all(|component| *component == 0 || *component == generation) =>
            {
                self.receipt_state()
            }
            Some(_) => {
                return Err(BackendError::new(
                    "component generation differs from receipt",
                ));
            }
        };
        self.complete("verify-install-receipt")?;
        Ok(state)
    }

    fn rollback_component_update(&mut self) -> Result<(), BackendError> {
        let durable_snapshot = self.journal.as_ref().map(|journal| {
            (
                journal.snapshot.components,
                journal.snapshot.loaded_service,
                journal.snapshot.receipt_generation,
            )
        });
        if let Some((components, loaded_service, receipt_generation)) =
            self.component_snapshot.take().or(durable_snapshot)
        {
            self.components = components;
            self.loaded_service = loaded_service;
            if !self.leave_new_receipt_on_rollback {
                self.receipt_generation = receipt_generation;
            }
        }
        self.staged = self.components;
        self.capture_effect("rollback-components-effect");
        self.complete("rollback-component-update")
    }

    fn finish_component_update(&mut self) -> Result<(), BackendError> {
        self.component_snapshot = None;
        self.staged = self.components;
        self.capture_effect("finish-components-effect");
        self.complete("finish-component-update")
    }

    fn component_state(&mut self) -> Result<ComponentState, BackendError> {
        let state = self
            .component_state_override
            .unwrap_or(match self.components {
                [0, 0, 0, 0] => ComponentState::Missing,
                [a, b, c, d]
                    if a != 0 && a == b && a == c && a == d && self.loaded_service == a =>
                {
                    ComponentState::HealthyDenyOnly
                }
                _ => ComponentState::Drifted,
            });
        self.complete("component-state")?;
        Ok(state)
    }

    fn service_removal_state(
        &mut self,
        expected: InstallReceiptFingerprint,
    ) -> Result<ServiceRemovalState, BackendError> {
        if self.receipt_state() != InstallReceiptState::Trusted(expected) {
            return Err(BackendError::new(
                "loaded service receipt differs from expected generation",
            ));
        }
        let state = if self.loaded_service == 0 {
            ServiceRemovalState::Stopped
        } else if self.receipt_generation == Some(self.loaded_service)
            && self.components[Self::component_index(Component::Service)] == self.loaded_service
        {
            ServiceRemovalState::RunningTrusted
        } else {
            return Err(BackendError::new(
                "loaded service is outside the receipted generation",
            ));
        };
        self.complete("service-removal-state")?;
        Ok(state)
    }

    fn remove_component(
        &mut self,
        component: Component,
        expected: InstallReceiptFingerprint,
    ) -> Result<(), BackendError> {
        if self
            .inject_installed_generation_before_remove
            .is_some_and(|(target, _)| target == component)
        {
            let generation = self
                .inject_installed_generation_before_remove
                .take()
                .expect("checked component-generation injection")
                .1;
            self.components = [generation; 4];
            self.loaded_service = generation;
            self.receipt_generation = Some(generation);
        }
        if self.receipt_state() != InstallReceiptState::Trusted(expected) {
            return Err(BackendError::new(
                "component receipt differs from expected generation",
            ));
        }
        let index = Self::component_index(component);
        if self.components[index] != 0 && self.receipt_generation != Some(self.components[index]) {
            return Err(BackendError::new(
                "refusing to remove component outside the receipt",
            ));
        }
        self.components[index] = 0;
        self.capture_effect(match component {
            Component::Plugin => "remove-plugin-effect",
            Component::Service => "remove-service-effect",
            Component::LaunchdPlist => "remove-launchd-plist-effect",
            Component::SocketDirectory => "remove-socket-directory-effect",
        });
        self.complete(match component {
            Component::Plugin => "remove-plugin",
            Component::Service => "remove-service",
            Component::LaunchdPlist => "remove-launchd-plist",
            Component::SocketDirectory => "remove-socket-directory",
        })
    }

    fn stop_service(&mut self, expected: InstallReceiptFingerprint) -> Result<(), BackendError> {
        if self.receipt_state() != InstallReceiptState::Trusted(expected)
            || self.loaded_service == 0
            || self.components[Self::component_index(Component::Service)] != self.loaded_service
        {
            return Err(BackendError::new(
                "service generation changed before atomic stop",
            ));
        }
        self.loaded_service = 0;
        self.capture_effect("stop-service-effect");
        self.complete("stop-service")
    }

    fn finish_component_removal(
        &mut self,
        expected: Option<InstallReceiptFingerprint>,
    ) -> Result<(), BackendError> {
        if self.components != [0; 4] {
            return Err(BackendError::new(
                "cannot remove receipt while components remain",
            ));
        }
        match (expected, self.receipt_state()) {
            (None, InstallReceiptState::Missing) | (Some(_), InstallReceiptState::Missing) => {}
            (Some(expected), InstallReceiptState::Trusted(actual)) if expected == actual => {}
            _ => {
                return Err(BackendError::new(
                    "install receipt changed before final removal",
                ));
            }
        }
        self.receipt_generation = None;
        self.capture_effect("remove-install-receipt-effect");
        self.complete("finish-component-removal")
    }

    fn read_validated_backup(&mut self, _: &Path) -> Result<Vec<u8>, BackendError> {
        self.complete("read-validated-backup")?;
        Ok(THIRD_PARTY_POLICY.to_vec())
    }
}

#[test]
fn every_install_step_failure_rolls_back_policy_named_rule_and_components() {
    let mut baseline = FakeBackend::stock();
    install(
        &mut baseline,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let install_step_count = baseline
        .calls
        .iter()
        .position(|call| *call == "release-exclusive-lock")
        .unwrap();

    for fail_at in 1..=install_step_count {
        let mut backend = FakeBackend::stock();
        let before = backend.snapshot();
        backend.fail_at = Some(fail_at);

        let error = install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package")),
        )
        .expect_err("the selected step must fail");

        assert!(
            error.to_string().contains("injected"),
            "failure {fail_at}: {error}"
        );
        let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
        assert!(policy.has_exactly_one_password_fallback());
        if policy.repose_candidate_index().is_some() {
            assert_eq!(backend.components, [2; 4], "failure after step {fail_at}");
            assert_eq!(backend.loaded_service, 2, "failure after step {fail_at}");
            assert!(backend.named_rule.as_deref().is_some_and(|bytes| {
                repose_unlockctl::install_transaction::verify_named_rule_v1(bytes).is_ok()
            }));
        } else {
            assert_snapshot_equivalent(
                &backend.snapshot(),
                &before,
                &format!("failure after step {fail_at}"),
            );
        }
        assert!(backend.calls.iter().all(|call| !call.contains("console")));
    }
}

#[test]
fn install_and_uninstall_are_idempotent_and_preserve_third_party_candidates() {
    let mut backend = FakeBackend::stock();
    let request = InstallRequest::new(PathBuf::from("/safe/package"));
    install(&mut backend, &request).unwrap();
    install(&mut backend, &request).unwrap();

    let installed = ScreenSaverPolicy::parse(&backend.policy).unwrap();
    installed.verify_installed(&PolicySpec::v1()).unwrap();
    let installed_xml = String::from_utf8(installed.to_xml_bytes().unwrap()).unwrap();
    assert!(installed_xml.contains("com.example.security-key"));

    uninstall(&mut backend).unwrap();
    uninstall(&mut backend).unwrap();
    let removed_xml = String::from_utf8(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .to_xml_bytes()
            .unwrap(),
    )
    .unwrap();
    assert!(removed_xml.contains("com.example.security-key"));
    assert!(!removed_xml.contains("ai.repose.unlock"));
    assert_eq!(backend.components, [0; 4]);
}

#[test]
fn any_policy_or_named_rule_removal_failure_keeps_all_components_installed() {
    let mut baseline = FakeBackend::installed();
    uninstall(&mut baseline).unwrap();
    let stop_position = baseline
        .calls
        .iter()
        .position(|call| *call == "stop-service")
        .unwrap();
    for failing_call in 1..=stop_position {
        let mut backend = FakeBackend::installed();
        backend.fail_at = Some(failing_call);
        assert!(uninstall(&mut backend).is_err());
        assert_eq!(
            backend.components, [1; 4],
            "components changed when uninstall call {failing_call} failed: {:?}",
            backend.calls
        );
    }
}

#[test]
fn status_reports_missing_healthy_and_policy_drift_without_mutating() {
    let mut missing = FakeBackend::stock();
    assert_eq!(inspect_status(&mut missing).unwrap(), Status::NotInstalled);
    assert!(missing.backups.is_empty());

    let mut installed = FakeBackend::installed();
    assert_eq!(
        inspect_status(&mut installed).unwrap(),
        Status::InstalledDenyOnly
    );

    installed.named_rule = None;
    assert_eq!(inspect_status(&mut installed).unwrap(), Status::Drifted);
}

#[test]
fn status_reports_every_pending_journal_phase_as_drifted_without_recovery() {
    for operation in [
        JournalOperation::Install,
        JournalOperation::Uninstall,
        JournalOperation::Repair,
    ] {
        for phase in [
            JournalPhase::Prepared,
            JournalPhase::PolicyInactive,
            JournalPhase::ComponentsCommittedHealthy,
            JournalPhase::NamedReady,
            JournalPhase::PolicyActive,
            JournalPhase::Committed,
            JournalPhase::ComponentsRemoved,
            JournalPhase::Aborted,
        ] {
            let mut backend = FakeBackend::installed();
            let snapshot = backend.snapshot();
            backend.journal = Some(FakeJournal {
                record: DurableJournal {
                    operation,
                    phase,
                    policy: snapshot.policy.clone(),
                    named_rule: snapshot.named_rule.clone(),
                    repair_base: Some(THIRD_PARTY_POLICY.to_vec()),
                    prior_receipt: backend.receipt_state(),
                    target_receipt: None,
                },
                snapshot: snapshot.clone(),
            });

            assert_eq!(inspect_status(&mut backend).unwrap(), Status::Drifted);
            assert_snapshot_equivalent(&backend.snapshot(), &snapshot, "read-only pending status");
            assert!(backend.journal.is_some());
        }
    }
}

#[test]
fn active_upgrade_failpoints_never_leave_a_mixed_active_dependency_closure() {
    let mut baseline = FakeBackend::installed();
    install(
        &mut baseline,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let step_count = baseline
        .calls
        .iter()
        .position(|call| *call == "release-exclusive-lock")
        .unwrap();

    for fail_at in 1..=step_count {
        let mut backend = FakeBackend::installed();
        backend.fail_at = Some(fail_at);
        let result = install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package")),
        );
        assert!(
            result.is_err(),
            "failpoint {fail_at} did not fire: {:?}",
            backend.calls
        );

        let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
        assert!(policy.has_exactly_one_password_fallback());
        if policy.repose_candidate_index().is_some() {
            let [plugin, service, launchd, socket] = backend.components;
            assert_ne!(plugin, 0, "failpoint {fail_at}");
            assert_eq!([plugin; 4], [plugin, service, launchd, socket]);
            assert_eq!(backend.loaded_service, plugin, "failpoint {fail_at}");
            assert!(backend.named_rule.as_deref().is_some_and(|bytes| {
                repose_unlockctl::install_transaction::verify_named_rule_v1(bytes).is_ok()
            }));
        }
    }
}

#[test]
fn drifted_old_runtime_is_deactivated_and_never_reenabled_by_upgrade() {
    let mut backend = FakeBackend::installed();
    backend.loaded_service = 99;
    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );
    let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
    assert!(policy.repose_candidate_index().is_none());
    assert!(policy.has_exactly_one_password_fallback());
}

#[test]
fn last_moment_live_policy_is_transformed_without_losing_new_third_party_rule() {
    let mut backend = FakeBackend::stock();
    let late = String::from_utf8(THIRD_PARTY_POLICY.to_vec())
        .unwrap()
        .replace(
            "<string>use-login-window-ui</string>",
            "<string>late.third-party</string>\n    <string>use-login-window-ui</string>",
        )
        .into_bytes();
    backend.replace_policy_on_read = Some((2, late));
    install(
        &mut backend,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let xml = String::from_utf8(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .to_xml_bytes()
            .unwrap(),
    )
    .unwrap();
    assert!(xml.contains("late.third-party"));
}

#[test]
fn observable_authorization_drift_at_readback_is_disabled_without_losing_foreign_fields() {
    let mut concurrent = Value::from_reader(std::io::Cursor::new(THIRD_PARTY_POLICY)).unwrap();
    concurrent.as_dictionary_mut().unwrap().insert(
        "concurrent-owner".to_owned(),
        Value::String("kept".to_owned()),
    );
    let mut concurrent_bytes = Vec::new();
    concurrent.to_writer_xml(&mut concurrent_bytes).unwrap();
    let concurrent_active = ScreenSaverPolicy::parse(&concurrent_bytes)
        .unwrap()
        .install(&PolicySpec::v1())
        .unwrap()
        .to_bytes()
        .unwrap();

    let mut backend = FakeBackend::stock();
    backend.inject_policy_after_call = Some(("set-screensaver-rule", concurrent_active));
    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );
    let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
    assert!(policy.repose_candidate_index().is_none());
    let xml = String::from_utf8(policy.to_xml_bytes().unwrap()).unwrap();
    assert!(xml.contains("concurrent-owner"));
}

#[test]
fn authorization_services_read_then_write_gap_is_explicitly_not_a_cas_guarantee() {
    let mut trace = FakeBackend::stock();
    install(
        &mut trace,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let final_transform_read = trace.policy_reads - 2;

    let mut concurrent = Value::from_reader(std::io::Cursor::new(THIRD_PARTY_POLICY)).unwrap();
    concurrent.as_dictionary_mut().unwrap().insert(
        "unobservable-between-read-and-write".to_owned(),
        Value::String("writer-lost".to_owned()),
    );
    let mut concurrent_bytes = Vec::new();
    concurrent.to_writer_xml(&mut concurrent_bytes).unwrap();

    let mut backend = FakeBackend::stock();
    backend.inject_policy_after_read_return = Some((final_transform_read, concurrent_bytes));
    install(
        &mut backend,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    assert!(
        backend.inject_policy_after_read_return.is_none(),
        "the fake must exercise the read-return/write gap"
    );
    let xml = String::from_utf8(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .to_xml_bytes()
            .unwrap(),
    )
    .unwrap();
    assert!(!xml.contains("unobservable-between-read-and-write"));
    assert!(
        repose_unlockctl::production::verify_production_mutation_gate().is_err(),
        "Task 7 must not expose this unavoidable platform race to production"
    );
}

#[test]
fn foreign_named_rule_observed_before_mutation_is_retained() {
    let mut backend = FakeBackend::installed();
    let foreign = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>class</key><string>allow</string></dict></plist>"#.to_vec();
    backend.named_rule = Some(foreign.clone());
    assert!(uninstall(&mut backend).is_err());
    assert_eq!(backend.named_rule, Some(foreign));
    assert_eq!(backend.components, [1; 4]);
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );
}

#[test]
fn named_rule_read_then_write_gap_is_explicitly_not_a_cas_guarantee() {
    let mut trace = FakeBackend::stock();
    install(
        &mut trace,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let named_write = trace
        .calls
        .iter()
        .position(|call| *call == "set-named-rule")
        .unwrap();
    let last_read_before_write = trace.calls[..named_write]
        .iter()
        .filter(|call| **call == "read-named-rule")
        .count();
    let foreign = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>class</key><string>allow</string></dict></plist>"#.to_vec();

    let mut backend = FakeBackend::stock();
    backend.inject_named_after_read_return = Some((last_read_before_write, Some(foreign.clone())));
    install(
        &mut backend,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();

    assert!(backend.inject_named_after_read_return.is_none());
    assert_ne!(backend.named_rule, Some(foreign));
    assert!(
        repose_unlockctl::production::verify_production_mutation_gate().is_err(),
        "the Task 7 gate must remain closed until an exclusive maintenance window exists"
    );
}

#[test]
fn duplicate_or_misplaced_policy_is_status_drift_not_healthy() {
    let mut backend = FakeBackend::installed();
    backend.policy = include_bytes!(
        "../../repose-authdb-policy/tests/fixtures/authorizationdb/duplicate-repose.plist"
    )
    .to_vec();
    assert_eq!(inspect_status(&mut backend).unwrap(), Status::Drifted);
}

#[test]
fn active_but_duplicate_or_misplaced_install_is_disabled_after_durable_backup() {
    for fixture in [
        include_bytes!(
            "../../repose-authdb-policy/tests/fixtures/authorizationdb/duplicate-repose.plist"
        )
        .as_slice(),
        include_bytes!(
            "../../repose-authdb-policy/tests/fixtures/authorizationdb/misplaced-repose.plist"
        )
        .as_slice(),
    ] {
        let mut backend = FakeBackend::installed();
        backend.policy = fixture.to_vec();

        assert!(
            install(
                &mut backend,
                &InstallRequest::new(PathBuf::from("/safe/package"))
            )
            .is_err()
        );
        let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
        assert!(policy.repose_candidate_index().is_none());
        assert!(policy.has_exactly_one_password_fallback());
        assert_eq!(backend.components, [1; 4]);
        assert_eq!(backend.loaded_service, 1);
        assert!(backend.calls.contains(&"start-journal"));
        assert!(backend.calls.contains(&"persist-install-backup"));
        assert!(!backend.calls.contains(&"begin-component-update"));
    }
}

#[test]
fn component_removal_failures_happen_only_after_policy_dependency_is_absent() {
    let mut baseline = FakeBackend::installed();
    uninstall(&mut baseline).unwrap();
    let destructive_positions: Vec<usize> = baseline
        .calls
        .iter()
        .enumerate()
        .filter(|(_, call)| {
            matches!(
                **call,
                "stop-service"
                    | "remove-launchd-plist"
                    | "remove-service"
                    | "remove-plugin"
                    | "remove-socket-directory"
            )
        })
        .map(|(index, _)| index + 1)
        .collect();
    for failing_call in destructive_positions {
        let mut backend = FakeBackend::installed();
        backend.fail_at = Some(failing_call);
        assert!(uninstall(&mut backend).is_err());
        let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
        assert!(
            policy.repose_candidate_index().is_none(),
            "calls: {:?}",
            backend.calls
        );
        assert!(policy.has_exactly_one_password_fallback());
        assert!(backend.named_rule.is_none());
    }
}

#[test]
fn repair_primary_and_policy_rollback_failure_retains_named_rule_and_components() {
    let mut backend = FakeBackend::installed();
    backend.fail_at = Some(10);
    backend.second_fail_at = Some(12);
    let error = repair_policy(&mut backend, Path::new("/safe/backup.plist")).unwrap_err();
    assert!(
        !error.rollback_failures().is_empty(),
        "calls: {:?}",
        backend.calls
    );
    assert_eq!(backend.components, [1; 4]);
    assert_eq!(backend.loaded_service, 1);
    assert!(backend.named_rule.is_some());
}

#[test]
fn active_repair_with_foreign_named_rule_disables_policy_before_refusing_conflict() {
    let foreign = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>class</key><string>allow</string></dict></plist>"#.to_vec();
    let mut backend = FakeBackend::installed();
    backend.named_rule = Some(foreign.clone());

    assert!(repair_policy(&mut backend, Path::new("/safe/backup.plist")).is_err());
    let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
    assert!(policy.repose_candidate_index().is_none());
    assert!(policy.has_exactly_one_password_fallback());
    assert_eq!(backend.named_rule, Some(foreign));
    assert_eq!(backend.components, [1; 4]);
    let deactivate = backend
        .calls
        .iter()
        .position(|call| *call == "set-screensaver-rule")
        .unwrap();
    assert!(backend.calls[..deactivate].contains(&"start-journal"));
    assert!(!backend.calls.contains(&"set-named-rule"));
}

#[test]
fn repair_abort_finish_failure_is_never_rolled_forward_by_a_fresh_process() {
    let mut trace = FakeBackend::installed();
    let named_write = {
        repair_policy(&mut trace, Path::new("/safe/backup.plist")).unwrap();
        trace
            .calls
            .iter()
            .position(|call| *call == "set-named-rule")
            .unwrap()
            + 1
    };
    let mut failed_trace = FakeBackend::installed();
    failed_trace.fail_at = Some(named_write);
    assert!(repair_policy(&mut failed_trace, Path::new("/safe/backup.plist")).is_err());
    let finish = failed_trace
        .calls
        .iter()
        .position(|call| *call == "finish-journal")
        .unwrap()
        + 1;

    let mut backend = FakeBackend::installed();
    backend.fail_at = Some(named_write);
    backend.second_fail_at = Some(finish);
    assert!(repair_policy(&mut backend, Path::new("/safe/backup.plist")).is_err());
    let journal = backend
        .journal
        .clone()
        .expect("failed clear must retain journal");
    assert_eq!(journal.record.phase, JournalPhase::Aborted);
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );

    backend.fail_at = None;
    backend.second_fail_at = None;
    backend.failed = false;
    backend.second_failed = false;
    recover_pending(&mut backend).unwrap();
    assert!(backend.journal.is_none());
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none(),
        "a safely aborted repair must never reactivate on fresh recovery"
    );
}

#[test]
fn repair_abort_marker_failure_is_conservatively_aborted_by_a_fresh_process() {
    let mut trace = FakeBackend::installed();
    repair_policy(&mut trace, Path::new("/safe/backup.plist")).unwrap();
    let named_write = trace
        .calls
        .iter()
        .position(|call| *call == "set-named-rule")
        .unwrap()
        + 1;

    let mut backend = FakeBackend::installed();
    backend.fail_at = Some(named_write);
    backend.fail_phase_before_effect = Some(JournalPhase::Aborted);
    assert!(repair_policy(&mut backend, Path::new("/safe/backup.plist")).is_err());
    assert_eq!(
        backend.journal.as_ref().unwrap().record.phase,
        JournalPhase::PolicyInactive,
        "the failed abort marker leaves an ambiguous pre-terminal phase"
    );

    backend.fail_at = None;
    backend.second_fail_at = None;
    backend.failed = false;
    backend.second_failed = false;
    recover_pending(&mut backend).unwrap();
    assert!(backend.journal.is_none());
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none(),
        "ambiguous repair recovery must prefer password-only over silent activation"
    );
}

#[test]
fn fresh_process_recovery_reconciles_every_durable_install_phase() {
    let mut source = FakeBackend::stock();
    install(
        &mut source,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    assert!(!source.power_snapshots.is_empty());

    for (phase, snapshot, journal) in source.power_snapshots.clone() {
        let mut recovered = FakeBackend::stock();
        recovered.policy = snapshot.policy;
        recovered.named_rule = snapshot.named_rule;
        recovered.components = snapshot.components;
        recovered.loaded_service = snapshot.loaded_service;
        recovered.receipt_generation = snapshot.receipt_generation;
        recovered.staged = recovered.components;
        recovered.journal = Some(journal);
        recover_pending(&mut recovered).unwrap();

        let policy = ScreenSaverPolicy::parse(&recovered.policy).unwrap();
        assert!(policy.has_exactly_one_password_fallback());
        if matches!(phase, JournalPhase::PolicyActive | JournalPhase::Committed) {
            assert_eq!(
                inspect_status(&mut recovered).unwrap(),
                Status::InstalledDenyOnly
            );
        } else {
            assert!(
                policy.repose_candidate_index().is_none(),
                "phase {phase:?} recovered active"
            );
            assert_eq!(recovered.components, [0; 4], "phase {phase:?}");
            assert_eq!(recovered.loaded_service, 0, "phase {phase:?}");
        }
        assert!(recovered.journal.is_none());
    }
}

#[test]
fn upgrade_named_rule_race_cannot_reactivate_old_policy() {
    let mut backend = FakeBackend::installed();
    let foreign = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>class</key><string>allow</string></dict></plist>"#.to_vec();
    backend.replace_named_on_read = Some((2, Some(foreign.clone())));

    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );
    assert_eq!(backend.named_rule, Some(foreign));
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );
    assert_eq!(backend.components, [1; 4]);
    assert_eq!(backend.loaded_service, 1);
}

#[test]
fn fresh_install_rechecks_inactivity_after_durable_preparation() {
    let mut backend = FakeBackend::stock();
    let active = FakeBackend::installed().policy;
    backend.inject_policy_after_call = Some(("persist-install-backup", active));

    let result = install(
        &mut backend,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    );

    assert!(
        result.is_ok(),
        "safe surgical deactivation may continue: {result:?}"
    );
    assert!(
        !backend.component_mutation_while_policy_active,
        "component mutation began before a fresh inactive-policy proof: {:?}",
        backend.calls
    );
}

#[test]
fn recovery_rolls_forward_when_policy_write_preceded_its_phase_marker() {
    let mut backend = FakeBackend::stock();
    let original = backend.snapshot();
    backend.journal = Some(FakeJournal {
        record: DurableJournal {
            operation: JournalOperation::Install,
            phase: JournalPhase::NamedReady,
            policy: original.policy.clone(),
            named_rule: original.named_rule.clone(),
            repair_base: None,
            prior_receipt: original.receipt_generation.map_or(
                InstallReceiptState::Missing,
                |generation| {
                    let mut fingerprint = [0_u8; 32];
                    fingerprint[..8].copy_from_slice(&generation.to_be_bytes());
                    InstallReceiptState::Trusted(InstallReceiptFingerprint::new(fingerprint))
                },
            ),
            target_receipt: Some(InstallReceiptFingerprint::new({
                let mut fingerprint = [0_u8; 32];
                fingerprint[..8].copy_from_slice(&2_u64.to_be_bytes());
                fingerprint
            })),
        },
        snapshot: original,
    });
    backend.components = [2; 4];
    backend.loaded_service = 2;
    backend.receipt_generation = Some(2);
    backend.staged = [2; 4];
    backend.named_rule = Some(repose_unlockctl::install_transaction::named_rule_v1());
    backend.policy = ScreenSaverPolicy::parse(THIRD_PARTY_POLICY)
        .unwrap()
        .install(&PolicySpec::v1())
        .unwrap()
        .to_bytes()
        .unwrap();

    recover_pending(&mut backend).unwrap();

    assert_eq!(
        inspect_status(&mut backend).unwrap(),
        Status::InstalledDenyOnly
    );
    assert!(backend.journal.is_none());
}

#[test]
fn recovery_never_overwrites_a_concurrent_foreign_named_rule() {
    let mut backend = FakeBackend::stock();
    let original = backend.snapshot();
    backend.journal = Some(FakeJournal {
        record: DurableJournal {
            operation: JournalOperation::Install,
            phase: JournalPhase::PolicyInactive,
            policy: original.policy.clone(),
            named_rule: original.named_rule.clone(),
            repair_base: None,
            prior_receipt: InstallReceiptState::Missing,
            target_receipt: None,
        },
        snapshot: original,
    });
    let foreign = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>class</key><string>allow</string></dict></plist>"#.to_vec();
    backend.named_rule = Some(foreign.clone());
    backend.components = [2; 4];
    backend.loaded_service = 2;

    assert!(recover_pending(&mut backend).is_err());

    assert_eq!(backend.named_rule, Some(foreign));
    assert_eq!(backend.components, [2; 4]);
    assert!(backend.journal.is_some());
}

#[test]
fn uninstall_rechecks_inactivity_after_stop_and_every_component_unlink() {
    for trigger in [
        "stop-service",
        "remove-launchd-plist",
        "remove-service",
        "remove-plugin",
        "remove-socket-directory",
    ] {
        let mut backend = FakeBackend::installed();
        backend.inject_policy_after_call = Some((trigger, FakeBackend::installed().policy));

        let result = uninstall(&mut backend);

        assert!(
            result.is_err(),
            "reactivation after {trigger} was not detected"
        );
        let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
        assert!(
            policy.repose_candidate_index().is_none(),
            "reactivation after {trigger} left an active missing dependency: {:?}",
            backend.calls
        );
        assert!(policy.has_exactly_one_password_fallback());
    }
}

#[test]
fn uninstall_never_overwrites_foreign_named_rule_during_reactivation_guard() {
    let mut backend = FakeBackend::installed();
    let foreign = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>class</key><string>allow</string></dict></plist>"#.to_vec();
    backend.inject_policy_after_call = Some(("stop-service", FakeBackend::installed().policy));
    backend.inject_named_after_call = Some(("stop-service", Some(foreign.clone())));

    assert!(uninstall(&mut backend).is_err());

    assert_eq!(backend.named_rule, Some(foreign));
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );
    assert_eq!(backend.components, [1; 4]);
}

#[test]
fn fresh_install_rollback_deactivates_a_concurrent_external_candidate() {
    let mut baseline = FakeBackend::stock();
    install(
        &mut baseline,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let failure = baseline
        .calls
        .iter()
        .position(|call| *call == "stage-service")
        .unwrap()
        + 1;

    let mut backend = FakeBackend::stock();
    backend.inject_policy_after_call = Some(("stage-plugin", FakeBackend::installed().policy));
    backend.fail_at = Some(failure);

    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );

    let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
    assert!(policy.repose_candidate_index().is_none());
    assert!(policy.has_exactly_one_password_fallback());
    assert_eq!(backend.components, [0; 4]);
    assert_eq!(backend.loaded_service, 0);
}

#[test]
fn rollback_does_not_rewrite_an_already_inactive_live_policy() {
    let mut baseline = FakeBackend::stock();
    install(
        &mut baseline,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let failure = baseline
        .calls
        .iter()
        .position(|call| *call == "verify-staged-components")
        .unwrap()
        + 1;
    let mut backend = FakeBackend::stock();
    backend.fail_at = Some(failure);

    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );

    assert_eq!(
        backend
            .calls
            .iter()
            .filter(|call| **call == "set-screensaver-rule")
            .count(),
        0,
        "an absent candidate needs no no-op AuthorizationRightSet"
    );
}

#[test]
fn a_new_process_recovers_every_install_effect_before_the_next_phase_fsync() {
    let mut source = FakeBackend::stock();
    install(
        &mut source,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    assert!(
        source
            .effect_snapshots
            .iter()
            .any(|cut| cut.label == "set-screensaver-effect"
                && cut.journal.record.phase == JournalPhase::NamedReady)
    );

    for cut in source.effect_snapshots.clone() {
        let mut recovered = FakeBackend::stock();
        recovered.policy = cut.state.policy;
        recovered.named_rule = cut.state.named_rule;
        recovered.components = cut.state.components;
        recovered.loaded_service = cut.state.loaded_service;
        recovered.receipt_generation = cut.state.receipt_generation;
        recovered.staged = cut.staged;
        recovered.component_snapshot = cut.component_snapshot;
        recovered.journal = Some(cut.journal);

        recover_pending(&mut recovered)
            .unwrap_or_else(|error| panic!("cut {} did not recover: {error}", cut.label));

        let policy = ScreenSaverPolicy::parse(&recovered.policy).unwrap();
        assert!(
            policy.has_exactly_one_password_fallback(),
            "cut {}",
            cut.label
        );
        if policy.repose_candidate_index().is_some() {
            assert_eq!(
                inspect_status(&mut recovered).unwrap(),
                Status::InstalledDenyOnly,
                "cut {}",
                cut.label
            );
        } else {
            assert_eq!(recovered.components, [0; 4], "cut {}", cut.label);
            assert_eq!(recovered.loaded_service, 0, "cut {}", cut.label);
        }
        assert!(recovered.journal.is_none(), "cut {}", cut.label);
    }
}

#[test]
fn a_new_process_rolls_uninstall_forward_after_stop_and_each_unlink_effect() {
    let mut source = FakeBackend::installed();
    uninstall(&mut source).unwrap();
    for expected in [
        "stop-service-effect",
        "remove-launchd-plist-effect",
        "remove-service-effect",
        "remove-plugin-effect",
        "remove-socket-directory-effect",
    ] {
        assert!(
            source
                .effect_snapshots
                .iter()
                .any(|cut| cut.label == expected)
        );
    }

    for cut in source.effect_snapshots.clone() {
        let mut recovered = FakeBackend::installed();
        recovered.policy = cut.state.policy;
        recovered.named_rule = cut.state.named_rule;
        recovered.components = cut.state.components;
        recovered.loaded_service = cut.state.loaded_service;
        recovered.receipt_generation = cut.state.receipt_generation;
        recovered.staged = cut.staged;
        recovered.component_snapshot = cut.component_snapshot;
        recovered.journal = Some(cut.journal);

        recover_pending(&mut recovered)
            .unwrap_or_else(|error| panic!("cut {} did not recover: {error}", cut.label));

        assert_eq!(recovered.components, [0; 4], "cut {}", cut.label);
        assert_eq!(recovered.loaded_service, 0, "cut {}", cut.label);
        assert!(recovered.named_rule.is_none(), "cut {}", cut.label);
        let policy = ScreenSaverPolicy::parse(&recovered.policy).unwrap();
        assert!(
            policy.repose_candidate_index().is_none(),
            "cut {}",
            cut.label
        );
        assert!(
            policy.has_exactly_one_password_fallback(),
            "cut {}",
            cut.label
        );
        assert!(recovered.journal.is_none(), "cut {}", cut.label);
    }
}

#[test]
fn fresh_process_recovers_every_active_upgrade_effect_without_mixing_generations() {
    let mut source = FakeBackend::installed();
    install(
        &mut source,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    assert!(
        source
            .effect_snapshots
            .iter()
            .any(|cut| cut.label == "persist-install-receipt-effect")
    );

    for cut in source.effect_snapshots.clone() {
        let mut recovered = FakeBackend::installed();
        recovered.policy = cut.state.policy;
        recovered.named_rule = cut.state.named_rule;
        recovered.components = cut.state.components;
        recovered.loaded_service = cut.state.loaded_service;
        recovered.receipt_generation = cut.state.receipt_generation;
        recovered.staged = cut.staged;
        recovered.component_snapshot = cut.component_snapshot;
        recovered.journal = Some(cut.journal);

        recover_pending(&mut recovered)
            .unwrap_or_else(|error| panic!("upgrade cut {} did not recover: {error}", cut.label));

        assert_eq!(
            inspect_status(&mut recovered).unwrap(),
            Status::InstalledDenyOnly,
            "upgrade cut {}",
            cut.label
        );
        let generation = recovered.receipt_generation.unwrap();
        assert_eq!(recovered.components, [generation; 4], "cut {}", cut.label);
        assert_eq!(recovered.loaded_service, generation, "cut {}", cut.label);
        assert!(recovered.journal.is_none(), "cut {}", cut.label);
    }
}

#[test]
fn fresh_process_recovers_every_repair_effect_with_policy_last() {
    let mut source = FakeBackend::installed();
    repair_policy(&mut source, Path::new("/safe/backup.plist")).unwrap();
    assert!(
        source
            .effect_snapshots
            .iter()
            .any(|cut| cut.label == "set-screensaver-effect")
    );

    for cut in source.effect_snapshots.clone() {
        let mut recovered = FakeBackend::installed();
        recovered.policy = cut.state.policy;
        recovered.named_rule = cut.state.named_rule;
        recovered.components = cut.state.components;
        recovered.loaded_service = cut.state.loaded_service;
        recovered.receipt_generation = cut.state.receipt_generation;
        recovered.staged = cut.staged;
        recovered.component_snapshot = cut.component_snapshot;
        recovered.journal = Some(cut.journal);

        recover_pending(&mut recovered)
            .unwrap_or_else(|error| panic!("repair cut {} did not recover: {error}", cut.label));

        let status = inspect_status(&mut recovered).unwrap();
        assert!(
            matches!(status, Status::InstalledDenyOnly | Status::Drifted),
            "repair cut {} returned {status:?}",
            cut.label
        );
        if status == Status::Drifted {
            let policy = ScreenSaverPolicy::parse(&recovered.policy).unwrap();
            assert!(
                policy.repose_candidate_index().is_none(),
                "ambiguous repair cut {} must recover password-only",
                cut.label
            );
            assert!(policy.has_exactly_one_password_fallback());
        }
        assert!(recovered.journal.is_none(), "cut {}", cut.label);
    }
}

#[test]
fn malformed_repair_prepared_journal_can_recover_in_a_new_process() {
    let malformed = b"not a plist".to_vec();
    let mut backend = FakeBackend::installed();
    let snapshot = Snapshot {
        policy: malformed.clone(),
        named_rule: backend.named_rule.clone(),
        components: backend.components,
        loaded_service: backend.loaded_service,
        receipt_generation: backend.receipt_generation,
    };
    backend.policy = malformed.clone();
    backend.journal = Some(FakeJournal {
        record: DurableJournal {
            operation: JournalOperation::Repair,
            phase: JournalPhase::Prepared,
            policy: malformed,
            named_rule: backend.named_rule.clone(),
            repair_base: Some(THIRD_PARTY_POLICY.to_vec()),
            prior_receipt: backend.receipt_state(),
            target_receipt: None,
        },
        snapshot,
    });

    recover_pending(&mut backend).unwrap();

    assert_eq!(inspect_status(&mut backend).unwrap(), Status::Drifted);
    let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
    assert!(policy.repose_candidate_index().is_none());
    assert!(policy.has_exactly_one_password_fallback());
    assert!(backend.journal.is_none());
}

#[test]
fn final_policy_activation_rechecks_the_loaded_service_identity() {
    let mut backend = FakeBackend::stock();
    backend.inject_loaded_after_call = Some(("set-screensaver-rule", 99));
    backend.component_state_override = Some(ComponentState::HealthyDenyOnly);

    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );

    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );
}

#[test]
fn named_rule_verification_rejects_duplicate_dictionary_keys() {
    let duplicate = br#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>class</key><string>evaluate-mechanisms</string>
<key>class</key><string>evaluate-mechanisms</string>
<key>mechanisms</key><array><string>ReposeUnlock:unlock,privileged</string></array>
</dict></plist>"#;
    assert!(
        repose_unlockctl::install_transaction::verify_named_rule_v1(duplicate).is_err(),
        "duplicate keys must be rejected before plist::Value collapses them"
    );
}

#[test]
fn install_persists_a_generation_receipt_before_named_or_policy_activation() {
    let mut backend = FakeBackend::stock();
    install(
        &mut backend,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();

    let receipt = backend
        .calls
        .iter()
        .position(|call| *call == "persist-install-receipt")
        .expect("successful install must persist a trusted generation receipt");
    let named = backend
        .calls
        .iter()
        .position(|call| *call == "set-named-rule")
        .unwrap();
    let target_marker = backend
        .calls
        .iter()
        .position(|call| *call == "record-target-receipt")
        .expect("target receipt must be durably bound into the journal");
    let policy = backend
        .calls
        .iter()
        .position(|call| *call == "set-screensaver-rule")
        .unwrap();
    assert!(receipt < target_marker && target_marker < named && named < policy);
}

#[test]
fn receipt_swap_before_persist_returns_never_becomes_the_install_target() {
    let mut backend = FakeBackend::stock();
    backend.inject_installed_generation_after_call = Some(("persist-install-receipt", 3));

    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package")),
        )
        .is_err(),
        "an arbitrary trusted generation must not replace the requested target"
    );
    let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
    assert!(policy.repose_candidate_index().is_none());
    assert!(policy.has_exactly_one_password_fallback());
}

#[test]
fn recovery_never_rolls_forward_an_unrecorded_or_different_target_generation() {
    for target_receipt in [
        None,
        Some(InstallReceiptFingerprint::new({
            let mut fingerprint = [0_u8; 32];
            fingerprint[..8].copy_from_slice(&2_u64.to_be_bytes());
            fingerprint
        })),
    ] {
        let mut backend = FakeBackend::stock();
        let original = backend.snapshot();
        backend.journal = Some(FakeJournal {
            record: DurableJournal {
                operation: JournalOperation::Install,
                phase: JournalPhase::NamedReady,
                policy: original.policy.clone(),
                named_rule: original.named_rule.clone(),
                repair_base: None,
                prior_receipt: InstallReceiptState::Missing,
                target_receipt,
            },
            snapshot: original,
        });
        backend.components = [3; 4];
        backend.loaded_service = 3;
        backend.receipt_generation = Some(3);
        backend.staged = [3; 4];
        backend.named_rule = Some(repose_unlockctl::install_transaction::named_rule_v1());
        backend.policy = FakeBackend::installed().policy;

        recover_pending(&mut backend).unwrap();

        assert_eq!(inspect_status(&mut backend).unwrap(), Status::NotInstalled);
        assert_eq!(backend.components, [0; 4]);
        assert_eq!(backend.loaded_service, 0);
        assert_eq!(backend.receipt_generation, None);
        assert!(backend.journal.is_none());
    }
}

#[test]
fn uninstall_refuses_unreceipted_fixed_path_occupants_before_policy_mutation() {
    let mut backend = FakeBackend::stock();
    backend.components = [99, 0, 0, 0];
    let before = backend.snapshot();

    assert!(uninstall(&mut backend).is_err());

    assert_snapshot_equivalent(&backend.snapshot(), &before, "foreign fixed-path occupant");
    assert!(!backend.calls.iter().any(|call| matches!(
        *call,
        "set-screensaver-rule" | "remove-named-rule" | "stop-service" | "remove-plugin"
    )));
}

#[test]
fn active_unreceipted_uninstall_disables_policy_but_retains_components() {
    let mut backend = FakeBackend::installed();
    backend.receipt_generation = None;

    assert!(uninstall(&mut backend).is_err());

    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );
    assert!(backend.named_rule.is_none());
    assert_eq!(backend.components, [1; 4]);
    assert_eq!(backend.loaded_service, 1);
    assert!(!backend.calls.iter().any(|call| matches!(
        *call,
        "stop-service"
            | "remove-launchd-plist"
            | "remove-service"
            | "remove-plugin"
            | "remove-socket-directory"
    )));
}

#[test]
fn active_unreceipted_upgrade_disables_policy_without_adopting_components() {
    let mut backend = FakeBackend::installed();
    backend.receipt_generation = None;

    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );

    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );
    assert_eq!(backend.components, [1; 4]);
    assert_eq!(backend.receipt_generation, None);
    assert!(!backend.calls.contains(&"begin-component-update"));
}

#[test]
fn uninstall_never_stops_a_loaded_service_outside_the_receipted_generation() {
    let mut backend = FakeBackend::installed();
    backend.loaded_service = 99;
    backend.component_state_override = Some(ComponentState::HealthyDenyOnly);

    assert!(uninstall(&mut backend).is_err());

    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );
    assert!(backend.named_rule.is_none());
    assert_eq!(backend.components, [1; 4]);
    assert_eq!(backend.loaded_service, 99);
    assert!(!backend.calls.contains(&"stop-service"));
}

#[test]
fn uninstall_never_stops_or_deletes_a_new_trusted_generation_after_journaling() {
    let mut backend = FakeBackend::installed();
    backend.inject_installed_generation_after_call = Some(("remove-named-rule", 2));

    assert!(uninstall(&mut backend).is_err());
    assert_eq!(backend.components, [2; 4]);
    assert_eq!(backend.loaded_service, 2);
    assert_eq!(backend.receipt_generation, Some(2));
    assert!(!backend.calls.contains(&"stop-service"));
    assert!(
        !backend
            .calls
            .iter()
            .any(|call| call.starts_with("remove-") && *call != "remove-named-rule")
    );
    assert!(backend.journal.is_some());

    let prior_calls = backend.calls.len();
    assert!(recover_pending(&mut backend).is_err());
    assert!(!backend.calls[prior_calls..].contains(&"stop-service"));
    assert_eq!(backend.components, [2; 4]);
}

#[test]
fn stale_receipt_readback_cannot_authorize_stopping_a_new_generation() {
    let mut backend = FakeBackend::installed();
    backend.inject_installed_generation_after_call = Some(("verify-install-receipt", 2));

    assert!(uninstall(&mut backend).is_err());
    assert_eq!(backend.components, [2; 4]);
    assert_eq!(backend.loaded_service, 2);
    assert_eq!(backend.receipt_generation, Some(2));
    assert!(
        !backend.calls.contains(&"stop-service"),
        "a stale precheck authorized bootout: {:?}",
        backend.calls
    );
    assert!(backend.journal.is_some());
}

#[test]
fn generation_swap_between_service_state_and_stop_cannot_authorize_bootout() {
    let mut backend = FakeBackend::installed();
    backend.inject_installed_generation_after_call = Some(("service-removal-state", 2));

    assert!(uninstall(&mut backend).is_err());
    assert_eq!(backend.components, [2; 4]);
    assert_eq!(backend.loaded_service, 2);
    assert_eq!(backend.receipt_generation, Some(2));
    assert!(
        !backend.calls.contains(&"stop-service"),
        "the stop operation did not revalidate its expected generation"
    );
    assert!(backend.journal.is_some());
}

#[test]
fn generation_swap_at_remove_entry_cannot_authorize_any_unlink() {
    let mut backend = FakeBackend::installed();
    backend.inject_installed_generation_before_remove = Some((Component::LaunchdPlist, 2));

    assert!(uninstall(&mut backend).is_err());
    assert_eq!(
        backend.components, [2; 4],
        "the expected generation must be checked inside the remove operation"
    );
    assert_eq!(backend.loaded_service, 2);
    assert_eq!(backend.receipt_generation, Some(2));
    assert!(backend.journal.is_some());
}

#[test]
fn install_reproves_policy_inactive_around_component_commit_and_activation() {
    for (trigger, next_dependency_effect) in [
        ("verify-staged-deny-only-health", "commit-component-update"),
        ("commit-component-update", "activate-installed-service"),
        ("activate-installed-service", "persist-install-receipt"),
    ] {
        let mut backend = FakeBackend::stock();
        backend.inject_policy_after_call = Some((trigger, FakeBackend::installed().policy));

        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package")),
        )
        .unwrap_or_else(|error| panic!("guard after {trigger} failed safely: {error}"));

        let trigger_index = backend
            .calls
            .iter()
            .position(|call| *call == trigger)
            .unwrap();
        let dependency_index = backend
            .calls
            .iter()
            .enumerate()
            .skip(trigger_index + 1)
            .find(|(_, call)| **call == next_dependency_effect)
            .map(|(index, _)| index)
            .unwrap();
        assert!(
            backend.calls[trigger_index + 1..dependency_index].contains(&"set-screensaver-rule"),
            "no surgical deactivation between {trigger} and {next_dependency_effect}: {:?}",
            backend.calls
        );
    }
}

#[test]
fn repair_proves_policy_inactive_before_touching_named_or_untrusted_runtime() {
    let mut backend = FakeBackend::installed();
    backend.named_rule = None;
    backend.loaded_service = 99;
    backend.component_state_override = Some(ComponentState::HealthyDenyOnly);

    assert!(repair_policy(&mut backend, Path::new("/safe/backup.plist")).is_err());

    let policy_write = backend
        .calls
        .iter()
        .position(|call| *call == "set-screensaver-rule")
        .expect("repair must durably disable the candidate");
    assert!(
        backend
            .calls
            .iter()
            .position(|call| *call == "set-named-rule")
            .is_none_or(|named_write| policy_write < named_write),
        "named mutation preceded policy deactivation: {:?}",
        backend.calls
    );
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );
}

#[test]
fn ambiguous_old_policy_restore_is_surgically_deactivated_before_journal_finish() {
    let mut successful = FakeBackend::installed();
    install(
        &mut successful,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let primary_position = successful
        .calls
        .iter()
        .position(|call| *call == "stage-service")
        .unwrap()
        + 1;

    let mut trace = FakeBackend::installed();
    trace.fail_at = Some(primary_position);
    assert!(
        install(
            &mut trace,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );
    let ambiguous_restore_position = trace
        .calls
        .iter()
        .enumerate()
        .filter(|(_, call)| **call == "set-screensaver-rule")
        .map(|(index, _)| index + 1)
        .next_back()
        .expect("upgrade rollback must attempt to restore the old candidate");

    let mut backend = FakeBackend::installed();
    backend.fail_at = Some(primary_position);
    backend.second_fail_at = Some(ambiguous_restore_position);
    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );

    let policy = ScreenSaverPolicy::parse(&backend.policy).unwrap();
    assert!(
        policy.repose_candidate_index().is_none(),
        "ambiguous restore left an active path: {:?}",
        backend.calls
    );
    assert!(policy.has_exactly_one_password_fallback());
}

#[test]
fn upgrade_never_reactivates_when_rollback_restores_files_but_not_prior_receipt() {
    let mut successful = FakeBackend::installed();
    install(
        &mut successful,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let failure_position = successful
        .calls
        .iter()
        .position(|call| *call == "set-named-rule")
        .unwrap()
        + 1;

    let mut backend = FakeBackend::installed();
    backend.leave_new_receipt_on_rollback = true;
    backend.fail_at = Some(failure_position);
    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );

    assert_eq!(backend.components, [1; 4]);
    assert_eq!(backend.receipt_generation, Some(2));
    assert!(backend.journal.is_some());
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );
}

#[test]
fn upgrade_rollback_restores_candidate_on_latest_policy_without_losing_foreign_fields() {
    let mut successful = FakeBackend::installed();
    install(
        &mut successful,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let failure_position = successful
        .calls
        .iter()
        .position(|call| *call == "stage-service")
        .unwrap()
        + 1;
    let mut concurrent = Value::from_reader(std::io::Cursor::new(THIRD_PARTY_POLICY)).unwrap();
    concurrent.as_dictionary_mut().unwrap().insert(
        "concurrent-owner".to_owned(),
        Value::String("kept".to_owned()),
    );
    let mut concurrent_bytes = Vec::new();
    concurrent.to_writer_xml(&mut concurrent_bytes).unwrap();

    let mut backend = FakeBackend::installed();
    backend.fail_at = Some(failure_position);
    backend.inject_policy_after_call = Some(("rollback-component-update", concurrent_bytes));
    assert!(
        install(
            &mut backend,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );

    let restored = Value::from_reader(std::io::Cursor::new(&backend.policy)).unwrap();
    assert_eq!(
        restored
            .as_dictionary()
            .unwrap()
            .get("concurrent-owner")
            .and_then(Value::as_string),
        Some("kept")
    );
    ScreenSaverPolicy::parse(&backend.policy)
        .unwrap()
        .verify_installed(&PolicySpec::v1())
        .unwrap();
}

#[test]
fn successfully_aborted_repair_clears_journal_and_never_reactivates_later() {
    let mut successful = FakeBackend::installed();
    repair_policy(&mut successful, Path::new("/safe/backup.plist")).unwrap();
    let failure_position = successful
        .calls
        .iter()
        .position(|call| *call == "set-named-rule")
        .unwrap()
        + 1;

    let mut backend = FakeBackend::installed();
    backend.fail_at = Some(failure_position);
    assert!(repair_policy(&mut backend, Path::new("/safe/backup.plist")).is_err());
    assert!(
        backend.journal.is_none(),
        "safe repair abort left a journal"
    );
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none()
    );

    recover_pending(&mut backend).unwrap();
    assert!(
        ScreenSaverPolicy::parse(&backend.policy)
            .unwrap()
            .repose_candidate_index()
            .is_none(),
        "a later recovery reactivated an already aborted repair"
    );
}

#[test]
fn failed_component_rollback_keeps_journal_and_is_retried_by_recovery() {
    let mut successful = FakeBackend::stock();
    install(
        &mut successful,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap();
    let primary_position = successful
        .calls
        .iter()
        .position(|call| *call == "stage-service")
        .unwrap()
        + 1;
    let mut trace = FakeBackend::stock();
    trace.fail_at = Some(primary_position);
    assert!(
        install(
            &mut trace,
            &InstallRequest::new(PathBuf::from("/safe/package"))
        )
        .is_err()
    );
    let rollback_position = trace
        .calls
        .iter()
        .position(|call| *call == "rollback-component-update")
        .unwrap()
        + 1;

    let mut backend = FakeBackend::stock();
    backend.fail_at = Some(primary_position);
    backend.second_fail_at = Some(rollback_position);
    let error = install(
        &mut backend,
        &InstallRequest::new(PathBuf::from("/safe/package")),
    )
    .unwrap_err();
    assert!(!error.rollback_failures().is_empty());
    assert!(backend.journal.is_some());

    recover_pending(&mut backend).unwrap();
    assert_eq!(backend.components, [0; 4]);
    assert_eq!(backend.receipt_generation, None);
    assert!(backend.journal.is_none());
}
