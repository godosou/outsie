use super::*;

#[cfg(test)]
pub(super) fn remove_plugin_tree_at(parent: &File, expected_uid: u32) -> Result<(), BackendError> {
    remove_plugin_tree_named_at(parent, "ReposeUnlock.bundle", expected_uid)
}

pub(super) fn remove_plugin_tree_named_at(
    parent: &File,
    bundle_name: &str,
    expected_uid: u32,
) -> Result<(), BackendError> {
    let parent_metadata = parent.metadata().map_err(backend_io)?;
    let expected_gid = if expected_uid == 0 {
        0
    } else {
        parent_metadata.gid()
    };
    validate_tree_directory(&parent_metadata, expected_uid, expected_gid, None)?;
    let Some(bundle) =
        open_child_directory(parent, bundle_name, expected_uid, expected_gid, 0o755)?
    else {
        return Ok(());
    };
    if let Some(contents) =
        open_child_directory(&bundle, "Contents", expected_uid, expected_gid, 0o755)?
    {
        if let Some(macos) =
            open_child_directory(&contents, "MacOS", expected_uid, expected_gid, 0o755)?
        {
            remove_regular_at(&macos, "ReposeUnlock", expected_uid, expected_gid, 0o755)?;
            remove_directory_at(&contents, "MacOS", &macos)?;
        }
        if let Some(signature) = open_child_directory(
            &contents,
            "_CodeSignature",
            expected_uid,
            expected_gid,
            0o755,
        )? {
            remove_regular_at(
                &signature,
                "CodeResources",
                expected_uid,
                expected_gid,
                0o644,
            )?;
            remove_directory_at(&contents, "_CodeSignature", &signature)?;
        }
        remove_regular_at(&contents, "Info.plist", expected_uid, expected_gid, 0o644)?;
        remove_directory_at(&bundle, "Contents", &contents)?;
    }
    remove_directory_at(parent, bundle_name, &bundle)
}

pub(super) fn open_directory_nofollow(path: &Path) -> Result<File, BackendError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    options.open(path).map_err(backend_io)
}

pub(super) fn open_child_directory(
    parent: &File,
    name: &str,
    expected_uid: u32,
    expected_gid: u32,
    expected_mode: u32,
) -> Result<Option<File>, BackendError> {
    let name = fixed_child_name(name)?;
    // SAFETY: `name` is NUL-terminated, the returned descriptor is adopted
    // exactly once, and `parent` remains live for the call.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOENT) {
            return Ok(None);
        }
        return Err(backend_io(error));
    }
    // SAFETY: `openat` returned a new owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    validate_tree_directory(
        &file.metadata().map_err(backend_io)?,
        expected_uid,
        expected_gid,
        Some(expected_mode),
    )?;
    Ok(Some(file))
}

pub(super) fn remove_regular_at(
    parent: &File,
    name: &str,
    expected_uid: u32,
    expected_gid: u32,
    expected_mode: u32,
) -> Result<(), BackendError> {
    let name = fixed_child_name(name)?;
    let Some(status) = stat_child(parent, &name)? else {
        return Ok(());
    };
    if status.st_mode & libc::S_IFMT != libc::S_IFREG
        || status.st_uid != expected_uid
        || status.st_gid != expected_gid
        || status.st_nlink != 1
        || u32::from(status.st_mode & 0o7777) != expected_mode
    {
        return Err(BackendError::new("refusing to unlink unsafe plugin file"));
    }
    // SAFETY: fixed validated child name and live directory descriptor.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(backend_io(io::Error::last_os_error()));
    }
    parent.sync_all().map_err(backend_io)
}

pub(super) fn remove_directory_at(
    parent: &File,
    name: &str,
    child: &File,
) -> Result<(), BackendError> {
    let name = fixed_child_name(name)?;
    let current = stat_child(parent, &name)?
        .ok_or_else(|| BackendError::new("plugin directory changed before unlink"))?;
    let opened = child.metadata().map_err(backend_io)?;
    if current.st_mode & libc::S_IFMT != libc::S_IFDIR
        || current.st_dev != opened.dev() as _
        || current.st_ino != opened.ino() as _
    {
        return Err(BackendError::new(
            "plugin directory identity changed before unlink",
        ));
    }
    child.sync_all().map_err(backend_io)?;
    // SAFETY: identity was revalidated without following symlinks.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        return Err(backend_io(io::Error::last_os_error()));
    }
    parent.sync_all().map_err(backend_io)
}

pub(super) fn stat_child(parent: &File, name: &CStr) -> Result<Option<libc::stat>, BackendError> {
    // SAFETY: zero is a valid initial representation for `stat`, and all
    // pointers are live for the duration of `fstatat`.
    let mut status: libc::stat = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            &mut status,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        return Ok(Some(status));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOENT) {
        Ok(None)
    } else {
        Err(backend_io(error))
    }
}

pub(super) fn fixed_child_name(name: &str) -> Result<CString, BackendError> {
    if name.is_empty() || name.as_bytes().contains(&b'/') {
        return Err(BackendError::new("invalid fixed plugin child name"));
    }
    CString::new(name).map_err(|_| BackendError::new("plugin child name contains NUL"))
}

pub(super) fn validate_tree_directory(
    metadata: &fs::Metadata,
    expected_uid: u32,
    expected_gid: u32,
    expected_mode: Option<u32>,
) -> Result<(), BackendError> {
    let mode = metadata.permissions().mode() & 0o7777;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != expected_uid
        || metadata.gid() != expected_gid
        || expected_mode.is_some_and(|expected| mode != expected)
        || expected_mode.is_none() && mode & 0o022 != 0
    {
        return Err(BackendError::new("unsafe plugin directory"));
    }
    Ok(())
}
