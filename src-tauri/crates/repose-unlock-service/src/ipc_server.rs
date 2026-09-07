use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{RwLock, RwLockReadGuard};
use repose_unlock_core::replay::CounterStore;
use repose_unlock_ipc::{
    ConsumeOrWatch, FRAME_LEN, HEADER_LEN, decode_consume_or_watch, encode_event, encode_keepalive,
    encode_reply, validate_consume_or_watch_header,
};

use crate::peer_identity::{PeerVerifier, VerifiedPeer};
use crate::permit_broker::{BrokerOutcome, MonotonicClock, PermitBroker, WatchWaitError};

pub const PRODUCTION_SOCKET_PATH: &str = "/var/run/ai.repose.unlockd/consume.sock";
pub const INITIAL_IO_DEADLINE: Duration = Duration::from_millis(100);
pub const MAXIMUM_ACTIVE_CONNECTIONS: usize = 128;
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(5);
const PRODUCTION_WATCH_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(1);

fn handle_connection_before<S, C, V>(
    mut stream: UnixStream,
    verifier: &V,
    broker: &PermitBroker<S, C>,
    deadline: Instant,
    shutdown: &ShutdownGate,
    watch_keepalive_interval: Duration,
) where
    S: CounterStore,
    C: MonotonicClock,
    V: PeerVerifier,
{
    if shutdown.is_stopped() {
        return;
    }
    let Some(request) = read_verified_request(&mut stream, verifier, deadline) else {
        return;
    };

    // This read gate is the authorization/ACK linearization boundary. Shutdown
    // takes the write gate, so once it returns no consume or initial response can
    // still be in flight.
    let Some(running) = shutdown.running_guard() else {
        return;
    };
    let outcome = broker.consume_or_watch(request.selector(), request.nonce());
    if ensure_before_deadline(deadline).is_err() {
        return;
    }
    match outcome {
        Ok(BrokerOutcome::Authorized(receipt)) => {
            if receipt.nonce() != request.nonce() {
                return;
            }
            let response = encode_reply::consumed(
                receipt.nonce(),
                receipt.binding(),
                receipt.service_instance(),
            );
            let _ = write_all_until(&mut stream, &response, deadline);
        }
        Ok(BrokerOutcome::Watching(registration)) => {
            let response = encode_reply::watching(
                registration.nonce(),
                registration.binding(),
                registration.service_instance(),
                registration.watch_id(),
            );
            if write_all_until(&mut stream, &response, deadline).is_err() {
                return;
            }
            drop(running);
            wait_for_one_event(
                &mut stream,
                &registration,
                shutdown,
                watch_keepalive_interval,
            );
        }
        Err(_) => {}
    }
}

fn handle_offline_connection_before<V: PeerVerifier>(
    mut stream: UnixStream,
    verifier: &V,
    deadline: Instant,
) {
    // Offline production mode deliberately performs the complete authenticated,
    // bounded read and then closes without emitting a response. It has no permit
    // authority and therefore cannot accidentally authorize an unlock.
    let _ = read_verified_request(&mut stream, verifier, deadline);
}

fn read_verified_request<V: PeerVerifier>(
    stream: &mut UnixStream,
    verifier: &V,
    deadline: Instant,
) -> Option<ConsumeOrWatch> {
    configure_stream(stream).ok()?;
    let peer = verifier.verify(stream).ok()?;
    ensure_before_deadline(deadline).ok()?;
    let mut frame = [0_u8; FRAME_LEN];
    read_exact_until(stream, &mut frame[..HEADER_LEN], deadline).ok()?;
    validate_consume_or_watch_header(&frame[..HEADER_LEN]).ok()?;
    read_exact_until(stream, &mut frame[HEADER_LEN..], deadline).ok()?;
    confirm_request_end(stream, deadline).ok()?;
    let request = decode_consume_or_watch(&frame).ok()?;
    peer_matches_request(peer, request.selector()).then_some(request)
}

