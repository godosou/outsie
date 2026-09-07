use super::*;

pub(super) fn component_index(component: Component) -> usize {
    match component {
        Component::Plugin => 0,
        Component::Service => 1,
        Component::LaunchdPlist => 2,
        Component::SocketDirectory => 3,
    }
}

pub(super) const fn component_name(component: Component) -> &'static str {
    match component {
        Component::Plugin => "plugin",
        Component::Service => "service",
        Component::LaunchdPlist => "launchd-plist",
        Component::SocketDirectory => "socket-directory",
    }
}

pub(super) const fn component_kind(component: Component) -> &'static str {
    match component {
        Component::Plugin | Component::SocketDirectory => "directory",
        Component::Service | Component::LaunchdPlist => "regular-file",
    }
}

pub(super) const fn component_mode(component: Component) -> u32 {
    match component {
        Component::Plugin | Component::Service => 0o755,
        Component::LaunchdPlist => 0o644,
        Component::SocketDirectory => 0o700,
    }
}

pub(super) fn component_path(component: Component) -> &'static Path {
    Path::new(match component {
        Component::Plugin => PLUGIN_PATH,
        Component::Service => SERVICE_PATH,
        Component::LaunchdPlist => LAUNCHD_PATH,
        Component::SocketDirectory => SOCKET_DIRECTORY,
    })
}

pub(super) fn component_existing_path(
    component: Component,
) -> Result<Option<PathBuf>, BackendError> {
    let installed = component_path(component);
    let quarantine = quarantine_path(installed)?;
    let installed_present = symlink_metadata_optional(installed)?.is_some();
    let quarantine_present = symlink_metadata_optional(&quarantine)?.is_some();
    match (installed_present, quarantine_present) {
        (true, true) => Err(BackendError::new(format!(
            "both installed and quarantined paths are occupied for {}",
            component_name(component)
        ))),
        (true, false) => Ok(Some(installed.to_path_buf())),
        (false, true) => Ok(Some(quarantine)),
        (false, false) => Ok(None),
    }
}

pub(super) fn quarantine_path(path: &Path) -> Result<PathBuf, BackendError> {
    let parent = path
        .parent()
        .ok_or_else(|| BackendError::new("fixed component has no parent"))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| BackendError::new("fixed component name is invalid"))?;
    Ok(parent.join(format!(".{name}.repose-uninstalling")))
}

