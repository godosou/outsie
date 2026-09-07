use super::*;

pub fn repair_policy<B: InstallBackend>(
    backend: &mut B,
    backup_path: &Path,
) -> Result<(), TransactionError> {
    backend
        .acquire_exclusive_lock()
        .map_err(TransactionError::primary)?;
    let result = backend
        .load_journal()
        .map_err(TransactionError::primary)
        .and_then(|journal| recover_loaded_journal(backend, journal))
        .and_then(|()| repair_policy_locked(backend, backup_path));
    backend.release_exclusive_lock();
    result
}

fn repair_policy_locked<B: InstallBackend>(
    backend: &mut B,
    backup_path: &Path,
) -> Result<(), TransactionError> {
    let backup = backend
        .read_validated_backup(backup_path)
        .map_err(TransactionError::primary)?;
    let backup_policy = ScreenSaverPolicy::parse(&backup).map_err(policy_error)?;
    if !backup_policy.has_exactly_one_password_fallback() {
        return Err(TransactionError::primary(BackendError::new(
            "backup has no unique password fallback",
        )));
    }
    let _original_live = backend
        .read_screensaver_rule()
        .map_err(TransactionError::primary)?;
    let original_named = backend
        .read_named_rule()
        .map_err(TransactionError::primary)?;
    let named = named_rule_v1();
    let mut repaired_expected = None;
    let mut named_attempted = false;
    backend
        .start_journal(
            JournalOperation::Repair,
            &_original_live,
            original_named.as_deref(),
            Some(&backup),
        )
        .map_err(TransactionError::primary)?;
    backend
        .record_journal_phase(JournalPhase::Prepared)
        .map_err(TransactionError::primary)?;

    let operation = (|| {
        // Repair may start from an active, malformed, or partially configured
        // right. Establish and read back a unique password-only fallback
        // before touching the named rule or trusting a broad component state.
        deactivate_policy_for_repair(backend, &backup_policy)?;
        backend.record_journal_phase(JournalPhase::PolicyInactive)?;
        if original_named
            .as_deref()
            .is_some_and(|bytes| verify_named_rule_v1(bytes).is_err())
        {
            return Err(BackendError::new(
                "foreign named rule retained after repair disabled the active policy",
            ));
        }
        let InstallReceiptState::Trusted(installed_fingerprint) =
            backend.verify_install_receipt()?
        else {
            return Err(BackendError::new(
                "repair requires a trusted installed-generation receipt",
            ));
        };
        if backend.component_state()? != ComponentState::HealthyDenyOnly {
            return Err(BackendError::new(
                "repair is policy-only and requires healthy installed components",
            ));
        }
        backend.verify_active_service_and_components(Some(installed_fingerprint))?;

        let latest_named = backend.read_named_rule()?;
        if !same_named_definition(latest_named.as_deref(), original_named.as_deref()) {
            return Err(BackendError::new(
                "named rule changed before repair mutation",
            ));
        }
        named_attempted = true;
        backend.set_named_rule(&named)?;
        if backend
            .read_named_rule()?
            .as_deref()
            .is_none_or(|bytes| verify_named_rule_v1(bytes).is_err())
        {
            return Err(BackendError::new("repair named-rule readback differs"));
        }
        backend.record_journal_phase(JournalPhase::NamedReady)?;
        // Transform a last-moment read so parseable third-party changes are
        // preserved. The explicit backup is used only when live is malformed.
        let latest_live = backend.read_screensaver_rule()?;
        let base = ScreenSaverPolicy::parse(&latest_live).unwrap_or_else(|_| backup_policy.clone());
        let repaired = base
            .install(&PolicySpec::v1())
            .and_then(|policy| policy.to_bytes())
            .map_err(|error| BackendError::new(error.to_string()))?;
        repaired_expected = Some(repaired.clone());
        backend.set_screensaver_rule(&repaired)?;
        let readback = backend.read_screensaver_rule()?;
        if !policies_equivalent(&readback, &repaired)? {
            return Err(BackendError::new("repair policy readback differs"));
        }
        ScreenSaverPolicy::parse(&readback)
            .and_then(|policy| policy.verify_installed(&PolicySpec::v1()))
            .map_err(|error| BackendError::new(error.to_string()))?;
        backend.verify_active_service_and_components(Some(installed_fingerprint))?;
        if backend.verify_install_receipt()? != InstallReceiptState::Trusted(installed_fingerprint)
        {
            return Err(BackendError::new(
                "repair receipt generation drifted after activation",
            ));
        }
        if backend.component_state()? != ComponentState::HealthyDenyOnly
            || backend
                .read_named_rule()?
                .as_deref()
                .is_none_or(|bytes| verify_named_rule_v1(bytes).is_err())
        {
            return Err(BackendError::new(
                "repair dependency closure drifted after activation",
            ));
        }
        backend.record_journal_phase(JournalPhase::PolicyActive)?;
        Ok(())
    })();
    if let Err(primary) = operation {
        let mut rollback_failures = Vec::new();
        let policy_inactive =
            match surgically_remove_live_repose(backend, repaired_expected.as_deref()) {
                Ok(()) => true,
                Err(error) => {
                    rollback_failures.push(format!(
                        "repair policy could not be proven inactive: {error}"
                    ));
                    false
                }
            };
        if policy_inactive {
            if let Err(error) = backend.record_journal_phase(JournalPhase::PolicyInactive) {
                rollback_failures.push(format!("repair journal update failed: {error}"));
            }
            let mut named_restored = true;
            if named_attempted {
                let named_result = restore_named_if_unchanged(backend, original_named.as_deref());
                if let Err(error) = named_result {
                    named_restored = false;
                    rollback_failures.push(format!("repair named-rule rollback failed: {error}"));
                }
            }
            let inactive_after_named = live_policy_is_inactive(backend).unwrap_or(false);
            if named_restored && inactive_after_named {
                match backend.record_journal_phase(JournalPhase::Aborted) {
                    Ok(()) => {
                        if let Err(error) = backend.finish_journal() {
                            rollback_failures
                                .push(format!("repair journal finalization failed: {error}"));
                        }
                    }
                    Err(error) => rollback_failures.push(format!(
                        "repair abort marker could not be made durable: {error}"
                    )),
                }
            }
        } else {
            rollback_failures.push(
                "named rule and components retained because live policy is ambiguous".to_owned(),
            );
        }
        return Err(TransactionError {
            primary,
            rollback_failures,
        });
    }
    backend
        .finish_journal()
        .map_err(TransactionError::primary)?;
    Ok(())
}
