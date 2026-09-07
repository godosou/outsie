fn main() {
    if let Err(error) = repose_unlock_service::run_production() {
        eprintln!("{error}");
        std::process::exit(78);
    }
}
