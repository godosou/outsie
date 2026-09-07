#![cfg(debug_assertions)]

#[path = "../../repose-unlock-core/tests/common/mod.rs"]
mod common;

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use repose_unlock_core::domain::MonoMillis;
use repose_unlock_core::replay::{DurableReplayGuard, MemoryCounterStore, ReplayPolicy};
use repose_unlock_core::state_machine::{Effect, Event, Permit, UnlockState, transition};
use repose_unlock_ipc::{
    FRAME_LEN, RequestNonce, ServiceInstanceId, SessionSelector, decode_event, decode_keepalive,
    decode_reply, encode_consume_or_watch,
};
use repose_unlock_service::ipc_server::test_support::{TestPeer, TestServer};
use repose_unlock_service::permit_broker::{BrokerOutcome, MonotonicClock, PermitBroker};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repose-unlock-service-{}-{serial}",
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

fn permit_ready(reducer_now: u64) -> (UnlockState, Permit, DurableReplayGuard<MemoryCounterStore>) {
    let (state, response, _) = common::authenticated();
    let guard = DurableReplayGuard::new(MemoryCounterStore::new(), ReplayPolicy::default());
    let committed = guard.commit(response).unwrap();
    let proof = guard.finalize(committed).unwrap();
    let (state, effects) = transition(
        state,
        Event::ChallengeVerified(proof),
        MonoMillis::new(reducer_now),
    )
    .unwrap();
    let [effect]: [Effect; 1] = effects.try_into().unwrap();
    let Effect::CreatePermit(permit) = effect else {
        panic!("expected permit effect");
    };
    (state, permit, guard)
}

#[test]
fn one_hundred_concurrent_invocations_consume_exactly_once() {
    let (state, permit, guard) = permit_ready(2_000);
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let broker = Arc::new(PermitBroker::new(
        guard,
        ManualClock(Arc::new(AtomicU64::new(2_001))),
        ServiceInstanceId::try_new([1; 16]).unwrap(),
        state,
        128,
    ));
    broker.publish(permit).unwrap();
    let barrier = Arc::new(Barrier::new(100));
    let workers: Vec<_> = (1_u8..=100)
        .map(|value| {
            let broker = Arc::clone(&broker);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                matches!(
                    broker.consume_or_watch(selector, RequestNonce::try_new([value; 32]).unwrap(),),
                    Ok(BrokerOutcome::Authorized(_))
                )
            })
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|consumed| *consumed)
            .count(),
        1
    );
    assert_eq!(broker.watch_count(), 0);
}

fn socket_server(
    broker: Arc<PermitBroker<MemoryCounterStore, ManualClock>>,
    directory: &TestDirectory,
) -> TestServer<MemoryCounterStore, ManualClock> {
    TestServer::start(
        directory.path().join("consume.sock"),
        broker,
        TestPeer::authorization_host(common::Vectors::load().binding().audit_session_id()),
        128,
    )
    .unwrap()
}

fn read_frame(stream: &mut UnixStream) -> [u8; FRAME_LEN] {
    let mut frame = [0_u8; FRAME_LEN];
    stream.read_exact(&mut frame).unwrap();
    frame
}

#[test]
fn half_closed_partial_request_survives_idle_then_gets_event_and_fresh_invoke_consumes() {
    let (state, permit, guard) = permit_ready(2_000);
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let broker = Arc::new(PermitBroker::new(
        guard,
        ManualClock(Arc::new(AtomicU64::new(2_001))),
        ServiceInstanceId::try_new([1; 16]).unwrap(),
        state,
        128,
    ));
    let directory = TestDirectory::new();
    let server = socket_server(Arc::clone(&broker), &directory);
    let mut first = UnixStream::connect(server.socket_path()).unwrap();
    first
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let first_nonce = RequestNonce::try_new([0x11; 32]).unwrap();
    let request = encode_consume_or_watch(first_nonce, selector);
    for chunk in request.chunks(7) {
        first.write_all(chunk).unwrap();
    }
    first.shutdown(Shutdown::Write).unwrap();
    let ack = decode_reply(&read_frame(&mut first), first_nonce, selector).unwrap();
    assert!(ack.is_watching());

    thread::sleep(Duration::from_millis(30));
    assert_eq!(broker.watch_count(), 1, "SHUT_WR is not a disconnect");

    broker.publish(permit).unwrap();
    let event_frame = loop {
        let frame = read_frame(&mut first);
        if decode_keepalive(
            &frame,
            first_nonce,
            ack.binding(),
            ack.service_instance(),
            ack.watch_id().unwrap(),
        )
        .is_ok()
        {
            continue;
        }
        break frame;
    };
    decode_event(
        &event_frame,
        first_nonce,
        ack.binding(),
        ack.service_instance(),
        ack.watch_id().unwrap(),
    )
    .unwrap();

    let mut second = UnixStream::connect(server.socket_path()).unwrap();
    second
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let second_nonce = RequestNonce::try_new([0x12; 32]).unwrap();
    second
        .write_all(&encode_consume_or_watch(second_nonce, selector))
        .unwrap();
    second.shutdown(Shutdown::Write).unwrap();
    let consumed = decode_reply(&read_frame(&mut second), second_nonce, selector).unwrap();
    assert!(consumed.is_consumed());
    assert!(broker.is_unlocking());
}

