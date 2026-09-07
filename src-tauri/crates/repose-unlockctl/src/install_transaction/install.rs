use super::*;

pub fn install<B: InstallBackend>(
    backend: &mut B,
    request: &InstallRequest,
) -> Result<(), TransactionError> {
    backend
        .acquire_exclusive_lock()
        .map_err(TransactionError::primary)?;
    let result = backend
        .load_journal()
        .map_err(TransactionError::primary)
        .and_then(|journal| recover_loaded_journal(backend, journal))
        .and_then(|()| install_locked(backend, request));
    backend.release_exclusive_lock();
    result
}

fn install_locked<B: InstallBackend>(
    backend: &mut B,
    request: &InstallRequest,
) -> Result<(), TransactionError> {
    let artifacts = backend
        .verify_apply_artifacts(request.artifacts())
        .map_err(TransactionError::primary)?;
    let initial_policy = backend
        .read_screensaver_rule()
        .map_err(TransactionError::primary)?;
    let initial_parsed = ScreenSaverPolicy::parse(&initial_policy).map_err(policy_error)?;
    let initially_active = initial_parsed.repose_candidate_index().is_some();
    let initial_policy_exact = initial_parsed.verify_installed(&PolicySpec::v1()).is_ok();
    let initial_named = backend
        .read_named_rule()
        .map_err(TransactionError::primary)?;
    let named_is_exact = initial_named
        .as_deref()
        .is_some_and(|bytes| verify_named_rule_v1(bytes).is_ok());
    let initial_component_state = backend
        .component_state()
        .map_err(TransactionError::primary)?;
    let receipt_result = backend.verify_install_receipt();
    if !initially_active {
        receipt_result
            .as_ref()
            .map_err(|error| TransactionError::primary(BackendError::new(error.to_string())))?;
    }
    let initial_receipt = receipt_result
        .as_ref()
        .copied()
        .unwrap_or(InstallReceiptState::Missing);
    let old_components_healthy = if initially_active {
        initial_component_state == ComponentState::HealthyDenyOnly
            && match initial_receipt {
                InstallReceiptState::Trusted(fingerprint) => backend
                    .verify_active_service_and_components(Some(fingerprint))
                    .is_ok(),
                InstallReceiptState::Missing => false,
            }
    } else {
        false
    };
    if !initially_active && initial_named.is_some() && !named_is_exact {
        return Err(TransactionError::primary(BackendError::new(
            "foreign definition occupies ai.repose.unlock",
        )));
    }
    backend
        .start_journal(
            JournalOperation::Install,
            &initial_policy,
            initial_named.as_deref(),
            None,
        )
        .map_err(TransactionError::primary)?;
    backend
        .record_journal_phase(JournalPhase::Prepared)
        .map_err(TransactionError::primary)?;
    // Persist the validated preimage after the durable journal but before any
    // authorization or component mutation.
    backend
        .persist_install_backup(&initial_policy)
        .map_err(TransactionError::primary)?;
    if initially_active
        && (!initial_policy_exact
            || !named_is_exact
            || !old_components_healthy
            || receipt_result.is_err())
    {
        let primary = BackendError::new(
            "active upgrade found an incomplete dependency closure and disabled it",
        );
        return match surgically_remove_live_repose(backend, None) {
            Ok(()) => {
                let _ = backend.record_journal_phase(JournalPhase::PolicyInactive);
                let _ = backend.finish_journal();
                Err(TransactionError::primary(primary))
            }
            Err(rollback) => Err(TransactionError {
                primary,
                rollback_failures: vec![format!(
                    "could not prove the unsafe path inactive: {rollback}"
                )],
            }),
        };
    }
    let mut state = RollbackState {
        initial_policy,
        initial_named,
        initially_active,
        policy_was_deactivated: false,
        final_policy_attempted: false,
        final_policy_expected: None,
        named_attempted: false,
        components_started: false,
        initial_receipt,
    };

    if initially_active && let Err(error) = deactivate_for_upgrade(backend, &mut state) {
        return Err(rollback_install(backend, state, error));
    }
    // The initial read and even an upgrade deactivation are stale by the time
    // component work begins. Prove the live path inactive again after the
    // durable backup, immediately before recording the phase that authorizes
    // component mutation.
    if let Err(error) = surgically_remove_live_repose(backend, None) {
        return Err(rollback_install(backend, state, error));
    }
    if let Err(error) = backend.record_journal_phase(JournalPhase::PolicyInactive) {
        return Err(rollback_install(backend, state, error));
    }

    let operation = (|| {
        state.components_started = true;
        backend.begin_component_update(&artifacts)?;
        for component in Component::INSTALL_ORDER {
            backend.stage_component(component, &artifacts)?;
        }
        backend.verify_staged_components()?;
        backend.verify_staged_deny_only_health()?;
        surgically_remove_live_repose(backend, None)?;
        backend.commit_component_update()?;
        surgically_remove_live_repose(backend, None)?;
        backend.activate_installed_service()?;
        surgically_remove_live_repose(backend, None)?;
        backend.verify_active_service_and_components(None)?;
        let target_receipt = backend.persist_install_receipt()?;
        backend.record_target_receipt(target_receipt)?;
        if backend.verify_install_receipt()? != InstallReceiptState::Trusted(target_receipt) {
            return Err(BackendError::new(
                "installed generation receipt differs from the recorded target",
            ));
        }
        backend.verify_active_service_and_components(Some(target_receipt))?;
        backend.record_journal_phase(JournalPhase::ComponentsCommittedHealthy)?;

        let named = named_rule_v1();
        let latest_named = backend.read_named_rule()?;
        if !same_named_definition(latest_named.as_deref(), state.initial_named.as_deref()) {
            return Err(BackendError::new(
                "named rule changed while components were staged",
            ));
        }
        state.named_attempted = true;
        backend.set_named_rule(&named)?;
        let named_readback = backend.read_named_rule()?;
        if named_readback
            .as_deref()
            .is_none_or(|bytes| verify_named_rule_v1(bytes).is_err())
        {
            return Err(BackendError::new("named-rule readback differs"));
        }
        verify_named_rule_v1(&named_readback.expect("checked Some"))
            .map_err(|error| BackendError::new(error.to_string()))?;
        backend.record_journal_phase(JournalPhase::NamedReady)?;

        // Authorization Services offers no compare-and-swap. Re-read as late
        // as possible and transform that live value, never an earlier backup.
        let live = backend.read_screensaver_rule()?;
        let installed = ScreenSaverPolicy::parse(&live)
            .map_err(|error| BackendError::new(error.to_string()))?
            .install(&PolicySpec::v1())
            .map_err(|error| BackendError::new(error.to_string()))?
            .to_bytes()
            .map_err(|error| BackendError::new(error.to_string()))?;
        state.final_policy_attempted = true;
        state.final_policy_expected = Some(installed.clone());
        backend.set_screensaver_rule(&installed)?;
        let readback = backend.read_screensaver_rule()?;
        ScreenSaverPolicy::parse(&readback)
            .and_then(|policy| policy.verify_installed(&PolicySpec::v1()))
            .map_err(|error| BackendError::new(error.to_string()))?;
        if !policies_equivalent(&readback, &installed)? {
            return Err(BackendError::new("screensaver rule readback differs"));
        }
        // Close the final authorization dependency graph only after every
        // dependency is re-read in its active state.
        if backend.verify_install_receipt()? != InstallReceiptState::Trusted(target_receipt) {
            return Err(BackendError::new(
                "target receipt drifted after policy activation",
            ));
        }
        if backend.component_state()? != ComponentState::HealthyDenyOnly {
            return Err(BackendError::new(
                "component closure drifted after policy activation",
            ));
        }
        if backend
            .read_named_rule()?
            .as_deref()
            .is_none_or(|bytes| verify_named_rule_v1(bytes).is_err())
        {
            return Err(BackendError::new(
                "named-rule closure drifted after policy activation",
            ));
        }
        let final_policy = backend.read_screensaver_rule()?;
        ScreenSaverPolicy::parse(&final_policy)
            .and_then(|policy| policy.verify_installed(&PolicySpec::v1()))
            .map_err(|error| BackendError::new(error.to_string()))?;
        // Make the receipt-bound loaded generation the final dependency read
        // before the durable active marker. Cross-process filesystem CAS is
        // unavailable, so Task 8 still requires an exclusive maintenance
        // window, but no earlier broad `Trusted` result authorizes this step.
        backend.verify_active_service_and_components(Some(target_receipt))?;
        backend.record_journal_phase(JournalPhase::PolicyActive)?;
        Ok(())
    })();

    match operation {
        Ok(()) => {
            backend
                .record_journal_phase(JournalPhase::Committed)
                .map_err(TransactionError::primary)?;
            backend
                .finish_component_update()
                .map_err(TransactionError::primary)?;
            backend
                .finish_journal()
                .map_err(TransactionError::primary)?;
            Ok(())
        }
        Err(error) => Err(rollback_install(backend, state, error)),
    }
}

