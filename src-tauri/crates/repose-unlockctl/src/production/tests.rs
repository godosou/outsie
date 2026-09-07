use super::*;
use std::os::unix::fs::symlink;

fn temporary_directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "repose-production-{label}-{}-{}",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn make_plugin_tree(parent: &Path) {
    let contents = parent.join("ReposeUnlock.bundle/Contents");
    fs::create_dir(contents.parent().unwrap()).unwrap();
    fs::create_dir(&contents).unwrap();
    fs::create_dir(contents.join("MacOS")).unwrap();
    fs::create_dir(contents.join("_CodeSignature")).unwrap();
    fs::write(contents.join("Info.plist"), b"plist").unwrap();
    fs::write(contents.join("MacOS/ReposeUnlock"), b"binary").unwrap();
    fs::write(contents.join("_CodeSignature/CodeResources"), b"signature").unwrap();
    fs::set_permissions(
        contents.join("Info.plist"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    fs::set_permissions(
        contents.join("MacOS/ReposeUnlock"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::set_permissions(
        contents.join("_CodeSignature/CodeResources"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    for directory in [
        contents.join("MacOS"),
        contents.join("_CodeSignature"),
        contents,
        parent.join("ReposeUnlock.bundle"),
    ] {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn receipt_bytes_for(
    component: Component,
    selected: ComponentReceipt,
) -> (InstallReceipt, Vec<u8>) {
    let components = Component::INSTALL_ORDER.map(|candidate| {
        if candidate == component {
            selected.clone()
        } else {
            ComponentReceipt {
                identity: FileIdentity {
                    device: 100 + component_index(candidate) as u64,
                    inode: 200 + component_index(candidate) as u64,
                },
                kind: component_kind(candidate).to_owned(),
                uid: effective_uid(),
                gid: selected.gid,
                mode: component_mode(candidate),
                links: 1,
                flags: 0,
                digest: "00".repeat(32),
                signing_requirement: "test-pinned-requirement".to_owned(),
                security_policy: InstallReceipt::SECURITY_POLICY.to_owned(),
            }
        }
    });
    let receipt = InstallReceipt {
        generation: "test-generation-1".to_owned(),
        package_version: "0.1.0".to_owned(),
        minimum_os: "15.0".to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        team_id: "TESTTEAM01".to_owned(),
        components,
    };
    let bytes = receipt.encode().unwrap();
    (receipt, bytes)
}

#[test]
fn backup_is_one_checksum_bound_container() {
    let policy = b"policy bytes";
    let encoded = BackupRecord::new(policy).encode().unwrap();
    let decoded = BackupRecord::decode(&encoded).unwrap();
    assert_eq!(decoded.policy, policy);

    let mut value = Value::from_reader(std::io::Cursor::new(encoded)).unwrap();
    value
        .as_dictionary_mut()
        .unwrap()
        .insert("policy".to_owned(), Value::Data(b"changed".to_vec()));
    let mut tampered = Vec::new();
    value.to_writer_binary(&mut tampered).unwrap();
    assert!(BackupRecord::decode(&tampered).is_err());
}

#[test]
fn journal_round_trip_carries_validated_repair_base() {
    let record = JournalRecord {
        operation: JournalOperation::Repair,
        phase: JournalPhase::Prepared,
        policy: b"malformed live".to_vec(),
        named_rule: None,
        repair_base: Some(b"safe backup".to_vec()),
        prior_receipt: InstallReceiptState::Missing,
        target_receipt: None,
        identities: [None; 4],
    };
    let decoded = JournalRecord::decode(&record.encode().unwrap()).unwrap();
    assert_eq!(decoded.repair_base, Some(b"safe backup".to_vec()));
    assert_eq!(decoded.durable_view().operation, JournalOperation::Repair);
}

#[test]
fn journal_round_trip_binds_the_exact_install_target_receipt() {
    let target = InstallReceiptFingerprint::new([9; 32]);
    let record = JournalRecord {
        operation: JournalOperation::Install,
        phase: JournalPhase::ComponentsCommittedHealthy,
        policy: b"policy".to_vec(),
        named_rule: None,
        repair_base: None,
        prior_receipt: InstallReceiptState::Missing,
        target_receipt: Some(target),
        identities: [None; 4],
    };

    let decoded = JournalRecord::decode(&record.encode().unwrap()).unwrap();
    assert_eq!(decoded.target_receipt, Some(target));
    assert_eq!(decoded.durable_view().target_receipt, Some(target));
}

#[test]
fn missing_receipt_never_authorizes_a_foreign_fixed_path_file() {
    let root = temporary_directory("foreign-unreceipted");
    let foreign = root.join("ai.repose.unlockd");
    fs::write(&foreign, b"foreign sentinel").unwrap();
    fs::set_permissions(&foreign, fs::Permissions::from_mode(0o755)).unwrap();
    let record = JournalRecord {
        operation: JournalOperation::Uninstall,
        phase: JournalPhase::NamedReady,
        policy: Vec::new(),
        named_rule: None,
        repair_base: None,
        prior_receipt: InstallReceiptState::Missing,
        target_receipt: None,
        identities: [None; 4],
    };

    assert!(
        verify_bound_component_at(
            &record,
            None,
            Component::Service,
            &foreign,
            effective_uid(),
            fs::metadata(&root).unwrap().gid(),
        )
        .is_err()
    );
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign sentinel");

    fs::remove_file(&foreign).unwrap();
    fs::remove_dir(&root).unwrap();
}

#[test]
fn quarantine_validates_the_receipt_before_deleting_any_plugin_file() {
    let parent = temporary_directory("plugin-quarantine-extra");
    make_plugin_tree(&parent);
    let bundle = parent.join("ReposeUnlock.bundle");
    let uid = effective_uid();
    let gid = fs::metadata(&parent).unwrap().gid();
    let selected = capture_component_receipt(&bundle, Component::Plugin, uid, gid).unwrap();
    let (receipt, bytes) = receipt_bytes_for(Component::Plugin, selected);
    fs::write(bundle.join("Contents/foreign-sentinel"), b"must survive").unwrap();
    let fingerprint: [u8; 32] = Sha256::digest(&bytes).into();
    let journal = JournalRecord {
        operation: JournalOperation::Uninstall,
        phase: JournalPhase::NamedReady,
        policy: Vec::new(),
        named_rule: None,
        repair_base: None,
        prior_receipt: InstallReceiptState::Trusted(InstallReceiptFingerprint::new(fingerprint)),
        target_receipt: None,
        identities: receipt.identities(),
    };

    assert!(
        quarantine_verify_and_remove(&journal, Some(&bytes), Component::Plugin, &bundle, uid, gid,)
            .is_err()
    );
    assert_eq!(
        fs::read(bundle.join("Contents/foreign-sentinel")).unwrap(),
        b"must survive"
    );
    assert_eq!(
        fs::read(bundle.join("Contents/MacOS/ReposeUnlock")).unwrap(),
        b"binary"
    );
    assert!(!quarantine_path(&bundle).unwrap().exists());

    fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn receipted_file_quarantine_is_idempotent_before_and_after_rename() {
    for resume_after_rename in [false, true] {
        let parent = temporary_directory("service-quarantine");
        let service = parent.join("ai.repose.unlockd");
        fs::write(&service, b"receipted service").unwrap();
        fs::set_permissions(&service, fs::Permissions::from_mode(0o755)).unwrap();
        let uid = effective_uid();
        let gid = fs::metadata(&parent).unwrap().gid();
        let selected = capture_component_receipt(&service, Component::Service, uid, gid).unwrap();
        let (receipt, bytes) = receipt_bytes_for(Component::Service, selected);
        let fingerprint: [u8; 32] = Sha256::digest(&bytes).into();
        let journal = JournalRecord {
            operation: JournalOperation::Uninstall,
            phase: JournalPhase::NamedReady,
            policy: Vec::new(),
            named_rule: None,
            repair_base: None,
            prior_receipt: InstallReceiptState::Trusted(InstallReceiptFingerprint::new(
                fingerprint,
            )),
            target_receipt: None,
            identities: receipt.identities(),
        };
        let quarantine = quarantine_path(&service).unwrap();
        if resume_after_rename {
            fs::rename(&service, &quarantine).unwrap();
        }

        quarantine_verify_and_remove(
            &journal,
            Some(&bytes),
            Component::Service,
            &service,
            uid,
            gid,
        )
        .unwrap();

        assert!(!service.exists());
        assert!(!quarantine.exists());
        fs::remove_dir(parent).unwrap();
    }
}

#[test]
fn unique_backup_writer_never_replaces_historical_bytes() {
    let root = temporary_directory("unique-backup");
    let metadata = fs::metadata(&root).unwrap();
    let first = write_unique_owned_file(
        &root,
        "install-backup",
        b"historical",
        effective_uid(),
        metadata.gid(),
    )
    .unwrap();
    let second = write_unique_owned_file(
        &root,
        "install-backup",
        b"current",
        effective_uid(),
        metadata.gid(),
    )
    .unwrap();

    assert_ne!(first, second);
    assert_eq!(fs::read(&first).unwrap(), b"historical");
    assert_eq!(fs::read(&second).unwrap(), b"current");

    fs::remove_file(first).unwrap();
    fs::remove_file(second).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn atomic_owned_writer_replaces_only_a_safe_regular_file() {
    let root = temporary_directory("atomic-owned-writer");
    let metadata = fs::metadata(&root).unwrap();
    let uid = effective_uid();
    let gid = metadata.gid();
    let state = root.join("journal.plist");

    write_atomic_owned_file(&state, b"prepared", uid, gid).unwrap();
    write_atomic_owned_file(&state, b"policy-inactive", uid, gid).unwrap();
    assert_eq!(fs::read(&state).unwrap(), b"policy-inactive");

    fs::remove_file(&state).unwrap();
    let outside = root.join("outside");
    fs::write(&outside, b"foreign").unwrap();
    symlink(&outside, &state).unwrap();
    assert!(write_atomic_owned_file(&state, b"must-not-land", uid, gid).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"foreign");

    fs::remove_file(&state).unwrap();
    fs::write(&state, b"wrong-mode").unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(write_atomic_owned_file(&state, b"must-not-land", uid, gid).is_err());
    assert_eq!(fs::read(&state).unwrap(), b"wrong-mode");

    fs::remove_file(&state).unwrap();
    fs::remove_file(&outside).unwrap();
    fs::remove_dir(&root).unwrap();
}

#[test]
fn owned_reader_rejects_symlink_hardlink_and_wrong_mode() {
    let root = temporary_directory("owned-reader");
    let metadata = fs::metadata(&root).unwrap();
    let uid = effective_uid();
    let gid = metadata.gid();
    let state = root.join("backup.plist");
    write_new_owned_file(&state, b"trusted", uid, gid).unwrap();
    assert_eq!(
        read_optional_owned_file(&state, 32, uid, gid).unwrap(),
        Some(b"trusted".to_vec())
    );

    let hardlink = root.join("backup-hardlink.plist");
    fs::hard_link(&state, &hardlink).unwrap();
    assert!(read_optional_owned_file(&state, 32, uid, gid).is_err());
    fs::remove_file(&hardlink).unwrap();
    fs::remove_file(&state).unwrap();

    let outside = root.join("outside");
    fs::write(&outside, b"foreign").unwrap();
    symlink(&outside, &state).unwrap();
    assert!(read_optional_owned_file(&state, 32, uid, gid).is_err());
    fs::remove_file(&state).unwrap();

    fs::write(&state, b"wrong-mode").unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(read_optional_owned_file(&state, 32, uid, gid).is_err());

    fs::remove_file(&state).unwrap();
    fs::remove_file(&outside).unwrap();
    fs::remove_dir(&root).unwrap();
}

#[test]
fn owned_lock_is_nofollow_mode_checked_and_exclusive() {
    let root = temporary_directory("owned-lock");
    let metadata = fs::metadata(&root).unwrap();
    let uid = effective_uid();
    let gid = metadata.gid();
    let lock_path = root.join("transaction.lock");

    let lock = acquire_owned_lock(&lock_path, uid, gid).unwrap();
    assert!(acquire_owned_lock(&lock_path, uid, gid).is_err());
    drop(lock);
    drop(acquire_owned_lock(&lock_path, uid, gid).unwrap());

    fs::remove_file(&lock_path).unwrap();
    let outside = root.join("outside");
    fs::write(&outside, b"foreign").unwrap();
    symlink(&outside, &lock_path).unwrap();
    assert!(acquire_owned_lock(&lock_path, uid, gid).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"foreign");

    fs::remove_file(&lock_path).unwrap();
    fs::write(&lock_path, b"").unwrap();
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(acquire_owned_lock(&lock_path, uid, gid).is_err());

    fs::remove_file(&lock_path).unwrap();
    fs::remove_file(&outside).unwrap();
    fs::remove_dir(&root).unwrap();
}

#[test]
fn bootout_classifier_accepts_only_authoritative_success_and_no_socket() {
    assert!(classify_bootout_result(true, false).is_ok());
    assert!(classify_bootout_result(false, false).is_err());
    assert!(classify_bootout_result(true, true).is_err());
    assert!(classify_bootout_result(false, true).is_err());
}

#[test]
fn concrete_backend_mutators_all_fail_at_the_inner_gate_before_io() {
    fn assert_gate(result: Result<(), BackendError>) {
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("production apply gate is closed")
        );
    }

    let mut backend = ProductionBackend::new();
    assert_gate(backend.acquire_exclusive_lock());
    assert_gate(backend.start_journal(JournalOperation::Install, b"ignored", None, None));
    assert_gate(backend.record_journal_phase(JournalPhase::Prepared));
    assert_gate(backend.record_target_receipt(InstallReceiptFingerprint::new([7; 32])));
    assert_gate(backend.finish_journal());
    assert_gate(backend.set_screensaver_rule(b"ignored"));
    assert_gate(backend.set_named_rule(b"ignored"));
    assert_gate(backend.remove_named_rule());
    assert_gate(backend.persist_install_backup(b"ignored"));
    assert_gate(backend.commit_component_update());
    assert_gate(backend.activate_installed_service());
    assert_gate(backend.verify_staged_components());
    assert_gate(backend.verify_staged_deny_only_health());
    assert_gate(backend.verify_active_service_and_components(None));
    assert_gate(backend.persist_install_receipt().map(|_| ()));
    assert_gate(backend.rollback_component_update());
    assert_gate(backend.finish_component_update());
    let expected = InstallReceiptFingerprint::new([7; 32]);
    assert_gate(backend.service_removal_state(expected).map(|_| ()));
    assert_gate(backend.stop_service(expected));
    assert_gate(backend.remove_component(Component::Plugin, expected));
    assert_gate(backend.finish_component_removal(Some(expected)));
}

#[test]
fn plugin_removal_is_fd_relative_and_never_follows_a_child_symlink() {
    let parent = temporary_directory("plugin-symlink");
    let outside = temporary_directory("outside");
    fs::write(outside.join("ReposeUnlock"), b"must survive").unwrap();
    let bundle = parent.join("ReposeUnlock.bundle");
    fs::create_dir(&bundle).unwrap();
    fs::create_dir(bundle.join("Contents")).unwrap();
    fs::write(bundle.join("Contents/Info.plist"), b"plist").unwrap();
    symlink(&outside, bundle.join("Contents/MacOS")).unwrap();
    fs::create_dir(bundle.join("Contents/_CodeSignature")).unwrap();
    fs::write(
        bundle.join("Contents/_CodeSignature/CodeResources"),
        b"signature",
    )
    .unwrap();

    let parent_handle = open_directory_nofollow(&parent).unwrap();
    assert!(remove_plugin_tree_at(&parent_handle, effective_uid()).is_err());
    assert_eq!(
        fs::read(outside.join("ReposeUnlock")).unwrap(),
        b"must survive"
    );

    fs::remove_dir_all(&parent).unwrap();
    fs::remove_dir_all(&outside).unwrap();
}

#[test]
fn plugin_removal_deletes_only_the_fixed_exact_tree() {
    let parent = temporary_directory("plugin-exact");
    make_plugin_tree(&parent);
    let parent_handle = open_directory_nofollow(&parent).unwrap();

    remove_plugin_tree_at(&parent_handle, effective_uid()).unwrap();

    assert!(!parent.join("ReposeUnlock.bundle").exists());
    fs::remove_dir(&parent).unwrap();
}

#[test]
fn fixed_process_runner_kills_a_hung_child_within_its_bound() {
    let started = std::time::Instant::now();
    let error =
        fixed_command_with_timeout("/bin/sleep", &["2"], std::time::Duration::from_millis(20))
            .unwrap_err();
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}
