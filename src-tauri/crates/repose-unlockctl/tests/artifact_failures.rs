use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use repose_unlockctl::artifact_verify::{
    ArtifactAttestor, ArtifactError, SignaturePolicy, inspect_development_package, sha256_hex,
    verify_apply_artifacts,
};

const VALID_LAUNCHD_PLIST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>ai.repose.unlockd</string>
<key>ProgramArguments</key><array><string>/Library/PrivilegedHelperTools/ai.repose.unlockd</string></array>
<key>UserName</key><string>root</string>
<key>GroupName</key><string>wheel</string>
<key>Umask</key><integer>63</integer>
<key>Sockets</key><dict><key>ConsumeSocket</key><dict>
<key>SockPathName</key><string>/var/run/ai.repose.unlockd/consume.sock</string>
<key>SockType</key><string>stream</string>
<key>SockPassive</key><true/>
<key>SockPathOwner</key><integer>0</integer>
<key>SockPathGroup</key><integer>0</integer>
<key>SockPathMode</key><integer>384</integer>
</dict></dict>
</dict></plist>
"#;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repose-unlockctl-artifacts-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct FakeAttestor {
    reject_signature: bool,
    calls: Vec<&'static str>,
}

impl ArtifactAttestor for FakeAttestor {
    fn verify_plugin_signature(
        &mut self,
        _: &Path,
        _: SignaturePolicy,
    ) -> Result<(), ArtifactError> {
        self.calls.push("plugin-signature");
        if self.reject_signature {
            Err(ArtifactError::Attestation("bad plugin signature".into()))
        } else {
            Ok(())
        }
    }

    fn verify_service_signature(
        &mut self,
        _: &Path,
        _: SignaturePolicy,
    ) -> Result<(), ArtifactError> {
        self.calls.push("service-signature");
        Ok(())
    }
}

fn write_file(path: &Path, bytes: &[u8], mode: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn make_package() -> TempDir {
    let temp = TempDir::new();
    let root = temp.path();
    write_file(
        &root.join("ReposeUnlock.bundle/Contents/Info.plist"),
        b"plist",
        0o644,
    );
    write_file(
        &root.join("ReposeUnlock.bundle/Contents/MacOS/ReposeUnlock"),
        b"plugin",
        0o755,
    );
    write_file(
        &root.join("ReposeUnlock.bundle/Contents/_CodeSignature/CodeResources"),
        b"signature",
        0o644,
    );
    write_file(&root.join("bin/ai.repose.unlockd"), b"service", 0o755);
    write_file(
        &root.join("launchd/ai.repose.unlockd.plist"),
        VALID_LAUNCHD_PLIST,
        0o644,
    );

    let paths = [
        "ReposeUnlock.bundle/Contents/Info.plist",
        "ReposeUnlock.bundle/Contents/MacOS/ReposeUnlock",
        "ReposeUnlock.bundle/Contents/_CodeSignature/CodeResources",
        "bin/ai.repose.unlockd",
        "launchd/ai.repose.unlockd.plist",
    ];
    let mut manifest = String::from("repose-package-v1\nmode=adhoc-development\n");
    for relative in paths {
        let digest = sha256_hex(&fs::read(root.join(relative)).unwrap());
        manifest.push_str(&format!("{digest}  {relative}\n"));
    }
    write_file(&root.join("SHA256SUMS"), manifest.as_bytes(), 0o644);
    temp
}

fn inspect(
    root: &Path,
    attestor: &mut impl ArtifactAttestor,
) -> Result<repose_unlockctl::artifact_verify::InspectedDevelopmentPackage, ArtifactError> {
    inspect_development_package(root, fs::symlink_metadata(root).unwrap().uid(), attestor)
}

use std::os::unix::fs::MetadataExt;

#[test]
fn valid_plan_only_package_checks_hashes_and_development_signatures() {
    let package = make_package();
    let mut attestor = FakeAttestor::default();
    inspect(package.path(), &mut attestor).unwrap();
    assert_eq!(attestor.calls, ["plugin-signature", "service-signature"]);
}

#[test]
fn rejects_hash_signature_owner_and_mode_errors() {
    let package = make_package();
    fs::write(package.path().join("bin/ai.repose.unlockd"), b"tampered").unwrap();
    assert!(matches!(
        inspect(package.path(), &mut FakeAttestor::default()),
        Err(ArtifactError::HashMismatch { .. })
    ));

    let package = make_package();
    let mut signature = FakeAttestor {
        reject_signature: true,
        ..Default::default()
    };
    assert!(inspect(package.path(), &mut signature).is_err());

    let package = make_package();
    assert!(matches!(
        inspect_development_package(
            package.path(),
            fs::symlink_metadata(package.path())
                .unwrap()
                .uid()
                .wrapping_add(1),
            &mut FakeAttestor::default()
        ),
        Err(ArtifactError::WrongOwner { .. })
    ));

    let package = make_package();
    fs::set_permissions(
        package.path().join("launchd/ai.repose.unlockd.plist"),
        fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    assert!(matches!(
        inspect(package.path(), &mut FakeAttestor::default()),
        Err(ArtifactError::UnsafeMode { .. })
    ));
}

#[test]
fn rejects_symlinks_manifest_traversal_and_unexpected_files() {
    let package = make_package();
    let service = package.path().join("bin/ai.repose.unlockd");
    fs::remove_file(&service).unwrap();
    symlink("/bin/sh", &service).unwrap();
    assert!(matches!(
        inspect(package.path(), &mut FakeAttestor::default()),
        Err(ArtifactError::Symlink { .. })
    ));

    let package = make_package();
    let manifest = package.path().join("SHA256SUMS");
    let mut text = fs::read_to_string(&manifest).unwrap();
    text.push_str(&format!("{}  ../escape\n", "0".repeat(64)));
    fs::write(&manifest, text).unwrap();
    assert!(matches!(
        inspect(package.path(), &mut FakeAttestor::default()),
        Err(ArtifactError::UnsafeManifestPath { .. })
    ));

    let package = make_package();
    write_file(&package.path().join("payload.sh"), b"surprise", 0o755);
    assert!(matches!(
        inspect(package.path(), &mut FakeAttestor::default()),
        Err(ArtifactError::UnexpectedEntry { .. })
    ));
}

#[test]
fn a_self_consistent_manifest_cannot_authorize_a_malicious_launchd_template() {
    let package = make_package();
    let plist_path = package.path().join("launchd/ai.repose.unlockd.plist");
    let malicious = String::from_utf8(VALID_LAUNCHD_PLIST.to_vec())
        .unwrap()
        .replace(
            "/Library/PrivilegedHelperTools/ai.repose.unlockd",
            "/tmp/attacker",
        );
    fs::write(&plist_path, malicious).unwrap();

    let manifest_path = package.path().join("SHA256SUMS");
    let old_hash = sha256_hex(VALID_LAUNCHD_PLIST);
    let new_hash = sha256_hex(&fs::read(&plist_path).unwrap());
    let manifest = fs::read_to_string(&manifest_path)
        .unwrap()
        .replace(&old_hash, &new_hash);
    fs::write(manifest_path, manifest).unwrap();

    assert!(matches!(
        inspect(package.path(), &mut FakeAttestor::default()),
        Err(ArtifactError::InvalidLaunchdTemplate { .. })
    ));
}

#[test]
fn signed_apply_policy_rejects_development_package_before_attestation() {
    let package = make_package();
    assert!(matches!(
        verify_apply_artifacts(package.path()),
        Err(ArtifactError::ProductionGateClosed)
    ));
}
