use repose_unlock_core::calibration::{
    CalibrationError, CalibrationPolicy, SampleGroup, calibrate,
};

#[test]
fn rejects_overlapping_near_and_far_samples() {
    let near = [-61, -60, -62, -61, -60, -62, -61, -60];
    let far = [-62, -61, -63, -62, -61, -63, -62, -61];

    let error = calibrate(&near, &far, &CalibrationPolicy::prototype()).unwrap_err();

    assert!(matches!(error, CalibrationError::OverlappingDistributions));
}

#[test]
fn isolated_outliers_do_not_expand_the_unlock_boundary() {
    let near = [-48, -49, -47, -48, -49, -47, -48, -90];
    let far = [-74, -75, -73, -74, -75, -73, -74, -20];

    let profile = calibrate(&near, &far, &CalibrationPolicy::prototype()).unwrap();

    assert!(profile.near_threshold_dbm > profile.far_threshold_dbm);
    assert!(profile.near_threshold_dbm - profile.far_threshold_dbm >= 8);
    assert_eq!(profile.near_threshold_dbm, -49);
    assert_eq!(profile.far_threshold_dbm, -73);
}

#[test]
fn rejects_insufficient_samples_from_either_group() {
    let enough = [-48, -49, -47, -48, -49, -47, -48, -49];
    let too_few = [-74, -75, -73];

    let error = calibrate(&enough, &too_few, &CalibrationPolicy::prototype()).unwrap_err();

    assert_eq!(
        error,
        CalibrationError::InsufficientSamples {
            group: SampleGroup::Far,
            required: 8,
            actual: 3,
        }
    );
}

#[test]
fn rejects_invalid_rssi_values() {
    let near = [-48, -49, -47, -48, 0, -47, -48, -49];
    let far = [-74, -75, -73, -74, -75, -73, -74, -75];

    let error = calibrate(&near, &far, &CalibrationPolicy::prototype()).unwrap_err();

    assert_eq!(
        error,
        CalibrationError::InvalidRssi {
            group: SampleGroup::Near,
            index: 4,
            value: 0,
        }
    );
}

#[test]
fn enforces_the_configured_minimum_separation() {
    let near = [-55, -56, -55, -56, -55, -56, -55, -56];
    let far = [-61, -62, -61, -62, -61, -62, -61, -62];

    let error = calibrate(&near, &far, &CalibrationPolicy::prototype()).unwrap_err();

    assert_eq!(
        error,
        CalibrationError::InsufficientSeparation {
            required_db: 8,
            observed_db: 5,
        }
    );
}

#[test]
fn calibration_is_deterministic_for_equivalent_sample_sets() {
    let near_a = [-48, -49, -47, -50, -46, -48, -49, -47];
    let far_a = [-74, -75, -73, -76, -72, -74, -75, -73];
    let near_b = [-47, -49, -48, -46, -50, -47, -48, -49];
    let far_b = [-73, -75, -74, -72, -76, -73, -74, -75];
    let policy = CalibrationPolicy::prototype();

    assert_eq!(
        calibrate(&near_a, &far_a, &policy),
        calibrate(&near_b, &far_b, &policy)
    );
}

#[test]
fn policy_constructor_rejects_invalid_values() {
    assert!(CalibrationPolicy::new(0, 25, 75, 8).is_err());
    assert!(CalibrationPolicy::new(8, 101, 75, 8).is_err());
    assert!(CalibrationPolicy::new(8, 25, 101, 8).is_err());
    assert!(CalibrationPolicy::new(8, 75, 25, 8).is_err());
    assert!(CalibrationPolicy::new(8, 25, 75, 0).is_err());
}
