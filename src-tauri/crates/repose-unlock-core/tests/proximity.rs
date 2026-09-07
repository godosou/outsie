use repose_unlock_core::{
    calibration::{CalibrationPolicy, calibrate},
    domain::MonoMillis,
    proximity::{ProximityError, ProximityEvent, ProximityFilter, ProximityPolicy},
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
    let policy = ProximityPolicy::new(3, MonoMillis::new(100)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    assert_eq!(filter.push(-80, MonoMillis::new(0)).unwrap(), None);
    assert_eq!(filter.push(-79, MonoMillis::new(10)).unwrap(), None);
    assert_eq!(filter.push(-78, MonoMillis::new(20)).unwrap(), None);
    assert_eq!(filter.push(-80, MonoMillis::new(119)).unwrap(), None);
    assert_eq!(
        filter.push(-79, MonoMillis::new(120)).unwrap(),
        Some(ProximityEvent::FarStable)
    );
    assert_eq!(filter.push(-80, MonoMillis::new(220)).unwrap(), None);
}

#[test]
fn filter_emits_a_near_transition_after_a_far_transition() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100)).unwrap();
    let mut filter = ProximityFilter::new(calibrated_profile(), policy).unwrap();

    filter.push(-80, MonoMillis::new(0)).unwrap();
    filter.push(-79, MonoMillis::new(10)).unwrap();
    filter.push(-78, MonoMillis::new(20)).unwrap();
    assert_eq!(
        filter.push(-79, MonoMillis::new(120)).unwrap(),
        Some(ProximityEvent::FarStable)
    );

    assert_eq!(filter.push(-45, MonoMillis::new(200)).unwrap(), None);
    assert_eq!(filter.push(-46, MonoMillis::new(210)).unwrap(), None);
    assert_eq!(filter.push(-47, MonoMillis::new(309)).unwrap(), None);
    assert_eq!(
        filter.push(-46, MonoMillis::new(310)).unwrap(),
        Some(ProximityEvent::NearStable)
    );
}

#[test]
fn indeterminate_samples_reset_the_pending_dwell() {
    let policy = ProximityPolicy::new(3, MonoMillis::new(100)).unwrap();
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
    let policy = ProximityPolicy::new(3, MonoMillis::new(100)).unwrap();
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
    assert!(ProximityPolicy::new(0, MonoMillis::new(100)).is_err());
    assert!(ProximityPolicy::new(3, MonoMillis::new(0)).is_err());
}
