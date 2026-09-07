use super::*;

pub(super) fn ensure_state_directory() -> Result<(), BackendError> {
    let path = Path::new(STATE_DIRECTORY);
    let parent = path
        .parent()
        .ok_or_else(|| BackendError::new("fixed state directory has no parent"))?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(backend_io)?;
    validate_tree_directory(&parent_metadata, 0, 0, None)?;
    match symlink_metadata_optional(path)? {
        Some(metadata) => validate_root_directory(path, &metadata, ROOT_DIRECTORY_MODE),
        None => {
            fs::create_dir(path).map_err(backend_io)?;
            fs::set_permissions(path, fs::Permissions::from_mode(ROOT_DIRECTORY_MODE))
                .map_err(backend_io)?;
            let metadata = fs::symlink_metadata(path).map_err(backend_io)?;
            validate_root_directory(path, &metadata, ROOT_DIRECTORY_MODE)?;
            sync_directory(parent)
        }
    }
}

pub(super) fn write_atomic_root_file(path: &Path, bytes: &[u8]) -> Result<(), BackendError> {
    write_atomic_owned_file(path, bytes, 0, 0)
}

pub(super) fn write_atomic_owned_file(
    path: &Path,
    bytes: &[u8],
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), BackendError> {
    let parent = path
        .parent()
        .ok_or_else(|| BackendError::new("fixed state file has no parent"))?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".transaction.{}.{sequence}.tmp",
        std::process::id()
    ));
    let result = (|| {
        write_new_owned_file(&temporary, bytes, expected_uid, expected_gid)?;
        if let Some(metadata) = symlink_metadata_optional(path)? {
            validate_owned_file(path, &metadata, expected_uid, expected_gid, ROOT_FILE_MODE)?;
        }
        fs::rename(&temporary, path).map_err(backend_io)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn write_new_owned_file(
    path: &Path,
    bytes: &[u8],
    expected_uid: u32,
    expected_gid: u32,
) -> Result<(), BackendError> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(ROOT_FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options.open(path).map_err(backend_io)?;
    file.write_all(bytes).map_err(backend_io)?;
    file.sync_all().map_err(backend_io)?;
    validate_owned_file(
        path,
        &file.metadata().map_err(backend_io)?,
        expected_uid,
        expected_gid,
        ROOT_FILE_MODE,
    )
}

pub(super) fn acquire_owned_lock(
    path: &Path,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<File, BackendError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .mode(ROOT_FILE_MODE)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let lock = options.open(path).map_err(backend_io)?;
    validate_owned_file(
        path,
        &lock.metadata().map_err(backend_io)?,
        expected_uid,
        expected_gid,
        ROOT_FILE_MODE,
    )?;
    // SAFETY: flock operates on this owned descriptor and does not outlive it.
    let result = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        return Err(BackendError::new(
            "another unlockctl transaction holds the lock",
        ));
    }
    Ok(lock)
}

pub(super) fn write_unique_owned_file(
    parent: &Path,
    prefix: &str,
    bytes: &[u8],
    expected_uid: u32,
    expected_gid: u32,
) -> Result<PathBuf, BackendError> {
    if prefix.is_empty()
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(BackendError::new("unique state-file prefix is invalid"));
    }
    for _ in 0..4096 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{prefix}-{}-{sequence}.plist", std::process::id()));
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(ROOT_FILE_MODE)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(backend_io(error)),
        };
        let result = (|| {
            file.write_all(bytes).map_err(backend_io)?;
            file.sync_all().map_err(backend_io)?;
            let metadata = file.metadata().map_err(backend_io)?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.uid() != expected_uid
                || metadata.gid() != expected_gid
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o7777 != ROOT_FILE_MODE
            {
                return Err(BackendError::new("unique state file metadata is unsafe"));
            }
            sync_directory(parent)
        })();
        if let Err(error) = result {
            drop(file);
            let _ = fs::remove_file(&path);
            let _ = sync_directory(parent);
            return Err(error);
        }
        return Ok(path);
    }
    Err(BackendError::new(
        "could not allocate a unique state-file name",
    ))
}

pub(super) fn read_required_root_file(
    path: &Path,
    maximum: usize,
) -> Result<Vec<u8>, BackendError> {
    read_optional_root_file(path, maximum)?
        .ok_or_else(|| BackendError::new(format!("required file is missing: {}", path.display())))
}

pub(super) fn read_optional_root_file(
    path: &Path,
    maximum: usize,
) -> Result<Option<Vec<u8>>, BackendError> {
    read_optional_owned_file(path, maximum, 0, 0)
}

pub(super) fn read_optional_owned_file(
    path: &Path,
    maximum: usize,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<Option<Vec<u8>>, BackendError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(backend_io(error)),
    };
    let metadata = file.metadata().map_err(backend_io)?;
    validate_owned_file(path, &metadata, expected_uid, expected_gid, ROOT_FILE_MODE)?;
    if metadata.len() > maximum as u64 {
        return Err(BackendError::new("root-owned file exceeds size limit"));
    }
    let identity = (metadata.dev(), metadata.ino(), metadata.len());
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes).map_err(backend_io)?;
    let after = file.metadata().map_err(backend_io)?;
    if (after.dev(), after.ino(), after.len()) != identity || bytes.len() as u64 != identity.2 {
        return Err(BackendError::new("root-owned file changed while reading"));
    }
    Ok(Some(bytes))
}

pub(super) fn validate_root_file(path: &Path, metadata: &fs::Metadata) -> Result<(), BackendError> {
    validate_owned_file(path, metadata, 0, 0, ROOT_FILE_MODE)
}

pub(super) fn validate_owned_file(
    path: &Path,
    metadata: &fs::Metadata,
    expected_uid: u32,
    expected_gid: u32,
    expected_mode: u32,
) -> Result<(), BackendError> {
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o7777 != expected_mode
    {
        return Err(BackendError::new(format!(
            "unsafe root state file: {}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn validate_root_directory(
    path: &Path,
    metadata: &fs::Metadata,
    mode: u32,
) -> Result<(), BackendError> {
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.gid() != 0
        || metadata.permissions().mode() & 0o7777 != mode
    {
        return Err(BackendError::new(format!(
            "unsafe root directory: {}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn sync_directory(path: &Path) -> Result<(), BackendError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    options
        .open(path)
        .map_err(backend_io)?
        .sync_all()
        .map_err(backend_io)
}

pub(super) fn symlink_metadata_optional(path: &Path) -> Result<Option<fs::Metadata>, BackendError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(backend_io(error)),
    }
}

pub(super) fn read_install_receipt_snapshot()
-> Result<Option<(Vec<u8>, InstallReceipt)>, BackendError> {
    let Some(bytes) = read_optional_root_file(Path::new(INSTALL_RECEIPT_PATH), MAX_RECEIPT_BYTES)?
    else {
        return Ok(None);
    };
    let receipt = InstallReceipt::decode(&bytes)?;
    Ok(Some((bytes, receipt)))
}
