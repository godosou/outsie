use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

pub const LAUNCHD_SOCKET_KEY: &str = "ConsumeSocket";

#[derive(Debug)]
pub enum ProductionRunError {
    UnsupportedPlatform,
    InvalidInheritedSocket,
    Io(io::Error),
    PeerVerifier,
}

impl Display for ProductionRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("unlock service is supported only on macOS")
            }
            Self::InvalidInheritedSocket => {
                formatter.write_str("launchd did not provide the required protected socket")
            }
            Self::Io(_) => formatter.write_str("unlock service launchd socket failed"),
            Self::PeerVerifier => formatter.write_str("unlock service peer verifier failed closed"),
        }
    }
}

impl Error for ProductionRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ProductionRunError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn run_production() -> Result<(), ProductionRunError> {
    Err(ProductionRunError::UnsupportedPlatform)
}

#[cfg(target_os = "macos")]
pub fn run_production() -> Result<(), ProductionRunError> {
    platform::run()
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::CStr;
    use std::fs;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use super::{LAUNCHD_SOCKET_KEY, ProductionRunError};
    use crate::ipc_server::{PRODUCTION_SOCKET_PATH, ProductionConnectionServer, ServerError};

    const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(5);
    const CONNECTION_DRAIN_DEADLINE: Duration = Duration::from_millis(150);
    const OWNER_ONLY_MODE_MASK: u32 = 0o177;
    const OWNER_READ_WRITE: u32 = 0o600;
    const DIRECTORY_OWNER_READ_WRITE_EXECUTE: u32 = 0o700;
    const DIRECTORY_UNTRUSTED_WRITE_MASK: u32 = 0o022;
    const LAUNCHD_KEY_C: &[u8] = b"ConsumeSocket\0";

    static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);

    pub(super) fn run() -> Result<(), ProductionRunError> {
        debug_assert_eq!(
            &LAUNCHD_KEY_C[..LAUNCHD_KEY_C.len() - 1],
            LAUNCHD_SOCKET_KEY.as_bytes()
        );
        signal_sys::install()?;
        let listener = activate_listener()?;
        validate_inherited_listener(&listener, Path::new(PRODUCTION_SOCKET_PATH), 0, true)?;
        configure_inherited_listener(&listener)?;
        serve_offline_until_termination(listener)
    }

    fn serve_offline_until_termination(listener: UnixListener) -> Result<(), ProductionRunError> {
        let server = ProductionConnectionServer::new().map_err(|error| match error {
            ServerError::PeerInitialization => ProductionRunError::PeerVerifier,
            ServerError::Io(error) => ProductionRunError::Io(error),
            ServerError::InvalidConfiguration => ProductionRunError::InvalidInheritedSocket,
        })?;
        while !TERMINATION_REQUESTED.load(Ordering::Acquire) {
            match fd_sys::wait_for_accept(listener.as_raw_fd(), ACCEPT_POLL_INTERVAL) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(ProductionRunError::Io(error)),
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = server.dispatch_offline(stream);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(ProductionRunError::Io(error)),
            }
        }

        server.begin_shutdown();
        let drain_deadline = Instant::now() + CONNECTION_DRAIN_DEADLINE;
        while server.active_connections() != 0 && Instant::now() < drain_deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    fn activate_listener() -> Result<UnixListener, ProductionRunError> {
        let key = CStr::from_bytes_with_nul(LAUNCHD_KEY_C)
            .map_err(|_| ProductionRunError::InvalidInheritedSocket)?;
        let mut descriptors = launchd_sys::activate_owned(key)?;
        if descriptors.len() != 1 {
            return Err(ProductionRunError::InvalidInheritedSocket);
        }
        Ok(UnixListener::from(
            descriptors.pop().expect("exactly one descriptor checked"),
        ))
    }

    fn validate_inherited_listener(
        listener: &UnixListener,
        expected_path: &Path,
        expected_uid: u32,
        require_owner_only: bool,
    ) -> Result<(), ProductionRunError> {
        let socket_type = fd_sys::socket_type(listener.as_raw_fd())?;
        if socket_type != libc::SOCK_STREAM
            || !fd_sys::is_accepting(listener.as_raw_fd())?
            || listener.local_addr()?.as_pathname() != Some(expected_path)
        {
            return Err(ProductionRunError::InvalidInheritedSocket);
        }
        let parent = expected_path
            .parent()
            .ok_or(ProductionRunError::InvalidInheritedSocket)?;
        let parent_metadata = fs::symlink_metadata(parent)?;
        let parent_mode = parent_metadata.mode();
        if !parent_metadata.file_type().is_dir()
            || parent_metadata.uid() != expected_uid
            || (parent_mode & DIRECTORY_OWNER_READ_WRITE_EXECUTE)
                != DIRECTORY_OWNER_READ_WRITE_EXECUTE
            || (parent_mode & DIRECTORY_UNTRUSTED_WRITE_MASK) != 0
        {
            return Err(ProductionRunError::InvalidInheritedSocket);
        }
        let metadata = fs::symlink_metadata(expected_path)?;
        let mode = metadata.mode();
        if !metadata.file_type().is_socket()
            || metadata.uid() != expected_uid
            || (mode & OWNER_READ_WRITE) != OWNER_READ_WRITE
            || (require_owner_only && (mode & OWNER_ONLY_MODE_MASK) != 0)
        {
            return Err(ProductionRunError::InvalidInheritedSocket);
        }
        Ok(())
    }

    fn configure_inherited_listener(listener: &UnixListener) -> Result<(), ProductionRunError> {
        listener.set_nonblocking(true)?;
        fd_sys::set_close_on_exec(listener.as_raw_fd())?;
        Ok(())
    }

    #[allow(unsafe_code)]
    mod launchd_sys {
        use std::ffi::{CStr, c_char, c_int, c_void};
        use std::io;
        use std::os::fd::{FromRawFd, OwnedFd};
        use std::ptr;

        use super::ProductionRunError;

        unsafe extern "C" {
            fn launch_activate_socket(
                name: *const c_char,
                fds: *mut *mut c_int,
                count: *mut usize,
            ) -> c_int;
        }

        pub(super) fn activate_owned(name: &CStr) -> Result<Vec<OwnedFd>, ProductionRunError> {
            let mut raw_fds = ptr::null_mut();
            let mut count = 0_usize;
            // SAFETY: `name` is NUL-terminated and the out pointers name initialized
            // caller storage. No raw pointer crosses this module's safe API.
            let status = unsafe { launch_activate_socket(name.as_ptr(), &mut raw_fds, &mut count) };
            activation_result(status, (raw_fds, count), |(raw_fds, count)| {
                adopt_successful_result(raw_fds, count)
            })
        }

        fn activation_result<T, R>(
            status: c_int,
            outputs: T,
            on_success: impl FnOnce(T) -> Result<R, ProductionRunError>,
        ) -> Result<R, ProductionRunError> {
            if status != 0 {
                // The API transfers its allocation only on success. The out
                // values are unspecified on failure, so do not inspect them or
                // invoke any output-adoption code.
                return Err(ProductionRunError::Io(io::Error::from_raw_os_error(status)));
            }
            on_success(outputs)
        }

        fn adopt_successful_result(
            raw_fds: *mut c_int,
            count: usize,
        ) -> Result<Vec<OwnedFd>, ProductionRunError> {
            if (count == 0) != raw_fds.is_null() {
                if !raw_fds.is_null() {
                    release_descriptors(raw_fds, count);
                }
                return Err(ProductionRunError::InvalidInheritedSocket);
            }
            if count == 0 {
                return Ok(Vec::new());
            }
            // SAFETY: a successful launch_activate_socket returns `count` initialized
            // descriptors in a malloc-owned array. Copy before releasing the array.
            let descriptors = unsafe { std::slice::from_raw_parts(raw_fds, count) }.to_vec();
            // SAFETY: this is the unique pointer returned by launchd for the caller.
            unsafe { libc::free(raw_fds.cast::<c_void>()) };
            if descriptors.iter().any(|descriptor| *descriptor < 0) {
                for descriptor in descriptors
                    .into_iter()
                    .filter(|descriptor| *descriptor >= 0)
                {
                    // SAFETY: each nonnegative descriptor was transferred by launchd
                    // and has not yet been wrapped or closed.
                    drop(unsafe { OwnedFd::from_raw_fd(descriptor) });
                }
                return Err(ProductionRunError::InvalidInheritedSocket);
            }
            Ok(descriptors
                .into_iter()
                .map(|descriptor| {
                    // SAFETY: launchd transfers one owned reference for every returned fd.
                    unsafe { OwnedFd::from_raw_fd(descriptor) }
                })
                .collect())
        }

        fn release_descriptors(raw_fds: *mut c_int, count: usize) {
            if count != 0 {
                // SAFETY: liblaunch returned `count` initialized descriptor values
                // in this array. Each nonnegative descriptor is closed once.
                for descriptor in unsafe { std::slice::from_raw_parts(raw_fds, count) } {
                    if *descriptor >= 0 {
                        // SAFETY: this adopts one still-owned liblaunch descriptor.
                        drop(unsafe { OwnedFd::from_raw_fd(*descriptor) });
                    }
                }
            }
            // SAFETY: liblaunch transfers one malloc-owned array to the caller.
            unsafe { libc::free(raw_fds.cast::<c_void>()) };
        }

        #[cfg(test)]
        mod tests {
            use std::ptr::NonNull;
            use std::sync::atomic::{AtomicBool, Ordering};

            use super::activation_result;

            #[test]
            fn failed_activation_never_touches_bogus_outputs() {
                let touched = AtomicBool::new(false);
                let bogus = (NonNull::<libc::c_int>::dangling().as_ptr(), usize::MAX);
                let result: Result<(), _> = activation_result(libc::EINVAL, bogus, |_| {
                    touched.store(true, Ordering::Release);
                    Ok(())
                });
                assert!(result.is_err());
                assert!(!touched.load(Ordering::Acquire));
            }
        }
    }

    #[allow(unsafe_code)]
    mod fd_sys {
        use std::ffi::c_int;
        use std::io;

        pub(super) fn socket_type(fd: c_int) -> io::Result<c_int> {
            socket_integer_option(fd, libc::SO_TYPE)
        }

        pub(super) fn is_accepting(fd: c_int) -> io::Result<bool> {
            unsafe extern "C" {
                fn repose_listener_is_accepting(socket_fd: c_int) -> c_int;
            }
            // SAFETY: the C probe only borrows the descriptor and returns a scalar.
            match unsafe { repose_listener_is_accepting(fd) } {
                1 => Ok(true),
                0 => Ok(false),
                _ => Err(io::Error::other("listener state could not be verified")),
            }
        }

        fn socket_integer_option(fd: c_int, option: c_int) -> io::Result<c_int> {
            let mut socket_type = 0;
            let mut length = std::mem::size_of::<c_int>() as libc::socklen_t;
            // SAFETY: the output points to one correctly sized integer and the fd is borrowed.
            let status = unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    option,
                    (&mut socket_type as *mut c_int).cast(),
                    &mut length,
                )
            };
            if status != 0 {
                return Err(io::Error::last_os_error());
            }
            if length as usize != std::mem::size_of::<c_int>() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid socket type length",
                ));
            }
            Ok(socket_type)
        }

        #[cfg(test)]
        pub(super) fn duplicate_stream_as_listener(
            stream: &std::os::unix::net::UnixStream,
        ) -> io::Result<std::os::unix::net::UnixListener> {
            use std::os::fd::{AsRawFd, FromRawFd};

            // SAFETY: dup creates a new independently owned descriptor.
            let duplicate = unsafe { libc::dup(stream.as_raw_fd()) };
            if duplicate < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: this wrapper takes the newly duplicated descriptor. The
            // validation path uses only descriptor-generic socket operations.
            Ok(unsafe { std::os::unix::net::UnixListener::from_raw_fd(duplicate) })
        }

        pub(super) fn set_close_on_exec(fd: c_int) -> io::Result<()> {
            // SAFETY: F_GETFD only reads flags for the borrowed descriptor.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: F_SETFD updates descriptor flags and does not take ownership.
            if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn wait_for_accept(
            fd: c_int,
            maximum_wait: std::time::Duration,
        ) -> io::Result<bool> {
            let mut descriptor = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let timeout_ms =
                i32::try_from(maximum_wait.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX);
            // SAFETY: the pointer names exactly one initialized pollfd for the call.
            let result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
            if result < 0 {
                return Err(io::Error::last_os_error());
            }
            if result == 0 {
                return Ok(false);
            }
            if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(io::Error::other("inherited listener poll failed"));
            }
            Ok(descriptor.revents & libc::POLLIN != 0)
        }

        #[cfg(test)]
        pub(super) fn descriptor_flags(fd: c_int) -> io::Result<c_int> {
            // SAFETY: F_GETFD only reads flags for the borrowed descriptor.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if flags < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(flags)
            }
        }

        #[cfg(test)]
        pub(super) fn status_flags(fd: c_int) -> io::Result<c_int> {
            // SAFETY: F_GETFL only reads flags for the borrowed descriptor.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(flags)
            }
        }

        #[cfg(test)]
        pub(super) fn effective_uid() -> u32 {
            // SAFETY: geteuid has no preconditions or ownership effects.
            unsafe { libc::geteuid() }
        }
    }

    #[allow(unsafe_code)]
    mod signal_sys {
        use std::ffi::c_int;
        use std::io;
        use std::sync::atomic::Ordering;

        extern "C" fn request_termination(_signal: c_int) {
            super::TERMINATION_REQUESTED.store(true, Ordering::Release);
        }

        pub(super) fn install() -> io::Result<()> {
            for signal in [libc::SIGTERM, libc::SIGINT] {
                // SAFETY: the handler only stores to a lock-free process-lifetime atomic.
                let prior =
                    unsafe { libc::signal(signal, request_termination as libc::sighandler_t) };
                if prior == libc::SIG_ERR {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use std::fs;
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::fs::symlink;
        use std::os::unix::net::{UnixListener, UnixStream};
        use std::path::PathBuf;
        use std::time::{SystemTime, UNIX_EPOCH};

        use super::{
            LAUNCHD_KEY_C, LAUNCHD_SOCKET_KEY, configure_inherited_listener, fd_sys,
            validate_inherited_listener,
        };

        struct TempSocket {
            directory: PathBuf,
            path: PathBuf,
        }

        impl TempSocket {
            fn new() -> Self {
                static NEXT_SOCKET: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(0);
                let sequence = NEXT_SOCKET.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let unique = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock after epoch")
                    .as_nanos();
                let directory = PathBuf::from(format!(
                    "/tmp/rld-{}-{unique}-{sequence}",
                    std::process::id()
                ));
                fs::create_dir(&directory).expect("create protected socket directory");
                fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                    .expect("protect socket directory");
                let path = directory.join("consume.sock");
                Self { directory, path }
            }
        }

        impl Drop for TempSocket {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.path);
                let _ = fs::remove_dir(&self.directory);
            }
        }

        #[test]
        fn launchd_key_is_fixed_and_nul_terminated() {
            assert_eq!(LAUNCHD_SOCKET_KEY, "ConsumeSocket");
            assert_eq!(LAUNCHD_KEY_C, b"ConsumeSocket\0");
        }

        #[test]
        fn owned_listener_contract_checks_shape_path_mode_and_fd_flags() {
            let socket = TempSocket::new();
            let listener = UnixListener::bind(&socket.path).expect("bind temporary socket");
            fs::set_permissions(&socket.path, fs::Permissions::from_mode(0o600))
                .expect("protect temporary socket");
            configure_inherited_listener(&listener).expect("configure listener");

            validate_inherited_listener(&listener, &socket.path, fd_sys::effective_uid(), true)
                .expect("valid inherited listener");

            let descriptor_flags =
                fd_sys::descriptor_flags(listener.as_raw_fd()).expect("descriptor flags");
            let status_flags = fd_sys::status_flags(listener.as_raw_fd()).expect("status flags");
            assert_ne!(descriptor_flags & libc::FD_CLOEXEC, 0);
            assert_ne!(status_flags & libc::O_NONBLOCK, 0);
            assert_eq!(
                fd_sys::socket_type(listener.as_raw_fd()).expect("socket type"),
                libc::SOCK_STREAM
            );
            assert!(fd_sys::is_accepting(listener.as_raw_fd()).expect("accepting listener"));
            assert!(
                validate_inherited_listener(
                    &listener,
                    &PathBuf::from("/tmp/repose-launchd-wrong.sock"),
                    fd_sys::effective_uid(),
                    true,
                )
                .is_err()
            );
        }

        #[test]
        fn writable_or_symlinked_parent_directory_is_rejected() {
            let socket = TempSocket::new();
            let listener = UnixListener::bind(&socket.path).expect("bind temporary socket");
            fs::set_permissions(&socket.path, fs::Permissions::from_mode(0o600))
                .expect("protect socket");
            fs::set_permissions(&socket.directory, fs::Permissions::from_mode(0o722))
                .expect("make parent writable");
            assert!(
                validate_inherited_listener(
                    &listener,
                    &socket.path,
                    fd_sys::effective_uid(),
                    true,
                )
                .is_err()
            );
            fs::set_permissions(&socket.directory, fs::Permissions::from_mode(0o700))
                .expect("restore parent");

            drop(listener);
            fs::remove_file(&socket.path).expect("remove bound socket");
            let real_parent = socket.directory.with_extension("real");
            fs::rename(&socket.directory, &real_parent).expect("move real parent");
            symlink(&real_parent, &socket.directory).expect("symlink parent");
            let linked_path = socket.directory.join("consume.sock");
            let linked_listener = UnixListener::bind(&linked_path).expect("bind through symlink");
            fs::set_permissions(
                real_parent.join("consume.sock"),
                fs::Permissions::from_mode(0o600),
            )
            .expect("protect linked socket");
            assert!(
                validate_inherited_listener(
                    &linked_listener,
                    &linked_path,
                    fd_sys::effective_uid(),
                    true,
                )
                .is_err()
            );
            drop(linked_listener);
            fs::remove_file(real_parent.join("consume.sock")).expect("remove linked socket");
            fs::remove_file(&socket.directory).expect("remove parent symlink");
            fs::rename(&real_parent, &socket.directory).expect("restore parent for cleanup");
        }

        #[test]
        fn connected_unix_stream_is_not_accepted_as_inherited_listener() {
            let socket = TempSocket::new();
            let listener = UnixListener::bind(&socket.path).expect("bind temporary socket");
            fs::set_permissions(&socket.path, fs::Permissions::from_mode(0o600))
                .expect("protect socket");
            let _client = UnixStream::connect(&socket.path).expect("connect client");
            let (accepted, _) = listener.accept().expect("accept client");
            let disguised = fd_sys::duplicate_stream_as_listener(&accepted).expect("duplicate fd");
            assert!(!fd_sys::is_accepting(disguised.as_raw_fd()).expect("not accepting"));
            assert!(
                validate_inherited_listener(
                    &disguised,
                    &socket.path,
                    fd_sys::effective_uid(),
                    true,
                )
                .is_err()
            );
        }
    }
}
