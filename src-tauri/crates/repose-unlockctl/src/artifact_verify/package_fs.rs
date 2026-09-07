use super::*;

pub(super) fn validate_root(root: &Path, expected_uid: u32) -> Result<(), ArtifactError> {
    if !root.is_absolute() || root == Path::new("/") {
        return Err(ArtifactError::UnsafeRoot);
    }
    let metadata = fs::symlink_metadata(root).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ArtifactError::UnsafeRoot);
    }
    check_owner_mode(root, &metadata, expected_uid)?;
    Ok(())
}

pub(super) fn walk_package(
    root: &Path,
    expected_uid: u32,
) -> Result<BTreeSet<String>, ArtifactError> {
    let mut files = BTreeSet::new();
    let known_directories: BTreeSet<&str> = PACKAGE_DIRECTORIES.into_iter().collect();
    let known_files: BTreeSet<&str> = PAYLOAD_PATHS.into_iter().chain([MANIFEST_NAME]).collect();
    for relative_directory in PACKAGE_DIRECTORIES {
        let directory = root.join(relative_directory);
        let directory_metadata = fs::symlink_metadata(&directory).map_err(io_error)?;
        if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
            return Err(ArtifactError::Symlink {
                path: relative_directory.to_owned(),
            });
        }
        check_owner_mode(&directory, &directory_metadata, expected_uid)?;
        let mut entry_count = 0_usize;
        for entry in fs::read_dir(&directory).map_err(io_error)? {
            entry_count += 1;
            if entry_count > 16 {
                return Err(ArtifactError::UnexpectedEntry {
                    path: relative_directory.to_owned(),
                });
            }
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ArtifactError::UnsafeRoot)?;
            let relative = safe_relative_string(relative)?;
            if metadata.file_type().is_symlink() {
                return Err(ArtifactError::Symlink { path: relative });
            }
            if metadata.is_dir() {
                if !known_directories.contains(relative.as_str()) {
                    return Err(ArtifactError::UnexpectedEntry { path: relative });
                }
                check_owner_mode(&path, &metadata, expected_uid)?;
            } else if metadata.is_file() {
                if !known_files.contains(relative.as_str()) {
                    return Err(ArtifactError::UnexpectedEntry { path: relative });
                }
                check_owner_mode(&path, &metadata, expected_uid)?;
                if metadata.nlink() != 1 {
                    return Err(ArtifactError::HardLink { path: relative });
                }
                if metadata.len() > MAX_COMPONENT_BYTES {
                    return Err(ArtifactError::ComponentTooLarge { path: relative });
                }
                files.insert(relative);
            } else {
                return Err(ArtifactError::SpecialFile { path: relative });
            }
        }
    }
    Ok(files)
}

pub(super) fn expected_entries() -> BTreeSet<String> {
    PAYLOAD_PATHS
        .into_iter()
        .chain([MANIFEST_NAME])
        .map(str::to_owned)
        .collect()
}

pub(super) fn safe_relative_string(path: &Path) -> Result<String, ArtifactError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, PathComponent::Normal(_)))
    {
        return Err(ArtifactError::UnsafeManifestPath {
            path: path.display().to_string(),
        });
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ArtifactError::UnsafeManifestPath {
            path: path.display().to_string(),
        })
}

pub(super) fn check_owner_mode(
    path: &Path,
    metadata: &fs::Metadata,
    expected_uid: u32,
) -> Result<(), ArtifactError> {
    if metadata.uid() != expected_uid {
        return Err(ArtifactError::WrongOwner {
            path: path.display().to_string(),
            expected: expected_uid,
            actual: metadata.uid(),
        });
    }
    let mode = metadata.permissions().mode() & 0o7777;
    if mode & UNSAFE_WRITE_BITS != 0 {
        return Err(ArtifactError::UnsafeMode {
            path: path.display().to_string(),
            mode,
        });
    }
    let executable =
        path.ends_with(PLUGIN_EXECUTABLE_RELATIVE_PATH) || path.ends_with(SERVICE_RELATIVE_PATH);
    if metadata.is_file() && executable && mode != 0o755 {
        return Err(ArtifactError::UnsafeMode {
            path: path.display().to_string(),
            mode,
        });
    }
    if metadata.is_file() && !executable && mode != 0o644 {
        return Err(ArtifactError::UnsafeMode {
            path: path.display().to_string(),
            mode,
        });
    }
    if metadata.is_dir() && mode != 0o755 {
        return Err(ArtifactError::UnsafeMode {
            path: path.display().to_string(),
            mode,
        });
    }
    Ok(())
}