fn peer_matches_request(peer: VerifiedPeer, selector: repose_unlock_ipc::SessionSelector) -> bool {
    peer.effective_uid() == 0 && peer.audit_session_id() == selector.audit_session_id()
}

fn wait_for_one_event(
    stream: &mut UnixStream,
    registration: &crate::permit_broker::WatchRegistration,
    shutdown: &ShutdownGate,
    keepalive_interval: Duration,
) {
    let mut next_keepalive = Instant::now() + keepalive_interval;
    loop {
        if shutdown.is_stopped() {
            return;
        }
        let wait =
            WATCH_POLL_INTERVAL.min(next_keepalive.saturating_duration_since(Instant::now()));
        match registration.recv_timeout(wait) {
            Ok(ready) => {
                // A READY edge is non-authoritative, but it must not escape after
                // shutdown either. Do not hold this gate during the idle watch.
                let Some(_running) = shutdown.running_guard() else {
                    return;
                };
                if ready.nonce() != registration.nonce()
                    || ready.binding() != registration.binding()
                    || ready.service_instance() != registration.service_instance()
                    || ready.watch_id() != registration.watch_id()
                {
                    return;
                }
                let event = encode_event(
                    ready.nonce(),
                    ready.binding(),
                    ready.service_instance(),
                    ready.watch_id(),
                );
                let deadline = Instant::now() + INITIAL_IO_DEADLINE;
                let _ = write_all_until(stream, &event, deadline);
                return;
            }
            Err(WatchWaitError::Timeout) if Instant::now() >= next_keepalive => {
                // SHUT_WR is required request framing, so its EOF cannot prove
                // full disconnect. A correlated non-authorizing write detects a
                // closed read half (EPIPE) and reaps the registration/slot.
                let Some(_running) = shutdown.running_guard() else {
                    return;
                };
                let keepalive = encode_keepalive(
                    registration.nonce(),
                    registration.binding(),
                    registration.service_instance(),
                    registration.watch_id(),
                );
                let deadline = Instant::now() + INITIAL_IO_DEADLINE;
                if write_all_until(stream, &keepalive, deadline).is_err() {
                    return;
                }
                next_keepalive = Instant::now() + keepalive_interval;
            }
            Err(WatchWaitError::Timeout) => {}
            Err(WatchWaitError::Invalidated | WatchWaitError::Closed) => return,
        }
    }
}

fn configure_stream(stream: &UnixStream) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    set_no_sigpipe(stream)
}

#[cfg(target_os = "macos")]
fn set_no_sigpipe(stream: &UnixStream) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let status = no_sigpipe_sys::set(stream.as_raw_fd());
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn set_no_sigpipe(_stream: &UnixStream) -> io::Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod no_sigpipe_sys {
    use std::os::raw::c_int;

    unsafe extern "C" {
        fn setsockopt(
            socket: c_int,
            level: c_int,
            option_name: c_int,
            option_value: *const c_int,
            option_len: u32,
        ) -> c_int;
    }

    pub(super) fn set(socket: c_int) -> c_int {
        const SOL_SOCKET: c_int = 0xffff;
        const SO_NOSIGPIPE: c_int = 0x1022;
        let enabled: c_int = 1;
        // SAFETY: the descriptor is borrowed and the option points to a valid integer.
        unsafe {
            setsockopt(
                socket,
                SOL_SOCKET,
                SO_NOSIGPIPE,
                &enabled,
                std::mem::size_of::<c_int>() as u32,
            )
        }
    }
}

fn read_exact_until(
    stream: &mut UnixStream,
    mut output: &mut [u8],
    deadline: Instant,
) -> io::Result<()> {
    while !output.is_empty() {
        ensure_before_deadline(deadline)?;
        match stream.read(output) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "closed frame")),
            Ok(read) => output = &mut output[read..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_ready(stream, libc::POLLIN, deadline)?;
            }
            Err(error) => return Err(error),
        }
    }
    ensure_before_deadline(deadline)
}

