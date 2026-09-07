use super::*;

pub fn recover_pending<B: InstallBackend>(backend: &mut B) -> Result<(), TransactionError> {
    backend
        .acquire_exclusive_lock()
        .map_err(TransactionError::primary)?;
    let result = backend
        .load_journal()
        .map_err(TransactionError::primary)
        .and_then(|journal| recover_loaded_journal(backend, journal));
    backend.release_exclusive_lock();
    result
}

pub(super) fn recover_loaded_journal<B: InstallBackend>(
    backend: &mut B,
    journal: Option<DurableJournal>,
) -> Result<(), TransactionError> {
    let Some(journal) = journal else {
        return Ok(());
    };
    let result = match journal.operation {
        JournalOperation::Install => recover_install(backend, &journal),
        JournalOperation::Uninstall => recover_uninstall(backend, &journal),
        JournalOperation::Repair => recover_repair(backend, &journal),
    };
    result.map_err(TransactionError::primary)
}

fn recover_install<B: InstallBackend>(
    backend: &mut B,
    journal: &DurableJournal,
) -> Result<(), BackendError> {
    // This covers the critical effect-before-marker window: if the final
    // policy write landed while the durable phase is still `NamedReady`, a
    // complete healthy closure is rolled forward, never blindly rolled back.
    if let Some(target_receipt) = journal.target_receipt
        && active_dependency_closure(backend, InstallReceiptState::Trusted(target_receipt))
            .unwrap_or(false)
    {
        backend.record_journal_phase(JournalPhase::Committed)?;
        backend.finish_component_update()?;
        return backend.finish_journal();
    }

    surgically_remove_live_repose(backend, None)?;
    if journal.phase == JournalPhase::Committed {
        // `Committed` promises that old rollback material may already be
        // gone. Only roll forward/clean up; never claim the old generation can
        // still be restored.
        backend.finish_component_update()?;
        return backend.finish_journal();
    }

    ensure_named_recovery_is_safe(backend, journal.named_rule.as_deref())?;
    backend.rollback_component_update()?;
    if backend.verify_install_receipt()? != journal.prior_receipt {
        return Err(BackendError::new(
            "prior generation receipt could not be restored during recovery",
        ));
    }
    restore_named_snapshot(backend, journal.named_rule.as_deref())?;

    let originally_active = ScreenSaverPolicy::parse(&journal.policy)
        .is_ok_and(|policy| policy.verify_installed(&PolicySpec::v1()).is_ok());
    if originally_active {
        let InstallReceiptState::Trusted(prior_fingerprint) = journal.prior_receipt else {
            return Err(BackendError::new(
                "active journal preimage has no trusted prior receipt",
            ));
        };
        if backend.verify_install_receipt()? != journal.prior_receipt
            || backend.component_state()? != ComponentState::HealthyDenyOnly
            || backend
                .verify_active_service_and_components(Some(prior_fingerprint))
                .is_err()
            || backend
                .read_named_rule()?
                .as_deref()
                .is_none_or(|bytes| verify_named_rule_v1(bytes).is_err())
        {
            return Err(BackendError::new(
                "old dependency closure could not be revalidated during recovery",
            ));
        }
        restore_repose_on_latest_live_policy(backend, journal.prior_receipt)?;
        if !active_dependency_closure(backend, journal.prior_receipt)? {
            surgically_remove_live_repose(backend, None)?;
            return Err(BackendError::new(
                "old dependency closure drifted during policy recovery",
            ));
        }
    }
    backend.finish_component_update()?;
    backend.finish_journal()
}

fn recover_uninstall<B: InstallBackend>(
    backend: &mut B,
    journal: &DurableJournal,
) -> Result<(), BackendError> {
    // A malformed value at Prepared cannot be an effect of our surgical
    // transform. No component teardown was authorized yet, so clear the
    // journal and retain every dependency for explicit policy repair.
    if journal.phase == JournalPhase::Prepared {
        let live = backend.read_screensaver_rule()?;
        if ScreenSaverPolicy::parse(&live).is_err() {
            return backend.finish_journal();
        }
    }
    continue_uninstall(backend, journal.prior_receipt)
}

