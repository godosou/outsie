use super::*;

pub fn uninstall<B: InstallBackend>(backend: &mut B) -> Result<(), TransactionError> {
    backend
        .acquire_exclusive_lock()
        .map_err(TransactionError::primary)?;
    let result = backend
        .load_journal()
        .map_err(TransactionError::primary)
        .and_then(|journal| recover_loaded_journal(backend, journal))
        .and_then(|()| uninstall_locked(backend));
    backend.release_exclusive_lock();
    result
}

fn uninstall_locked<B: InstallBackend>(backend: &mut B) -> Result<(), TransactionError> {
    // The reference is removed and read back before even inspecting component
    // removal. Any ambiguous Authorization Services error exits here.
    let live = backend
        .read_screensaver_rule()
        .map_err(TransactionError::primary)?;
    let initial_named = backend
        .read_named_rule()
        .map_err(TransactionError::primary)?;
    // Reject malformed input before creating a journal. Authorization
    // Services is re-read again after the journal, so this is validation—not
    // a stale write source.
    ScreenSaverPolicy::parse(&live)
        .and_then(|policy| policy.remove(&PolicySpec::v1()))
        .map_err(policy_error)?;
    backend
        .start_journal(
            JournalOperation::Uninstall,
            &live,
            initial_named.as_deref(),
            None,
        )
        .map_err(TransactionError::primary)?;
    backend
        .record_journal_phase(JournalPhase::Prepared)
        .map_err(TransactionError::primary)?;
    let prior_receipt = backend
        .load_journal()
        .map_err(TransactionError::primary)?
        .ok_or_else(|| TransactionError::primary(BackendError::new("uninstall journal vanished")))?
        .prior_receipt;
    continue_uninstall(backend, prior_receipt).map_err(TransactionError::primary)
}

pub(super) fn continue_uninstall<B: InstallBackend>(
    backend: &mut B,
    prior_receipt: InstallReceiptState,
) -> Result<(), BackendError> {
    // Always operate on the latest live value; the journal preimage is audit
    // data, never a compare-and-swap substitute.
    surgically_remove_live_repose(backend, None)?;
    backend.record_journal_phase(JournalPhase::PolicyInactive)?;

    confirm_uninstall_inactive(backend)?;
    let named = backend.read_named_rule()?;
    if let Some(bytes) = named {
        verify_named_rule_v1(&bytes)
            .map_err(|_| BackendError::new("foreign named rule retained during uninstall"))?;
        backend.remove_named_rule()?;
    }
    if backend.read_named_rule()?.is_some() {
        return Err(BackendError::new("named rule remains after removal"));
    }
    backend.record_journal_phase(JournalPhase::NamedReady)?;

    confirm_uninstall_inactive(backend)?;
    let receipt = backend.verify_install_receipt()?;
    let component_state = backend.component_state()?;
    if component_state == ComponentState::Missing {
        if receipt != prior_receipt && receipt != InstallReceiptState::Missing {
            return Err(BackendError::new(
                "foreign install receipt appeared after component removal",
            ));
        }
        backend.finish_component_removal(match prior_receipt {
            InstallReceiptState::Trusted(fingerprint) => Some(fingerprint),
            InstallReceiptState::Missing => None,
        })?;
        backend.record_journal_phase(JournalPhase::ComponentsRemoved)?;
        return backend.finish_journal();
    }
    if receipt != prior_receipt {
        return Err(BackendError::new(
            "installed generation changed after uninstall was journaled",
        ));
    }
    let InstallReceiptState::Trusted(expected_generation) = prior_receipt else {
        return Err(BackendError::new(
            "components are present without a trusted install receipt",
        ));
    };
    // The launchd label is not ownership. Bind a running image to the receipt,
    // while allowing recovery to continue if our service was already stopped.
    if backend.service_removal_state(expected_generation)? == ServiceRemovalState::RunningTrusted {
        backend.stop_service(expected_generation)?;
    }
    // A concurrent writer may reactivate immediately after bootout. Check
    // again before each destructive step and pause after surgical deactivation.
    for component in Component::REMOVE_ORDER {
        confirm_uninstall_inactive(backend)?;
        if backend.verify_install_receipt()? != prior_receipt {
            return Err(BackendError::new(
                "install receipt generation changed during component removal",
            ));
        }
        backend.remove_component(component, expected_generation)?;
        confirm_uninstall_inactive(backend)?;
    }
    backend.finish_component_removal(Some(expected_generation))?;
    backend.record_journal_phase(JournalPhase::ComponentsRemoved)?;
    backend.finish_journal()
}

fn confirm_uninstall_inactive<B: InstallBackend>(backend: &mut B) -> Result<(), BackendError> {
    let live = backend.read_screensaver_rule()?;
    if verify_repose_absent(&live).is_ok() {
        return Ok(());
    }
    surgically_remove_live_repose(backend, None)?;
    Err(BackendError::new(
        "screensaver candidate reappeared during uninstall and was disabled; teardown paused",
    ))
}
