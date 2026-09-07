#![cfg(debug_assertions)]

#[path = "../../repose-unlock-core/tests/common/mod.rs"]
mod common;

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use repose_unlock_core::domain::{AuditSessionId, MonoMillis};
use repose_unlock_core::replay::{DurableReplayGuard, MemoryCounterStore, ReplayPolicy};
use repose_unlock_core::state_machine::{Event, TimingPolicy, UnlockState, transition};
use repose_unlock_ipc::{
    RequestNonce, ServiceInstanceId, SessionSelector, encode_consume_or_watch,
};
use repose_unlock_service::ipc_server::test_support::{TestPeer, TestServer};
use repose_unlock_service::permit_broker::{MonotonicClock, PermitBroker};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repose-unlock-peer-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct ManualClock(Arc<AtomicU64>);

impl MonotonicClock for ManualClock {
    fn now(&self) -> MonoMillis {
        MonoMillis::new(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Clone)]
struct SlowClock {
    now: u64,
    delay: Duration,
}

impl MonotonicClock for SlowClock {
    fn now(&self) -> MonoMillis {
        std::thread::sleep(self.delay);
        MonoMillis::new(self.now)
    }
}

fn broker() -> Arc<PermitBroker<MemoryCounterStore, ManualClock>> {
    let binding = common::Vectors::load().binding();
    let (state, _) = transition(
        UnlockState::unlocked(TimingPolicy::new(5_000, 3_000, 1_000).unwrap()),
        Event::SessionLocked { binding },
        MonoMillis::new(2_000),
    )
    .unwrap();
    Arc::new(PermitBroker::new(
        DurableReplayGuard::new(MemoryCounterStore::new(), ReplayPolicy::default()),
        ManualClock(Arc::new(AtomicU64::new(2_001))),
        ServiceInstanceId::try_new([1; 16]).unwrap(),
        state,
        128,
    ))
}

fn assert_closed(mut stream: UnixStream) {
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mut byte = [0_u8; 1];
    assert_eq!(stream.read(&mut byte).unwrap(), 0);
}

#[test]
fn peer_is_rejected_before_server_waits_for_any_frame_bytes() {
    let directory = TestDirectory::new();
    let server = TestServer::start(
        directory.path().join("reject.sock"),
        broker(),
        TestPeer::reject(),
        128,
    )
    .unwrap();
    let stream = UnixStream::connect(server.socket_path()).unwrap();
    let started = Instant::now();
    assert_closed(stream);
    assert!(started.elapsed() < Duration::from_millis(100));
    assert_eq!(server.verification_count(), 1);
}

#[test]
fn non_root_or_wrong_audit_session_is_rejected_after_identity_before_broker() {
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let request = encode_consume_or_watch(RequestNonce::try_new([3; 32]).unwrap(), selector);
    for peer in [
        TestPeer::non_root(binding.audit_session_id()),
        TestPeer::authorization_host(AuditSessionId::new(binding.audit_session_id().get() + 1)),
    ] {
        let directory = TestDirectory::new();
        let server =
            TestServer::start(directory.path().join("peer.sock"), broker(), peer, 128).unwrap();
        let mut stream = UnixStream::connect(server.socket_path()).unwrap();
        stream.write_all(&request).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        assert_closed(stream);
        assert_eq!(server.verification_count(), 1);
    }
}

#[test]
fn active_connection_limit_closes_excess_verified_peer() {
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let directory = TestDirectory::new();
    let service = broker();
    let server = TestServer::start(
        directory.path().join("bounded.sock"),
        Arc::clone(&service),
        TestPeer::authorization_host(binding.audit_session_id()),
        2,
    )
    .unwrap();
    let mut held = Vec::new();
    for value in [4_u8, 5] {
        let mut stream = UnixStream::connect(server.socket_path()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        stream
            .write_all(&encode_consume_or_watch(
                RequestNonce::try_new([value; 32]).unwrap(),
                selector,
            ))
            .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut ack = [0_u8; repose_unlock_ipc::FRAME_LEN];
        stream.read_exact(&mut ack).unwrap();
        held.push(stream);
    }

    let mut excess = UnixStream::connect(server.socket_path()).unwrap();
    excess
        .write_all(&encode_consume_or_watch(
            RequestNonce::try_new([6; 32]).unwrap(),
            selector,
        ))
        .unwrap();
    excess.shutdown(Shutdown::Write).unwrap();
    assert_closed(excess);
    assert_eq!(service.watch_count(), 2);
    drop(held);
}

#[test]
fn slow_peer_verification_cannot_produce_a_response_after_initial_deadline() {
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let directory = TestDirectory::new();
    let service = broker();
    let server = TestServer::start(
        directory.path().join("slow-peer.sock"),
        Arc::clone(&service),
        TestPeer::delayed_authorization_host(
            binding.audit_session_id(),
            Duration::from_millis(120),
        ),
        2,
    )
    .unwrap();
    let mut stream = UnixStream::connect(server.socket_path()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    stream
        .write_all(&encode_consume_or_watch(
            RequestNonce::try_new([7; 32]).unwrap(),
            selector,
        ))
        .unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    assert_closed(stream);
    assert_eq!(service.watch_count(), 0);
}

#[test]
fn slow_broker_clock_cannot_produce_late_watching_ack_and_raii_cancels_watch() {
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let (state, _) = transition(
        UnlockState::unlocked(TimingPolicy::new(5_000, 3_000, 1_000).unwrap()),
        Event::SessionLocked { binding },
        MonoMillis::new(2_000),
    )
    .unwrap();
    let service = Arc::new(PermitBroker::new(
        DurableReplayGuard::new(MemoryCounterStore::new(), ReplayPolicy::default()),
        SlowClock {
            now: 2_001,
            delay: Duration::from_millis(120),
        },
        ServiceInstanceId::try_new([1; 16]).unwrap(),
        state,
        8,
    ));
    let directory = TestDirectory::new();
    let server = TestServer::start(
        directory.path().join("slow-broker.sock"),
        Arc::clone(&service),
        TestPeer::authorization_host(binding.audit_session_id()),
        2,
    )
    .unwrap();
    let mut stream = UnixStream::connect(server.socket_path()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    stream
        .write_all(&encode_consume_or_watch(
            RequestNonce::try_new([8; 32]).unwrap(),
            selector,
        ))
        .unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    assert_closed(stream);
    assert_eq!(service.watch_count(), 0);
}
