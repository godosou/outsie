use repose_unlock_core::{
    calibration::{CalibrationPolicy, calibrate},
    domain::MonoMillis,
    proximity::{
        ProximityError, ProximityEvent, ProximityFilter, ProximityPolicy, ProximityPolicyError,
    },
};

fn calibrated_profile() -> repose_unlock_core::calibration::CalibrationProfile {
    calibrate(
        &[-48, -49, -47, -48, -49, -47, -48, -49],
        &[-74, -75, -73, -74, -75, -73, -74, -75],
        &CalibrationPolicy::prototype(),
    )
    .unwrap()
}

#[test]
fn stable_events_require_a_full_window_and_the_complete_dwell() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(50)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    assert_eq!(filter.push(-80, MonoMillis::new(0)).unwrap(), None);
    assert_eq!(filter.push(-79, MonoMillis::new(10)).unwrap(), None);
    assert_eq!(filter.push(-78, MonoMillis::new(20)).unwrap(), None);
    assert_eq!(filter.push(-80, MonoMillis::new(70)).unwrap(), None);
    assert_eq!(
        filter.push(-79, MonoMillis::new(120)).unwrap(),
        Some(ProximityEvent::FarStable)
    );
    assert_eq!(filter.push(-80, MonoMillis::new(220)).unwrap(), None);
}

#[test]
fn filter_emits_a_near_transition_after_a_far_transition() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(50)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    filter.push(-80, MonoMillis::new(0)).unwrap();
    filter.push(-79, MonoMillis::new(10)).unwrap();
    filter.push(-78, MonoMillis::new(20)).unwrap();
    filter.push(-79, MonoMillis::new(70)).unwrap();
    assert_eq!(
        filter.push(-79, MonoMillis::new(120)).unwrap(),
        Some(ProximityEvent::FarStable)
    );

    assert_eq!(filter.push(-45, MonoMillis::new(160)).unwrap(), None);
    assert_eq!(filter.push(-46, MonoMillis::new(170)).unwrap(), None);
    assert_eq!(filter.push(-47, MonoMillis::new(220)).unwrap(), None);
    assert_eq!(
        filter.push(-46, MonoMillis::new(270)).unwrap(),
        Some(ProximityEvent::NearStable)
    );
}

#[test]
fn indeterminate_samples_reset_the_pending_dwell() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(100)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    filter.push(-80, MonoMillis::new(0)).unwrap();
    filter.push(-79, MonoMillis::new(10)).unwrap();
    filter.push(-78, MonoMillis::new(20)).unwrap();
    filter.push(-60, MonoMillis::new(80)).unwrap();
    filter.push(-60, MonoMillis::new(90)).unwrap();
    assert_eq!(filter.push(-60, MonoMillis::new(100)).unwrap(), None);
    assert_eq!(filter.push(-80, MonoMillis::new(110)).unwrap(), None);
    assert_eq!(filter.push(-80, MonoMillis::new(120)).unwrap(), None);
    assert_eq!(filter.push(-80, MonoMillis::new(130)).unwrap(), None);
    assert_eq!(filter.push(-80, MonoMillis::new(219)).unwrap(), None);
    assert_eq!(
        filter.push(-80, MonoMillis::new(220)).unwrap(),
        Some(ProximityEvent::FarStable)
    );
}

#[test]
fn rejects_invalid_samples_and_non_monotonic_time() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(50)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    assert_eq!(
        filter.push(0, MonoMillis::new(0)),
        Err(ProximityError::InvalidRssi { value: 0 })
    );
    filter.push(-80, MonoMillis::new(10)).unwrap();
    assert_eq!(
        filter.push(-80, MonoMillis::new(9)),
        Err(ProximityError::NonMonotonicTime {
            previous: MonoMillis::new(10),
            current: MonoMillis::new(9),
        })
    );
}

#[test]
fn validates_proximity_policy_inputs() {
    assert!(ProximityPolicy::new(0, MonoMillis::new(100), MonoMillis::new(50)).is_err());
    assert!(ProximityPolicy::new(3, MonoMillis::new(0), MonoMillis::new(50)).is_err());
    assert!(ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(0)).is_err());
}

#[test]
fn rejects_even_sample_windows_without_a_strict_median_majority() {
    assert!(ProximityPolicy::new(4, MonoMillis::new(100), MonoMillis::new(50)).is_err());
}