fn write_all_until(stream: &mut UnixStream, mut input: &[u8], deadline: Instant) -> io::Result<()> {
    while !input.is_empty() {
        ensure_before_deadline(deadline)?;
        match stream.write(input) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "closed frame")),
            Ok(written) => input = &input[written..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_ready(stream, libc::POLLOUT, deadline)?;
            }
            Err(error) => return Err(error),
        }
    }
    ensure_before_deadline(deadline)
}

fn confirm_request_end(stream: &mut UnixStream, deadline: Instant) -> io::Result<()> {
    let mut extra = [0_u8; 1];
    loop {
        ensure_before_deadline(deadline)?;
        match stream.read(&mut extra) {
            Ok(0) => return Ok(()),
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "trailing request bytes",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_ready(stream, libc::POLLIN, deadline)?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn ensure_before_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(io::ErrorKind::TimedOut, "IPC deadline"))
    } else {
        Ok(())
    }
}

fn wait_ready(stream: &UnixStream, events: i16, deadline: Instant) -> io::Result<()> {
    let mut descriptor = libc::pollfd {
        fd: stream.as_raw_fd(),
        events,
        revents: 0,
    };
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "IPC deadline"));
        }
        let remaining = deadline - now;
        let timeout_ms =
            i32::try_from(remaining.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX);
        descriptor.revents = 0;
        let result = poll_sys::poll(&mut descriptor, timeout_ms);
        if result > 0 {
            return ensure_before_deadline(deadline);
        }
        if result == 0 {
            ensure_before_deadline(deadline)?;
            continue;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        ensure_before_deadline(deadline)?;
    }
}

#[allow(unsafe_code)]
mod poll_sys {
    pub(super) fn poll(descriptor: &mut libc::pollfd, timeout_ms: i32) -> i32 {
        // SAFETY: the pointer names exactly one initialized pollfd for the call.
        unsafe { libc::poll(descriptor, 1, timeout_ms) }
    }
}

#[cfg(target_os = "macos")]
pub(crate) struct ProductionConnectionServer {
    verifier: Arc<crate::peer_identity::MacOsAuthorizationHostVerifier>,
    active: Arc<AtomicUsize>,
    shutdown: Arc<ShutdownGate>,
}

#[cfg(target_os = "macos")]
impl ProductionConnectionServer {
    pub(crate) fn new() -> Result<Self, ServerError> {
        crate::peer_identity::MacOsAuthorizationHostVerifier::new()
            .map(|verifier| Self {
                verifier: Arc::new(verifier),
                active: Arc::new(AtomicUsize::new(0)),
                shutdown: Arc::new(ShutdownGate::new()),
            })
            .map_err(|_| ServerError::PeerInitialization)
    }

    #[allow(dead_code)]
    pub(crate) fn dispatch_accepted<S, C>(
        &self,
        stream: UnixStream,
        broker: Arc<PermitBroker<S, C>>,
    ) -> DispatchOutcome
    where
        S: CounterStore + 'static,
        C: MonotonicClock + 'static,
    {
        let deadline = Instant::now() + INITIAL_IO_DEADLINE;
        let Some(_running) = self.shutdown.running_guard() else {
            drop(stream);
            return DispatchOutcome::ShuttingDown;
        };
        let Some(lease) =
            ActiveConnectionLease::reserve(Arc::clone(&self.active), MAXIMUM_ACTIVE_CONNECTIONS)
        else {
            drop(stream);
            return DispatchOutcome::AtCapacity;
        };
        let verifier = Arc::clone(&self.verifier);
        let shutdown = Arc::clone(&self.shutdown);
        if std::thread::Builder::new()
            .name("repose-unlock-ipc".into())
            .spawn(move || {
                let _lease = lease;
                handle_connection_before(
                    stream,
                    verifier.as_ref(),
                    broker.as_ref(),
                    deadline,
                    shutdown.as_ref(),
                    PRODUCTION_WATCH_KEEPALIVE_INTERVAL,
                );
            })
            .is_ok()
        {
            DispatchOutcome::Dispatched
        } else {
            DispatchOutcome::SpawnFailed
        }
    }

