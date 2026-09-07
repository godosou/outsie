use super::*;

pub struct SystemAttestor;

impl ArtifactAttestor for SystemAttestor {
    fn verify_plugin_signature(
        &mut self,
        bundle: &Path,
        policy: SignaturePolicy,
    ) -> Result<(), ArtifactError> {
        verify_code_signature(bundle, policy)
    }

    fn verify_service_signature(
        &mut self,
        executable: &Path,
        policy: SignaturePolicy,
    ) -> Result<(), ArtifactError> {
        verify_code_signature(executable, policy)
    }
}

pub fn verify_plan_only_package(path: &Path) -> Result<InspectedDevelopmentPackage, ArtifactError> {
    let uid = effective_uid();
    inspect_development_package(path, uid, &mut SystemAttestor)
}

/// Fail-closed apply preflight. This deliberately runs before acquiring the
/// production transaction lock, so the Task 8 signing/measurement gate cannot
/// create even installer state files while it remains closed.
pub fn verify_apply_package_before_mutation(
    path: &Path,
) -> Result<ProductionSealedArtifacts, ArtifactError> {
    verify_apply_artifacts(path)
}

/// Task 7 intentionally ships every production mutation behind one hard gate.
/// Task 8 must replace this only after signed measurement, sealed staging,
/// loaded-image attestation, and an exclusive maintenance precondition exist.
pub fn verify_production_mutation_gate() -> Result<(), ArtifactError> {
    Err(ArtifactError::ProductionGateClosed)
}

/// Read-only production status facade. It never performs journal recovery.
pub fn inspect_production_status() -> Result<Status, String> {
    let mut backend = ProductionBackend::new();
    inspect_status(&mut backend).map_err(|error| error.to_string())
}

/// Fail-closed production install facade. Task 7 returns before constructing
/// the backend or opening the caller's path.
pub fn apply_production_install(path: &Path) -> Result<(), String> {
    verify_apply_package_before_mutation(path).map_err(|error| error.to_string())?;
    let mut backend = ProductionBackend::new();
    install(&mut backend, &InstallRequest::new(path.to_path_buf()))
        .map_err(|error| error.to_string())
}

/// Fail-closed production uninstall facade.
pub fn apply_production_uninstall() -> Result<(), String> {
    verify_production_mutation_gate().map_err(|error| error.to_string())?;
    let mut backend = ProductionBackend::new();
    uninstall(&mut backend).map_err(|error| error.to_string())
}

/// Fail-closed production policy-repair facade.
pub fn apply_production_repair(backup: &Path) -> Result<(), String> {
    verify_production_mutation_gate().map_err(|error| error.to_string())?;
    let mut backend = ProductionBackend::new();
    repair_policy(&mut backend, backup).map_err(|error| error.to_string())
}

pub(super) fn verify_code_signature(
    path: &Path,
    policy: SignaturePolicy,
) -> Result<(), ArtifactError> {
    let status = Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict", "--verbose=2"])
        .arg(path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| ArtifactError::Attestation(error.to_string()))?;
    if !status.success() {
        return Err(ArtifactError::Attestation(
            "codesign verification failed".to_owned(),
        ));
    }
    let output = Command::new("/usr/bin/codesign")
        .arg("-dvv")
        .arg(path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .current_dir("/")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| ArtifactError::Attestation(error.to_string()))?;
    if output.stderr.len() > 64 * 1024 {
        return Err(ArtifactError::Attestation(
            "codesign output exceeded the bound".to_owned(),
        ));
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    match policy {
        SignaturePolicy::DevelopmentAdHoc
            if detail.lines().any(|line| line == "Signature=adhoc") =>
        {
            Ok(())
        }
        SignaturePolicy::DevelopmentAdHoc => Err(ArtifactError::Attestation(
            "development artifact is not ad-hoc signed".to_owned(),
        )),
        SignaturePolicy::PinnedDeveloperId => Err(ArtifactError::ProductionGateClosed),
    }
}
