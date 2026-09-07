fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments == ["--health-check-deny-only"] {
        println!("repose-unlock-service: deny-only-v1");
        return;
    }
    if !arguments.is_empty() {
        eprintln!("usage: repose-unlock-service [--health-check-deny-only]");
        std::process::exit(64);
    }
    if let Err(error) = repose_unlock_service::run_production() {
        eprintln!("{error}");
        std::process::exit(78);
    }
}