    pub(crate) fn dispatch_offline(&self, stream: UnixStream) -> DispatchOutcome {
        let deadline = Instant::now() + INITIAL_IO_DEADLINE;
        let Some(_running) = self.shutdown.running_guard() else {
            drop(stream);
            return DispatchOutcome::ShuttingDown;
        };
        let Some(lease) =
            ActiveConnectionLease::reserve(Arc::clone(&self.active), MAXIMUM_ACTIVE_CONNECTIONS)
        else {
            drop(stream);
            return DispatchOutcome::AtCapacity;
        };
        let verifier = Arc::clone(&self.verifier);
        if std::thread::Builder::new()
            .name("repose-unlock-ipc-offline".into())
            .spawn(move || {
                let _lease = lease;
                handle_offline_connection_before(stream, verifier.as_ref(), deadline);
            })
            .is_ok()
        {
            DispatchOutcome::Dispatched
        } else {
            DispatchOutcome::SpawnFailed
        }
    }

    #[must_use]
    pub(crate) fn active_connections(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn begin_shutdown(&self) {
        self.shutdown.stop();
    }
}

#[cfg(target_os = "macos")]
impl Drop for ProductionConnectionServer {
    fn drop(&mut self) {
        self.shutdown.stop();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchOutcome {
    Dispatched,
    AtCapacity,
    SpawnFailed,
    ShuttingDown,
}

struct ShutdownGate {
    stopped: AtomicBool,
    finalization: RwLock<()>,
}

impl ShutdownGate {
    fn new() -> Self {
        Self {
            stopped: AtomicBool::new(false),
            finalization: RwLock::new(()),
        }
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn running_guard(&self) -> Option<RwLockReadGuard<'_, ()>> {
        let guard = self.finalization.read();
        (!self.is_stopped()).then_some(guard)
    }

    fn stop(&self) {
        let _exclusive = self.finalization.write();
        self.stopped.store(true, Ordering::Release);
    }
}

struct ActiveConnectionLease(Arc<AtomicUsize>);

impl ActiveConnectionLease {
    fn reserve(active: Arc<AtomicUsize>, maximum: usize) -> Option<Self> {
        active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < maximum).then_some(current + 1)
            })
            .ok()
            .map(|_| Self(active))
    }
}

impl Drop for ActiveConnectionLease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
pub enum ServerError {
    Io(io::Error),
    InvalidConfiguration,
    PeerInitialization,
}

impl Display for ServerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("unlock IPC server I/O failed"),
            Self::InvalidConfiguration => {
                formatter.write_str("invalid unlock IPC server configuration")
            }
            Self::PeerInitialization => {
                formatter.write_str("production peer verifier initialization failed")
            }
        }
    }
}

impl Error for ServerError {}

impl From<io::Error> for ServerError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(test)]
mod unit_tests {
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::{ActiveConnectionLease, MAXIMUM_ACTIVE_CONNECTIONS, ShutdownGate, wait_ready};

