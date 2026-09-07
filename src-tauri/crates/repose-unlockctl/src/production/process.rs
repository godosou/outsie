use super::*;

pub(super) fn fixed_command(
    program: &str,
    arguments: &[&str],
) -> Result<std::process::ExitStatus, BackendError> {
    fixed_command_with_timeout(program, arguments, std::time::Duration::from_secs(5))
}

pub(super) fn fixed_command_with_timeout(
    program: &str,
    arguments: &[&str],
    timeout: std::time::Duration,
) -> Result<std::process::ExitStatus, BackendError> {
    let mut child = Command::new(program)
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(backend_io)?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(backend_io)? {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(BackendError::new(format!(
                "fixed command timed out: {program}"
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub(super) fn classify_bootout_result(
    command_succeeded: bool,
    socket_present: bool,
) -> Result<(), BackendError> {
    if !command_succeeded {
        return Err(BackendError::new(
            "launchd bootout was not authoritatively successful",
        ));
    }
    if socket_present {
        return Err(BackendError::new(
            "launchd socket still exists after service shutdown",
        ));
    }
    Ok(())
}