fn recover_repair<B: InstallBackend>(
    backend: &mut B,
    journal: &DurableJournal,
) -> Result<(), BackendError> {
    if journal.phase == JournalPhase::Aborted {
        let repair_base = journal
            .repair_base
            .as_deref()
            .ok_or_else(|| BackendError::new("aborted repair journal lacks its recovery base"))?;
        let repair_base = ScreenSaverPolicy::parse(repair_base)
            .map_err(|error| BackendError::new(error.to_string()))?;
        deactivate_policy_for_repair(backend, &repair_base)?;
        return backend.finish_journal();
    }
    if active_dependency_closure(backend, journal.prior_receipt).unwrap_or(false) {
        return backend.finish_journal();
    }
    let repair_base = journal
        .repair_base
        .as_deref()
        .ok_or_else(|| BackendError::new("repair journal lacks a validated recovery base"))?;
    let repair_base = ScreenSaverPolicy::parse(repair_base)
        .map_err(|error| BackendError::new(error.to_string()))?;
    if !repair_base.has_exactly_one_password_fallback() {
        return Err(BackendError::new(
            "repair journal recovery base has no unique password fallback",
        ));
    }
    deactivate_policy_for_repair(backend, &repair_base)?;
    backend.record_journal_phase(JournalPhase::PolicyInactive)?;

    // A pre-terminal repair phase can mean either a process crash while
    // rolling forward or a failed attempt to persist the `Aborted` marker.
    // Those states are intentionally indistinguishable after power loss.  Do
    // not silently activate authentication from an ambiguous journal: retain
    // the healthy dependencies, restore only the journaled named-rule state,
    // and finish password-only.  An operator can explicitly retry repair.
    restore_repair_named_snapshot(backend, journal.named_rule.as_deref())?;
    backend.record_journal_phase(JournalPhase::Aborted)?;
    backend.finish_journal()
}

fn restore_repair_named_snapshot<B: InstallBackend>(
    backend: &mut B,
    previous: Option<&[u8]>,
) -> Result<(), BackendError> {
    let current = backend.read_named_rule()?;
    let previous_is_ours = previous.is_some_and(|bytes| verify_named_rule_v1(bytes).is_ok());
    let current_is_ours = current
        .as_deref()
        .is_some_and(|bytes| verify_named_rule_v1(bytes).is_ok());

    match (
        previous,
        current.as_deref(),
        previous_is_ours,
        current_is_ours,
    ) {
        (None, None, _, _) => {}
        (None, Some(_), _, true) => backend.remove_named_rule()?,
        (Some(_), None, true, _) => backend.set_named_rule(&named_rule_v1())?,
        (Some(_), Some(_), true, true) => {}
        (Some(previous), Some(current), false, false) if previous == current => {}
        _ => {
            return Err(BackendError::new(
                "repair recovery retained a concurrently changed named rule",
            ));
        }
    }

    let readback = backend.read_named_rule()?;
    if previous_is_ours {
        if !same_named_definition(readback.as_deref(), previous) {
            return Err(BackendError::new(
                "repair named-rule abort readback differs",
            ));
        }
    } else if readback.as_deref() != previous {
        return Err(BackendError::new(
            "foreign named rule changed during repair abort",
        ));
    }
    Ok(())
}

fn ensure_named_recovery_is_safe<B: InstallBackend>(
    backend: &mut B,
    previous: Option<&[u8]>,
) -> Result<(), BackendError> {
    if previous.is_some_and(|bytes| verify_named_rule_v1(bytes).is_err()) {
        return Err(BackendError::new(
            "journal contains a foreign named-rule snapshot",
        ));
    }
    if backend
        .read_named_rule()?
        .as_deref()
        .is_some_and(|bytes| verify_named_rule_v1(bytes).is_err())
    {
        return Err(BackendError::new(
            "live named rule changed concurrently; dependencies retained",
        ));
    }
    Ok(())
}

fn restore_named_snapshot<B: InstallBackend>(
    backend: &mut B,
    previous: Option<&[u8]>,
) -> Result<(), BackendError> {
    let current = backend.read_named_rule()?;
    if current
        .as_deref()
        .is_some_and(|bytes| verify_named_rule_v1(bytes).is_err())
    {
        return Err(BackendError::new(
            "live named rule changed concurrently; left untouched",
        ));
    }
    match (current, previous) {
        (None, None) | (Some(_), Some(_)) => {}
        (Some(_), None) => backend.remove_named_rule()?,
        (None, Some(bytes)) => backend.set_named_rule(bytes)?,
    }
    let readback = backend.read_named_rule()?;
    if !same_named_definition(readback.as_deref(), previous) {
        return Err(BackendError::new("named-rule recovery readback differs"));
    }
    Ok(())
}
