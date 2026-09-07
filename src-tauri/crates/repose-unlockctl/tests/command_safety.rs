use std::path::PathBuf;

use repose_unlockctl::cli::{Command, authorize_command, parse_args};
use repose_unlockctl::production::{
    apply_production_install, apply_production_repair, apply_production_uninstall,
    verify_production_mutation_gate,
};

#[test]
fn mutation_commands_require_the_exact_apply_flag() {
    assert!(parse_args(["install", "--artifacts", "/pkg"]).is_err());
    assert!(parse_args(["uninstall"]).is_err());
    assert!(parse_args(["repair", "--backup", "/backup.plist"]).is_err());
    assert!(parse_args(["install", "--apply", "--artifacts", "/pkg", "extra"]).is_err());

    assert_eq!(
        parse_args(["install", "--artifacts", "/pkg", "--apply"]).unwrap(),
        Command::Install {
            artifacts: PathBuf::from("/pkg")
        }
    );
    assert_eq!(
        parse_args(["uninstall", "--apply"]).unwrap(),
        Command::Uninstall
    );
}

#[test]
fn repair_requires_an_absolute_explicit_backup_path() {
    assert!(parse_args(["repair", "--apply"]).is_err());
    assert!(parse_args(["repair", "--apply", "--backup", "relative.plist"]).is_err());
    assert!(parse_args(["repair", "--apply", "--backup", "/tmp/../policy.plist"]).is_err());
    assert_eq!(
        parse_args(["repair", "--backup", "/safe/policy.plist", "--apply"]).unwrap(),
        Command::Repair {
            backup: PathBuf::from("/safe/policy.plist")
        }
    );
}

#[test]
fn plan_and_status_commands_are_read_only_shapes() {
    assert_eq!(parse_args(["status"]).unwrap(), Command::Status);
    assert_eq!(
        parse_args(["plan-install", "--artifacts", "/pkg"]).unwrap(),
        Command::PlanInstall {
            artifacts: PathBuf::from("/pkg")
        }
    );
    assert_eq!(
        parse_args(["plan-uninstall"]).unwrap(),
        Command::PlanUninstall
    );
}

#[test]
fn broad_root_artifact_target_is_rejected() {
    assert!(parse_args(["plan-install", "--artifacts", "/"]).is_err());
    assert!(parse_args(["install", "--artifacts", "/", "--apply"]).is_err());
}

#[test]
fn effective_root_is_required_only_for_exact_apply_commands() {
    let readonly = [
        Command::Status,
        Command::PlanInstall {
            artifacts: PathBuf::from("/pkg"),
        },
        Command::PlanUninstall,
    ];
    for command in readonly {
        authorize_command(&command, 501).unwrap();
    }

    let mutations = [
        Command::Install {
            artifacts: PathBuf::from("/pkg"),
        },
        Command::Uninstall,
        Command::Repair {
            backup: PathBuf::from("/backup.plist"),
        },
    ];
    for command in mutations {
        assert!(authorize_command(&command, 501).is_err());
        authorize_command(&command, 0).unwrap();
    }
}

#[test]
fn every_production_apply_path_is_hard_gated_until_task_8() {
    let error = verify_production_mutation_gate().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("production apply gate is closed")
    );

    let main_source = include_str!("../src/main.rs");
    assert!(main_source.contains("apply_production_install"));
    assert!(main_source.contains("apply_production_uninstall"));
    assert!(main_source.contains("apply_production_repair"));
}

#[test]
fn read_only_output_discloses_the_closed_gate_and_no_cas_boundary() {
    let main_source = include_str!("../src/main.rs");
    assert_eq!(
        main_source
            .matches("print_task_7_safety_boundary();")
            .count(),
        3
    );
    assert!(main_source.contains("apply-gate=closed-pending-task-8"));
    assert!(main_source.contains("authorization-writes=no-cas-maintenance-window-required"));
}

#[test]
fn public_mutation_facades_fail_before_backend_or_native_adapter_construction() {
    for error in [
        apply_production_install(std::path::Path::new("/definitely/not/opened")).unwrap_err(),
        apply_production_uninstall().unwrap_err(),
        apply_production_repair(std::path::Path::new("/definitely/not/opened")).unwrap_err(),
    ] {
        assert!(error.contains("production apply gate is closed"));
    }

    let library_source = include_str!("../src/lib.rs");
    assert!(library_source.contains("mod authdb;"));
    assert!(!library_source.contains("pub mod authdb;"));
    let production_source = include_str!("../src/production.rs");
    assert!(production_source.contains("struct ProductionBackend"));
    assert!(!production_source.contains("pub struct ProductionBackend"));
    let main_source = include_str!("../src/main.rs");
    assert!(!main_source.contains("ProductionBackend"));
}