pub(super) fn parse_manifest(
    input: &str,
) -> Result<(&str, BTreeMap<String, String>), ArtifactError> {
    let mut lines = input.lines();
    if lines.next() != Some(MANIFEST_VERSION) {
        return Err(ArtifactError::MalformedManifest);
    }
    let mode = lines.next().ok_or(ArtifactError::MalformedManifest)?;
    if mode != DEVELOPMENT_MODE && mode != SIGNED_MODE {
        return Err(ArtifactError::MalformedManifest);
    }
    let mut entries = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            return Err(ArtifactError::MalformedManifest);
        }
        let (digest, relative) = line
            .split_once("  ")
            .ok_or(ArtifactError::MalformedManifest)?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ArtifactError::MalformedManifest);
        }
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, PathComponent::Normal(_)))
        {
            return Err(ArtifactError::UnsafeManifestPath {
                path: relative.to_owned(),
            });
        }
        if entries
            .insert(relative.to_owned(), digest.to_owned())
            .is_some()
        {
            return Err(ArtifactError::DuplicateDigest {
                path: relative.to_owned(),
            });
        }
    }
    Ok((mode, entries))
}

pub(super) fn hash_regular_file(path: &Path, expected_uid: u32) -> Result<String, ArtifactError> {
    let mut options = File::options();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options.open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err(ArtifactError::SpecialFile {
            path: path.display().to_string(),
        });
    }
    if metadata.nlink() != 1 {
        return Err(ArtifactError::HardLink {
            path: path.display().to_string(),
        });
    }
    if metadata.len() > MAX_COMPONENT_BYTES {
        return Err(ArtifactError::ComponentTooLarge {
            path: path.display().to_string(),
        });
    }
    check_owner_mode(path, &metadata, expected_uid)?;
    let initial_identity = (metadata.dev(), metadata.ino(), metadata.len());
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let digest = hasher.finalize();
    let final_metadata = file.metadata().map_err(io_error)?;
    if (
        final_metadata.dev(),
        final_metadata.ino(),
        final_metadata.len(),
    ) != initial_identity
    {
        return Err(ArtifactError::ChangedDuringVerification {
            path: path.display().to_string(),
        });
    }
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    Ok(output)
}

pub(super) fn validate_launchd_template(bytes: &[u8]) -> Result<(), ArtifactError> {
    if bytes.len() > MAX_LAUNCHD_BYTES {
        return Err(ArtifactError::InvalidLaunchdTemplate {
            reason: "plist is too large",
        });
    }
    if bytes != LAUNCHD_TEMPLATE {
        return Err(ArtifactError::InvalidLaunchdTemplate {
            reason: "bytes differ from the fixed canonical template",
        });
    }
    Ok(())
}

pub(super) fn read_bounded_regular_file(
    path: &Path,
    expected_uid: u32,
    maximum: u64,
) -> Result<Vec<u8>, ArtifactError> {
    let mut options = File::options();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options.open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(ArtifactError::SpecialFile {
            path: path.display().to_string(),
        });
    }
    if metadata.len() > maximum {
        return Err(ArtifactError::ManifestTooLarge);
    }
    check_owner_mode(path, &metadata, expected_uid)?;
    let identity = (metadata.dev(), metadata.ino(), metadata.len());
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes).map_err(io_error)?;
    let after = file.metadata().map_err(io_error)?;
    if (after.dev(), after.ino(), after.len()) != identity || bytes.len() as u64 != identity.2 {
        return Err(ArtifactError::ChangedDuringVerification {
            path: path.display().to_string(),
        });
    }
    Ok(bytes)
}

pub(super) fn io_error(error: io::Error) -> ArtifactError {
    ArtifactError::Io(error)
}
