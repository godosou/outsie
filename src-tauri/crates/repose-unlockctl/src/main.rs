use repose_unlockctl::cli::{Command, authorize_command, parse_args};
use repose_unlockctl::install_transaction::Status;
use repose_unlockctl::production::{
    apply_production_install, apply_production_repair, apply_production_uninstall, effective_uid,
    inspect_production_status, verify_plan_only_package,
};

fn main() {
    let command = match parse_args(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => exit_with(64, &error.to_string()),
    };
    if let Err(error) = authorize_command(&command, effective_uid()) {
        exit_with(77, &error.to_string());
    }
    if let Err(error) = run(command) {
        exit_with(1, &error);
    }
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Status => {
            print_status(inspect_production_status()?);
            print_task_7_safety_boundary();
        }
        Command::PlanInstall { artifacts } => {
            let package =
                verify_plan_only_package(&artifacts).map_err(|error| error.to_string())?;
            println!("plan=install");
            println!("artifacts={}", package.root().display());
            println!("package-mode=unsigned-plan-only");
            println!(
                "order=verify-sealed-artifacts,disable-existing-policy,plugin,service,socket-directory,launchd,deny-only-health,named-rule,screensaver-policy"
            );
            print_task_7_safety_boundary();
        }
        Command::Install { artifacts } => {
            apply_production_install(&artifacts)?;
        }
        Command::PlanUninstall => {
            let status = inspect_production_status()?;
            println!("plan=uninstall");
            println!("current-status={}", status_name(status));
            println!(
                "order=screensaver-reference,named-rule,bootout,launchd,service,plugin,socket-directory"
            );
            println!("mutation=none");
            print_task_7_safety_boundary();
        }
        Command::Uninstall => {
            apply_production_uninstall()?;
            println!("status=not-installed");
        }
        Command::Repair { backup } => {
            apply_production_repair(&backup)?;
            println!("status=installed-deny-only");
        }
    }
    Ok(())
}

fn print_status(status: Status) {
    println!("status={}", status_name(status));
}

fn print_task_7_safety_boundary() {
    println!("apply-gate=closed-pending-task-8");
    println!("authorization-writes=no-cas-maintenance-window-required");
}

const fn status_name(status: Status) -> &'static str {
    match status {
        Status::NotInstalled => "not-installed",
        Status::InstalledDenyOnly => "installed-deny-only",
        Status::Drifted => "drifted",
    }
}

fn exit_with(code: i32, message: &str) -> ! {
    eprintln!("repose-unlockctl: {message}");
    std::process::exit(code)
}