#[test]
fn rejects_single_sample_windows_without_robust_aggregation() {
    assert_eq!(
        ProximityPolicy::new(1, MonoMillis::new(100), MonoMillis::new(50)),
        Err(ProximityPolicyError::SampleWindowTooSmall { value: 1 })
    );
}

#[test]
fn a_gap_beyond_the_limit_restarts_the_window_and_dwell() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(50)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    filter.push(-80, MonoMillis::new(0)).unwrap();
    filter.push(-79, MonoMillis::new(10)).unwrap();
    filter.push(-78, MonoMillis::new(20)).unwrap();

    assert_eq!(filter.push(-80, MonoMillis::new(121)).unwrap(), None);
    assert_eq!(filter.push(-79, MonoMillis::new(171)).unwrap(), None);
    assert_eq!(filter.push(-78, MonoMillis::new(221)).unwrap(), None);
    assert_eq!(filter.push(-80, MonoMillis::new(271)).unwrap(), None);
    assert_eq!(
        filter.push(-79, MonoMillis::new(321)).unwrap(),
        Some(ProximityEvent::FarStable)
    );
}

#[test]
fn an_exact_maximum_gap_preserves_continuous_evidence() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(50)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    filter.push(-80, MonoMillis::new(0)).unwrap();
    filter.push(-79, MonoMillis::new(50)).unwrap();
    filter.push(-78, MonoMillis::new(100)).unwrap();
    assert_eq!(filter.push(-80, MonoMillis::new(150)).unwrap(), None);
    assert_eq!(
        filter.push(-79, MonoMillis::new(200)).unwrap(),
        Some(ProximityEvent::FarStable)
    );
}

#[test]
fn invalid_rssi_clears_pending_continuity() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(50)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    filter.push(-80, MonoMillis::new(0)).unwrap();
    filter.push(-79, MonoMillis::new(10)).unwrap();
    filter.push(-78, MonoMillis::new(20)).unwrap();
    assert_eq!(
        filter.push(0, MonoMillis::new(70)),
        Err(ProximityError::InvalidRssi { value: 0 })
    );

    filter.push(-80, MonoMillis::new(80)).unwrap();
    filter.push(-79, MonoMillis::new(90)).unwrap();
    assert_eq!(filter.push(-78, MonoMillis::new(100)).unwrap(), None);
    assert_eq!(filter.push(-80, MonoMillis::new(150)).unwrap(), None);
    assert_eq!(
        filter.push(-79, MonoMillis::new(200)).unwrap(),
        Some(ProximityEvent::FarStable)
    );
}

#[test]
fn invalid_rssi_requires_reacquiring_an_already_stable_state() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(50)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    filter.push(-80, MonoMillis::new(0)).unwrap();
    filter.push(-79, MonoMillis::new(50)).unwrap();
    filter.push(-78, MonoMillis::new(100)).unwrap();
    filter.push(-80, MonoMillis::new(150)).unwrap();
    assert_eq!(
        filter.push(-79, MonoMillis::new(200)).unwrap(),
        Some(ProximityEvent::FarStable)
    );
    assert_eq!(
        filter.push(0, MonoMillis::new(250)),
        Err(ProximityError::InvalidRssi { value: 0 })
    );

    filter.push(-80, MonoMillis::new(260)).unwrap();
    filter.push(-79, MonoMillis::new(270)).unwrap();
    filter.push(-78, MonoMillis::new(280)).unwrap();
    filter.push(-80, MonoMillis::new(330)).unwrap();
    assert_eq!(
        filter.push(-79, MonoMillis::new(380)).unwrap(),
        Some(ProximityEvent::FarStable)
    );
}

#[test]
fn non_monotonic_input_clears_pending_continuity() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100), MonoMillis::new(50)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    filter.push(-80, MonoMillis::new(0)).unwrap();
    filter.push(-79, MonoMillis::new(10)).unwrap();
    filter.push(-78, MonoMillis::new(20)).unwrap();
    assert_eq!(
        filter.push(-80, MonoMillis::new(15)),
        Err(ProximityError::NonMonotonicTime {
            previous: MonoMillis::new(20),
            current: MonoMillis::new(15),
        })
    );

    filter.push(-80, MonoMillis::new(70)).unwrap();
    filter.push(-79, MonoMillis::new(80)).unwrap();
    assert_eq!(filter.push(-78, MonoMillis::new(90)).unwrap(), None);
    assert_eq!(filter.push(-80, MonoMillis::new(140)).unwrap(), None);
    assert_eq!(
        filter.push(-79, MonoMillis::new(190)).unwrap(),
        Some(ProximityEvent::FarStable)
    );
}