    #[test]
    fn active_connection_limit_is_exact_and_panic_releases_lease() {
        assert_eq!(MAXIMUM_ACTIVE_CONNECTIONS, 128);
        let active = Arc::new(AtomicUsize::new(0));
        let leases: Vec<_> = (0..MAXIMUM_ACTIVE_CONNECTIONS)
            .map(|_| {
                ActiveConnectionLease::reserve(Arc::clone(&active), MAXIMUM_ACTIVE_CONNECTIONS)
                    .expect("within cap")
            })
            .collect();
        assert!(
            ActiveConnectionLease::reserve(Arc::clone(&active), MAXIMUM_ACTIVE_CONNECTIONS)
                .is_none()
        );
        drop(leases);

        let lease = ActiveConnectionLease::reserve(Arc::clone(&active), MAXIMUM_ACTIVE_CONNECTIONS)
            .expect("slot released");
        let _ = std::panic::catch_unwind(move || {
            let _lease = lease;
            panic!("exercise unwind cleanup");
        });
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn shutdown_waits_for_final_send_gate_and_then_rejects_new_work() {
        let gate = Arc::new(ShutdownGate::new());
        let read_guard = gate.running_guard().expect("initially running");
        let entered = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let thread = {
            let gate = Arc::clone(&gate);
            let entered = Arc::clone(&entered);
            let stopped = Arc::clone(&stopped);
            std::thread::spawn(move || {
                entered.store(true, Ordering::Release);
                gate.stop();
                stopped.store(true, Ordering::Release);
            })
        };
        while !entered.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        assert!(!stopped.load(Ordering::Acquire));
        drop(read_guard);
        thread.join().expect("shutdown thread");
        assert!(gate.running_guard().is_none());
    }

    #[test]
    fn readiness_timeout_never_rounds_down_before_absolute_deadline() {
        let (stream, _peer) = UnixStream::pair().expect("socket pair");
        stream.set_nonblocking(true).expect("nonblocking");
        let deadline = Instant::now() + Duration::from_micros(1_100);
        let error = wait_ready(&stream, libc::POLLIN, deadline).expect_err("must time out");
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(Instant::now() >= deadline);
    }
}

#[cfg(debug_assertions)]
pub mod test_support {
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use repose_unlock_core::domain::AuditSessionId;
    use repose_unlock_core::replay::CounterStore;

    use super::{ActiveConnectionLease, ServerError, ShutdownGate, handle_connection_before};
    use crate::peer_identity::{PeerIdentityError, PeerVerifier, VerifiedPeer};
    use crate::permit_broker::{MonotonicClock, PermitBroker};

    const TEST_WATCH_KEEPALIVE_INTERVAL: Duration = Duration::from_millis(10);

    #[derive(Debug, Clone, Copy)]
    pub enum TestPeer {
        AuthorizationHost(AuditSessionId),
        DelayedAuthorizationHost(AuditSessionId, Duration),
        NonRoot(AuditSessionId),
        Reject,
    }

    impl TestPeer {
        #[must_use]
        pub const fn authorization_host(audit_session_id: AuditSessionId) -> Self {
            Self::AuthorizationHost(audit_session_id)
        }

        #[must_use]
        pub const fn non_root(audit_session_id: AuditSessionId) -> Self {
            Self::NonRoot(audit_session_id)
        }

        #[must_use]
        pub const fn delayed_authorization_host(
            audit_session_id: AuditSessionId,
            delay: Duration,
        ) -> Self {
            Self::DelayedAuthorizationHost(audit_session_id, delay)
        }

        #[must_use]
        pub const fn reject() -> Self {
            Self::Reject
        }
    }

    #[derive(Clone)]
    struct ExactTestVerifier {
        peer: TestPeer,
        calls: Arc<AtomicUsize>,
    }

    impl PeerVerifier for ExactTestVerifier {
        fn verify(
            &self,
            _stream: &std::os::unix::net::UnixStream,
        ) -> Result<VerifiedPeer, PeerIdentityError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.peer {
                TestPeer::AuthorizationHost(audit_session_id) => {
                    Ok(VerifiedPeer::for_test(0, audit_session_id, 100, 1))
                }
                TestPeer::DelayedAuthorizationHost(audit_session_id, delay) => {
                    thread::sleep(delay);
                    Ok(VerifiedPeer::for_test(0, audit_session_id, 100, 1))
                }
                TestPeer::NonRoot(audit_session_id) => {
                    Ok(VerifiedPeer::for_test(501, audit_session_id, 100, 1))
                }
                TestPeer::Reject => Err(PeerIdentityError::Rejected),
            }
        }
    }

