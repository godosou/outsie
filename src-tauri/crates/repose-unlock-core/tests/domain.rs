use repose_unlock_core::domain::{
    AuditSessionId, ConsoleUid, LockEpoch, MonoMillis, RssiDbm, RssiError,
};

#[test]
fn identifiers_and_monotonic_time_remain_distinct_newtypes() {
    assert_eq!(MonoMillis::new(42).get(), 42);
    assert_eq!(LockEpoch::new(42).get(), 42);
    assert_eq!(AuditSessionId::new(42).get(), 42);
    assert_eq!(ConsoleUid::new(42).get(), 42);
}

#[test]
fn rssi_newtype_enforces_the_ble_measurement_range() {
    assert_eq!(RssiDbm::try_new(-1).unwrap().get(), -1);
    assert_eq!(RssiDbm::try_new(-127).unwrap().get(), -127);
    assert_eq!(RssiDbm::try_new(0), Err(RssiError { value: 0 }));
    assert_eq!(RssiDbm::try_new(-128), Err(RssiError { value: -128 }));
}