fn deactivate_for_upgrade<B: InstallBackend>(
    backend: &mut B,
    state: &mut RollbackState,
) -> Result<(), BackendError> {
    let latest_live = backend.read_screensaver_rule()?;
    let removed = ScreenSaverPolicy::parse(&latest_live)
        .map_err(|error| BackendError::new(error.to_string()))?
        .remove(&PolicySpec::v1())
        .map_err(|error| BackendError::new(error.to_string()))?
        .to_bytes()
        .map_err(|error| BackendError::new(error.to_string()))?;
    state.policy_was_deactivated = true;
    backend.set_screensaver_rule(&removed)?;
    let readback = backend.read_screensaver_rule()?;
    verify_repose_absent(&readback)?;
    if !policies_equivalent(&readback, &removed)? {
        return Err(BackendError::new("upgrade deactivation readback differs"));
    }
    Ok(())
}

fn rollback_install<B: InstallBackend>(
    backend: &mut B,
    state: RollbackState,
    primary: BackendError,
) -> TransactionError {
    let mut rollback_failures = Vec::new();
    // Even a fresh install can race an external authorization writer. Never
    // remove or restore components based on an earlier "inactive" observation.
    let policy_safely_inactive =
        match surgically_remove_live_repose(backend, state.final_policy_expected.as_deref()) {
            Ok(()) => true,
            Err(error) => {
                rollback_failures.push(format!("could not deactivate authorization path: {error}"));
                false
            }
        };

    let components_restored = if state.components_started && policy_safely_inactive {
        match backend.rollback_component_update() {
            Ok(()) => match backend.verify_install_receipt() {
                Ok(receipt) if receipt == state.initial_receipt => true,
                Ok(_) => {
                    rollback_failures.push("prior generation receipt was not restored".to_owned());
                    false
                }
                Err(error) => {
                    rollback_failures.push(format!(
                        "prior generation receipt validation failed: {error}"
                    ));
                    false
                }
            },
            Err(error) => {
                rollback_failures.push(format!("component rollback failed: {error}"));
                false
            }
        }
    } else {
        !state.components_started
    };

    let named_restored = if state.named_attempted && policy_safely_inactive {
        match restore_named_if_unchanged(backend, state.initial_named.as_deref()) {
            Ok(()) => true,
            Err(error) => {
                rollback_failures.push(format!("named-rule rollback failed: {error}"));
                false
            }
        }
    } else {
        !state.named_attempted
    };

    let mut terminal_policy_safe = policy_safely_inactive;
    if state.initially_active && policy_safely_inactive && components_restored && named_restored {
        let dependency_closure_ready = backend
            .verify_install_receipt()
            .is_ok_and(InstallReceiptState::is_trusted)
            && match state.initial_receipt {
                InstallReceiptState::Trusted(fingerprint) => backend
                    .verify_active_service_and_components(Some(fingerprint))
                    .is_ok(),
                InstallReceiptState::Missing => false,
            }
            && backend
                .component_state()
                .is_ok_and(|state| state == ComponentState::HealthyDenyOnly)
            && backend.read_named_rule().is_ok_and(|named| {
                named
                    .as_deref()
                    .is_some_and(|bytes| verify_named_rule_v1(bytes).is_ok())
            });
        if !dependency_closure_ready {
            rollback_failures.push(
                "old dependency closure drifted; authorization remains password-only".to_owned(),
            );
            terminal_policy_safe = live_policy_is_inactive(backend).unwrap_or(false);
        } else {
            match restore_repose_on_latest_live_policy(backend, state.initial_receipt) {
                Ok(()) => {
                    terminal_policy_safe =
                        active_dependency_closure(backend, state.initial_receipt).unwrap_or(false);
                    if !terminal_policy_safe {
                        rollback_failures
                            .push("old authorization closure drifted after restoration".to_owned());
                    }
                }
                Err(error) => {
                    rollback_failures
                        .push(format!("old authorization path remains disabled: {error}"));
                    terminal_policy_safe = live_policy_is_inactive(backend).unwrap_or(false);
                }
            }
        }
    }

    if terminal_policy_safe
        && components_restored
        && named_restored
        && let Err(error) = backend.finish_journal()
    {
        rollback_failures.push(format!("journal finalization failed: {error}"));
    }

    if state.components_started && !(policy_safely_inactive && components_restored) {
        rollback_failures.push(
            "components retained because authorization state or component rollback was uncertain"
                .to_owned(),
        );
    }
    TransactionError {
        primary,
        rollback_failures,
    }
}