    pub struct TestServer<S, C> {
        socket_path: PathBuf,
        stop: Arc<AtomicBool>,
        connection_shutdown: Arc<ShutdownGate>,
        accept_thread: Option<JoinHandle<()>>,
        verification_calls: Arc<AtomicUsize>,
        active_connections: Arc<AtomicUsize>,
        _types: std::marker::PhantomData<(S, C)>,
    }

    impl<S, C> TestServer<S, C>
    where
        S: CounterStore + 'static,
        C: MonotonicClock + 'static,
    {
        pub fn start(
            socket_path: PathBuf,
            broker: Arc<PermitBroker<S, C>>,
            peer: TestPeer,
            maximum_connections: usize,
        ) -> Result<Self, ServerError> {
            if maximum_connections == 0
                || socket_path == Path::new(super::PRODUCTION_SOCKET_PATH)
                || !socket_path.starts_with(std::env::temp_dir())
                || socket_path.exists()
            {
                return Err(ServerError::InvalidConfiguration);
            }
            let listener = UnixListener::bind(&socket_path)?;
            listener.set_nonblocking(true)?;
            let stop = Arc::new(AtomicBool::new(false));
            let connection_shutdown = Arc::new(ShutdownGate::new());
            let active = Arc::new(AtomicUsize::new(0));
            let thread_active = Arc::clone(&active);
            let calls = Arc::new(AtomicUsize::new(0));
            let verifier = Arc::new(ExactTestVerifier {
                peer,
                calls: Arc::clone(&calls),
            });
            let thread_stop = Arc::clone(&stop);
            let thread_connection_shutdown = Arc::clone(&connection_shutdown);
            let accept_thread = thread::spawn(move || {
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let deadline = std::time::Instant::now() + super::INITIAL_IO_DEADLINE;
                            let Some(_running) = thread_connection_shutdown.running_guard() else {
                                drop(stream);
                                continue;
                            };
                            let Some(lease) = ActiveConnectionLease::reserve(
                                Arc::clone(&thread_active),
                                maximum_connections,
                            ) else {
                                drop(stream);
                                continue;
                            };
                            let broker = Arc::clone(&broker);
                            let verifier = Arc::clone(&verifier);
                            let connection_shutdown = Arc::clone(&thread_connection_shutdown);
                            let _ = thread::Builder::new()
                                .name("repose-unlock-ipc-test".into())
                                .spawn(move || {
                                    let _lease = lease;
                                    handle_connection_before(
                                        stream,
                                        verifier.as_ref(),
                                        broker.as_ref(),
                                        deadline,
                                        connection_shutdown.as_ref(),
                                        TEST_WATCH_KEEPALIVE_INTERVAL,
                                    );
                                });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(_) => break,
                    }
                }
            });
            Ok(Self {
                socket_path,
                stop,
                connection_shutdown,
                accept_thread: Some(accept_thread),
                verification_calls: calls,
                active_connections: active,
                _types: std::marker::PhantomData,
            })
        }

        #[must_use]
        pub fn socket_path(&self) -> &Path {
            &self.socket_path
        }

        #[must_use]
        pub fn verification_count(&self) -> usize {
            self.verification_calls.load(Ordering::SeqCst)
        }

        #[must_use]
        pub fn active_connection_count(&self) -> usize {
            self.active_connections.load(Ordering::Acquire)
        }
    }

    impl<S, C> Drop for TestServer<S, C> {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            self.connection_shutdown.stop();
            let _ = std::os::unix::net::UnixStream::connect(&self.socket_path);
            if let Some(thread) = self.accept_thread.take() {
                let _ = thread.join();
            }
            let _ = fs::remove_file(&self.socket_path);
        }
    }
}