pub(super) fn quarantine_verify_and_remove(
    journal: &JournalRecord,
    receipt_bytes: Option<&[u8]>,
    component: Component,
    path: &Path,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), BackendError> {
    let quarantine = quarantine_path(path)?;
    let original_present = symlink_metadata_optional(path)?.is_some();
    let quarantine_present = symlink_metadata_optional(&quarantine)?.is_some();
    if original_present && quarantine_present {
        return Err(BackendError::new(
            "both fixed component and uninstall quarantine are occupied",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| BackendError::new("fixed component has no parent"))?;
    if original_present {
        fs::rename(path, &quarantine).map_err(backend_io)?;
        sync_directory(parent)?;
    } else if !quarantine_present {
        return Ok(());
    }

    if let Err(error) = verify_bound_component_at(
        journal,
        receipt_bytes,
        component,
        &quarantine,
        expected_uid,
        expected_gid,
    ) {
        if symlink_metadata_optional(path)?.is_none() {
            fs::rename(&quarantine, path).map_err(backend_io)?;
            sync_directory(parent)?;
        }
        return Err(error);
    }

    match component {
        Component::Plugin => {
            let parent_handle = open_directory_nofollow(parent)?;
            let name = quarantine
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| BackendError::new("quarantine name is invalid"))?;
            remove_plugin_tree_named_at(&parent_handle, name, expected_uid)?;
        }
        Component::Service | Component::LaunchdPlist => {
            fs::remove_file(&quarantine).map_err(backend_io)?;
            sync_directory(parent)?;
        }
        Component::SocketDirectory => {
            if fs::read_dir(&quarantine)
                .map_err(backend_io)?
                .next()
                .is_some()
            {
                return Err(BackendError::new(
                    "socket quarantine is not empty after service shutdown",
                ));
            }
            fs::remove_dir(&quarantine).map_err(backend_io)?;
            sync_directory(parent)?;
        }
    }
    Ok(())
}

pub(super) fn verify_bound_component_at(
    journal: &JournalRecord,
    receipt_bytes: Option<&[u8]>,
    component: Component,
    path: &Path,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), BackendError> {
    let Some(metadata) = symlink_metadata_optional(path)? else {
        return Ok(());
    };
    let InstallReceiptState::Trusted(expected_fingerprint) = journal.prior_receipt else {
        return Err(BackendError::new(format!(
            "refusing to remove unreceipted component {}",
            path.display()
        )));
    };
    let receipt_bytes = receipt_bytes
        .ok_or_else(|| BackendError::new("install receipt disappeared before component removal"))?;
    let actual_fingerprint: [u8; 32] = Sha256::digest(receipt_bytes).into();
    if InstallReceiptFingerprint::new(actual_fingerprint) != expected_fingerprint {
        return Err(BackendError::new(
            "install receipt changed during component removal",
        ));
    }
    let receipt = InstallReceipt::decode(receipt_bytes)?;
    let expected = &receipt.components[component_index(component)];
    if journal.identities[component_index(component)] != Some(expected.identity) {
        return Err(BackendError::new(
            "journal component identity is not derived from its receipt",
        ));
    }
    verify_component_measurement(
        path,
        &metadata,
        component,
        expected,
        expected_uid,
        expected_gid,
    )
}

pub(super) fn verify_component_measurement(
    path: &Path,
    metadata: &fs::Metadata,
    component: Component,
    expected: &ComponentReceipt,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), BackendError> {
    let actual_identity = FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    let kind_matches = match component {
        Component::Plugin | Component::SocketDirectory => {
            metadata.is_dir() && !metadata.file_type().is_symlink()
        }
        Component::Service | Component::LaunchdPlist => {
            metadata.is_file() && !metadata.file_type().is_symlink()
        }
    };
    if !kind_matches
        || actual_identity != expected.identity
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || expected.uid != expected_uid
        || expected.gid != expected_gid
        || metadata.permissions().mode() & 0o7777 != expected.mode
        || metadata.nlink() != expected.links
    {
        return Err(BackendError::new(format!(
            "component metadata differs from install receipt: {}",
            path.display()
        )));
    }
    let digest = component_digest(path, component)?;
    if digest != expected.digest {
        return Err(BackendError::new(format!(
            "component digest differs from install receipt: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn capture_component_receipt(
    path: &Path,
    component: Component,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<ComponentReceipt, BackendError> {
    let metadata = fs::symlink_metadata(path).map_err(backend_io)?;
    let receipt = ComponentReceipt {
        identity: FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
        kind: component_kind(component).to_owned(),
        uid: expected_uid,
        gid: expected_gid,
        mode: component_mode(component),
        links: metadata.nlink(),
        flags: 0,
        digest: component_digest(path, component)?,
        signing_requirement: "test-pinned-requirement".to_owned(),
        security_policy: InstallReceipt::SECURITY_POLICY.to_owned(),
    };
    verify_component_measurement(
        path,
        &metadata,
        component,
        &receipt,
        expected_uid,
        expected_gid,
    )?;
    Ok(receipt)
}

pub(super) fn component_digest(path: &Path, component: Component) -> Result<String, BackendError> {
    match component {
        Component::Service | Component::LaunchdPlist => {
            let bytes = read_regular_nofollow(path, MAX_COMPONENT_BYTES)?;
            Ok(crate::artifact_verify::sha256_hex(&bytes))
        }
        Component::Plugin => plugin_tree_digest(path),
        Component::SocketDirectory => Ok(crate::artifact_verify::sha256_hex(
            b"repose-socket-directory-v1",
        )),
    }
}

pub(super) fn read_regular_nofollow(path: &Path, maximum: usize) -> Result<Vec<u8>, BackendError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options.open(path).map_err(backend_io)?;
    let before = file.metadata().map_err(backend_io)?;
    if !before.is_file() || before.len() > maximum as u64 || before.nlink() != 1 {
        return Err(BackendError::new("component file shape is invalid"));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(backend_io)?;
    if bytes.len() > maximum {
        return Err(BackendError::new("component file exceeds size limit"));
    }
    let after = file.metadata().map_err(backend_io)?;
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.nlink() != after.nlink()
    {
        return Err(BackendError::new(
            "component file changed while it was measured",
        ));
    }
    Ok(bytes)
}

pub(super) fn plugin_tree_digest(root: &Path) -> Result<String, BackendError> {
    verify_exact_children(root, &["Contents"])?;
    let contents = root.join("Contents");
    verify_exact_children(&contents, &["Info.plist", "MacOS", "_CodeSignature"])?;
    let macos = contents.join("MacOS");
    verify_exact_children(&macos, &["ReposeUnlock"])?;
    let signature = contents.join("_CodeSignature");
    verify_exact_children(&signature, &["CodeResources"])?;
    let mut hasher = Sha256::new();
    for (label, path) in [
        ("Info.plist", contents.join("Info.plist")),
        ("MacOS/ReposeUnlock", macos.join("ReposeUnlock")),
        (
            "_CodeSignature/CodeResources",
            signature.join("CodeResources"),
        ),
    ] {
        let bytes = read_regular_nofollow(&path, MAX_COMPONENT_BYTES)?;
        hasher.update(label.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(super) fn verify_exact_children(path: &Path, expected: &[&str]) -> Result<(), BackendError> {
    let metadata = fs::symlink_metadata(path).map_err(backend_io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BackendError::new(
            "plugin receipt path is not a safe directory",
        ));
    }
    let mut actual = fs::read_dir(path)
        .map_err(backend_io)?
        .map(|entry| {
            entry
                .map_err(backend_io)?
                .file_name()
                .into_string()
                .map_err(|_| BackendError::new("plugin contains a non-UTF-8 entry"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    actual.sort();
    let mut expected = expected
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    expected.sort();
    if actual != expected {
        return Err(BackendError::new(
            "plugin contains unknown or missing entries",
        ));
    }
    Ok(())
}
