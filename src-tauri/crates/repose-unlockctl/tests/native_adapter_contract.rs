const ADAPTER: &str = include_str!("../native/authorization_db.m");

#[test]
fn adapter_can_name_only_the_fixed_screensaver_and_repose_rights() {
    assert!(ADAPTER.contains("system.login.screensaver"));
    assert!(ADAPTER.contains("ai.repose.unlock"));
    assert!(!ADAPTER.contains("system.login.console"));
    assert_eq!(ADAPTER.matches("AuthorizationRightGet(").count(), 1);
    assert_eq!(ADAPTER.matches("AuthorizationRightSet(").count(), 1);
    assert_eq!(ADAPTER.matches("AuthorizationRightRemove(").count(), 1);
}

#[test]
fn adapter_does_not_request_interaction_or_execute_a_database_editor() {
    assert!(ADAPTER.contains("kAuthorizationFlagDefaults"));
    assert!(!ADAPTER.contains("kAuthorizationFlagInteractionAllowed"));
    assert!(!ADAPTER.contains("security authorizationdb"));
    assert!(!ADAPTER.contains("sqlite"));
    assert!(!ADAPTER.contains("system("));
    assert!(!ADAPTER.contains("popen("));
}

#[test]
fn production_native_mutation_gate_defaults_closed() {
    assert!(ADAPTER.contains("#define REPOSE_AUTHDB_PRODUCTION_MUTATION_ENABLED 0"));
    assert!(ADAPTER.contains("kReposeAdapterProductionGateClosed"));
    assert_eq!(
        ADAPTER
            .matches("if (!REPOSE_AUTHDB_PRODUCTION_MUTATION_ENABLED)")
            .count(),
        2,
        "both Set and Remove must fail before AuthorizationCreate"
    );
}