#[test]
fn malformed_and_stalled_frames_close_without_response_within_one_deadline() {
    let (state, _permit, guard) = permit_ready(2_000);
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let broker = Arc::new(PermitBroker::new(
        guard,
        ManualClock(Arc::new(AtomicU64::new(2_001))),
        ServiceInstanceId::try_new([1; 16]).unwrap(),
        state,
        128,
    ));
    let directory = TestDirectory::new();
    let server = socket_server(broker, &directory);
    let valid = encode_consume_or_watch(RequestNonce::try_new([0x21; 32]).unwrap(), selector);

    let mut cases = Vec::new();
    let mut oversized = valid.to_vec();
    oversized[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    cases.push(oversized);
    let mut unknown = valid.to_vec();
    unknown[5] = 0xff;
    cases.push(unknown);
    let mut trailing = valid.to_vec();
    trailing.push(0xaa);
    cases.push(trailing);

    for bytes in cases {
        let mut stream = UnixStream::connect(server.socket_path()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        stream.write_all(&bytes).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut byte = [0_u8; 1];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
    }

    let mut stalled = UnixStream::connect(server.socket_path()).unwrap();
    stalled
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    stalled.write_all(&valid[..9]).unwrap();
    let started = Instant::now();
    let mut byte = [0_u8; 1];
    assert_eq!(stalled.read(&mut byte).unwrap(), 0);
    assert!(started.elapsed() < Duration::from_millis(300));
}

#[test]
fn continuous_partial_progress_never_renews_the_absolute_deadline() {
    let (state, _permit, guard) = permit_ready(2_000);
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let broker = Arc::new(PermitBroker::new(
        guard,
        ManualClock(Arc::new(AtomicU64::new(2_001))),
        ServiceInstanceId::try_new([1; 16]).unwrap(),
        state,
        128,
    ));
    let directory = TestDirectory::new();
    let server = socket_server(broker, &directory);
    let request = encode_consume_or_watch(RequestNonce::try_new([0x31; 32]).unwrap(), selector);
    let mut stream = UnixStream::connect(server.socket_path()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let started = Instant::now();
    let mut successful_writes = 0;
    for byte in &request[..8] {
        match stream.write_all(&[*byte]) {
            Ok(()) => successful_writes += 1,
            Err(_) => break,
        }
        thread::sleep(Duration::from_millis(40));
    }
    let mut response = [0_u8; 1];
    assert_eq!(stream.read(&mut response).unwrap(), 0);
    assert!(successful_writes < 8, "drip feed kept the connection alive");
    assert!(started.elapsed() < Duration::from_millis(260));
}

#[test]
fn service_restart_closes_old_watch_and_new_ack_uses_higher_epoch_and_instance() {
    let (state, _permit, guard) = permit_ready(2_000);
    let binding = common::Vectors::load().binding();
    let replacement = repose_unlock_core::state_machine::SessionBinding::new(
        repose_unlock_core::domain::LockEpoch::new(binding.lock_epoch().get() + 1),
        binding.audit_session_id(),
        binding.console_uid(),
    );
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let broker = Arc::new(PermitBroker::new(
        guard,
        ManualClock(Arc::new(AtomicU64::new(2_001))),
        ServiceInstanceId::try_new([1; 16]).unwrap(),
        state,
        128,
    ));
    let directory = TestDirectory::new();
    let server = socket_server(Arc::clone(&broker), &directory);
    let mut old = UnixStream::connect(server.socket_path()).unwrap();
    old.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let old_nonce = RequestNonce::try_new([0x41; 32]).unwrap();
    old.write_all(&encode_consume_or_watch(old_nonce, selector))
        .unwrap();
    old.shutdown(Shutdown::Write).unwrap();
    let old_ack = decode_reply(&read_frame(&mut old), old_nonce, selector).unwrap();
    assert_eq!(
        old_ack.service_instance(),
        ServiceInstanceId::try_new([1; 16]).unwrap()
    );

    broker
        .restart(
            ServiceInstanceId::try_new([2; 16]).unwrap(),
            Some(replacement),
        )
        .unwrap();
    loop {
        let mut first = [0_u8; 1];
        if old.read(&mut first).unwrap() == 0 {
            break;
        }
        let mut frame = [0_u8; FRAME_LEN];
        frame[0] = first[0];
        old.read_exact(&mut frame[1..]).unwrap();
        decode_keepalive(
            &frame,
            old_nonce,
            old_ack.binding(),
            old_ack.service_instance(),
            old_ack.watch_id().unwrap(),
        )
        .expect("only an already-linearized non-authorizing keepalive may precede EOF");
    }

    let mut fresh = UnixStream::connect(server.socket_path()).unwrap();
    fresh
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let fresh_nonce = RequestNonce::try_new([0x42; 32]).unwrap();
    fresh
        .write_all(&encode_consume_or_watch(fresh_nonce, selector))
        .unwrap();
    fresh.shutdown(Shutdown::Write).unwrap();
    let ack = decode_reply(&read_frame(&mut fresh), fresh_nonce, selector).unwrap();
    assert!(ack.is_watching());
    assert_eq!(ack.binding(), replacement);
    assert_eq!(
        ack.service_instance(),
        ServiceInstanceId::try_new([2; 16]).unwrap()
    );
}

#[test]
fn delayed_trailing_byte_or_missing_half_close_is_rejected_before_consumption() {
    let (state, permit, guard) = permit_ready(2_000);
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let broker = Arc::new(PermitBroker::new(
        guard,
        ManualClock(Arc::new(AtomicU64::new(2_001))),
        ServiceInstanceId::try_new([1; 16]).unwrap(),
        state,
        128,
    ));
    broker.publish(permit).unwrap();
    let directory = TestDirectory::new();
    let server = socket_server(Arc::clone(&broker), &directory);

    let mut no_half_close = UnixStream::connect(server.socket_path()).unwrap();
    no_half_close
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    no_half_close
        .write_all(&encode_consume_or_watch(
            RequestNonce::try_new([0x51; 32]).unwrap(),
            selector,
        ))
        .unwrap();
    let started = Instant::now();
    let mut byte = [0_u8; 1];
    assert_eq!(no_half_close.read(&mut byte).unwrap(), 0);
    assert!(started.elapsed() < Duration::from_millis(300));

    let mut trailing = UnixStream::connect(server.socket_path()).unwrap();
    trailing
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    trailing
        .write_all(&encode_consume_or_watch(
            RequestNonce::try_new([0x52; 32]).unwrap(),
            selector,
        ))
        .unwrap();
    thread::sleep(Duration::from_millis(25));
    trailing.write_all(&[0xaa]).unwrap();
    trailing.shutdown(Shutdown::Write).unwrap();
    assert_eq!(trailing.read(&mut byte).unwrap(), 0);

    assert!(matches!(
        broker.consume_or_watch(selector, RequestNonce::try_new([0x53; 32]).unwrap()),
        Ok(BrokerOutcome::Authorized(_))
    ));
}

#[test]
fn full_close_after_watching_is_reaped_by_bounded_keepalive() {
    let (state, _permit, guard) = permit_ready(2_000);
    let binding = common::Vectors::load().binding();
    let selector = SessionSelector::new(binding.console_uid(), binding.audit_session_id());
    let broker = Arc::new(PermitBroker::new(
        guard,
        ManualClock(Arc::new(AtomicU64::new(2_001))),
        ServiceInstanceId::try_new([1; 16]).unwrap(),
        state,
        128,
    ));
    let directory = TestDirectory::new();
    let server = socket_server(Arc::clone(&broker), &directory);
    let mut stream = UnixStream::connect(server.socket_path()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    stream
        .write_all(&encode_consume_or_watch(
            RequestNonce::try_new([0x61; 32]).unwrap(),
            selector,
        ))
        .unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let _ack = read_frame(&mut stream);
    assert_eq!(broker.watch_count(), 1);
    drop(stream);

    let deadline = Instant::now() + Duration::from_millis(500);
    while (broker.watch_count() != 0 || server.active_connection_count() != 0)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(broker.watch_count(), 0);
    assert_eq!(server.active_connection_count(), 0);
}
