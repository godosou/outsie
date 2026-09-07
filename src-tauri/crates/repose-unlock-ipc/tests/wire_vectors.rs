use std::fs;
use std::path::PathBuf;

use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch};
use repose_unlock_core::state_machine::SessionBinding;
use repose_unlock_ipc::{
    FRAME_LEN, HEADER_LEN, PAYLOAD_LEN, RequestNonce, ServiceInstanceId, SessionSelector, WatchId,
    decode_consume_or_watch, decode_event, decode_keepalive, decode_reply, encode_consume_or_watch,
    encode_event, encode_keepalive, encode_reply, validate_consume_or_watch_header,
};

fn binding() -> SessionBinding {
    SessionBinding::new(
        LockEpoch::new(0x0102_0304_0506_0708),
        AuditSessionId::new(0x1122_3344),
        ConsoleUid::new(501),
    )
}

fn nonce() -> RequestNonce {
    RequestNonce::try_new([0x55; 32]).unwrap()
}

fn instance() -> ServiceInstanceId {
    ServiceInstanceId::try_new([0xa5; 16]).unwrap()
}

fn selector() -> SessionSelector {
    SessionSelector::new(ConsoleUid::new(501), AuditSessionId::new(0x1122_3344))
}

#[test]
fn consume_or_watch_has_one_exact_big_endian_c_layout() {
    let frame = encode_consume_or_watch(nonce(), selector());
    assert_eq!(frame.len(), FRAME_LEN);
    assert_eq!(&frame[0..4], b"RPUI");
    assert_eq!(frame[4], 1);
    assert_eq!(frame[5], 1);
    assert_eq!(frame[6], 0);
    assert_eq!(frame[7], 0);
    assert_eq!(&frame[8..12], &(PAYLOAD_LEN as u32).to_be_bytes());
    assert_eq!(&frame[12..44], &[0x55; 32]);
    assert_eq!(&frame[44..48], &501_u32.to_be_bytes());
    assert_eq!(&frame[48..52], &0x1122_3344_u32.to_be_bytes());
    assert_eq!(&frame[52..60], &[0; 8]);
    assert_eq!(&frame[60..76], &[0; 16]);
    assert_eq!(&frame[76..84], &[0; 8]);

    let decoded = decode_consume_or_watch(&frame).unwrap();
    assert_eq!(decoded.nonce(), nonce());
    assert_eq!(decoded.selector(), selector());
}

#[test]
fn every_invalid_request_header_or_shape_is_rejected() {
    let valid = encode_consume_or_watch(nonce(), selector());
    for (offset, value) in [(0, b'X'), (4, 2), (5, 0xff), (6, 1), (7, 1)] {
        let mut changed = valid;
        changed[offset] = value;
        assert!(
            decode_consume_or_watch(&changed).is_err(),
            "offset {offset}"
        );
    }
    for declared in [
        0_u32,
        (PAYLOAD_LEN - 1) as u32,
        (PAYLOAD_LEN + 1) as u32,
        u32::MAX,
    ] {
        let mut changed = valid;
        changed[8..12].copy_from_slice(&declared.to_be_bytes());
        assert!(decode_consume_or_watch(&changed).is_err());
    }
    let mut zero_nonce = valid;
    zero_nonce[12..44].fill(0);
    assert!(decode_consume_or_watch(&zero_nonce).is_err());
    let mut request_with_instance = valid;
    request_with_instance[60] = 1;
    assert!(decode_consume_or_watch(&request_with_instance).is_err());
    let mut request_with_watch = valid;
    request_with_watch[83] = 1;
    assert!(decode_consume_or_watch(&request_with_watch).is_err());
    let mut request_with_epoch = valid;
    request_with_epoch[59] = 1;
    assert!(decode_consume_or_watch(&request_with_epoch).is_err());
    assert!(decode_consume_or_watch(&valid[..FRAME_LEN - 1]).is_err());
    let mut trailing = valid.to_vec();
    trailing.push(0);
    assert!(decode_consume_or_watch(&trailing).is_err());
}

#[test]
fn request_header_is_validated_before_any_payload_is_needed() {
    let valid = encode_consume_or_watch(nonce(), selector());
    validate_consume_or_watch_header(&valid[..HEADER_LEN]).unwrap();
    for (offset, value) in [(0, b'X'), (4, 2), (5, 0xff), (6, 1), (7, 1)] {
        let mut header: [u8; HEADER_LEN] = valid[..HEADER_LEN].try_into().unwrap();
        header[offset] = value;
        assert!(validate_consume_or_watch_header(&header).is_err());
    }
    let mut oversized: [u8; HEADER_LEN] = valid[..HEADER_LEN].try_into().unwrap();
    oversized[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(validate_consume_or_watch_header(&oversized).is_err());
    assert!(validate_consume_or_watch_header(&valid[..HEADER_LEN - 1]).is_err());
}

#[test]
fn replies_must_echo_nonce_and_full_session_and_have_valid_status_shape() {
    let consumed = encode_reply::consumed(nonce(), binding(), instance());
    let watching =
        encode_reply::watching(nonce(), binding(), instance(), WatchId::try_new(9).unwrap());
    let denied = encode_reply::denied(nonce(), binding(), instance());

    assert!(
        decode_reply(&consumed, nonce(), selector())
            .unwrap()
            .is_consumed()
    );
    let watching = decode_reply(&watching, nonce(), selector()).unwrap();
    assert!(watching.is_watching());
    assert_eq!(watching.watch_id(), Some(WatchId::try_new(9).unwrap()));
    assert!(
        decode_reply(&denied, nonce(), selector())
            .unwrap()
            .is_denied()
    );

    assert!(
        decode_reply(
            &consumed,
            RequestNonce::try_new([0x56; 32]).unwrap(),
            selector()
        )
        .is_err()
    );
    let other = SessionSelector::new(ConsoleUid::new(7), AuditSessionId::new(8));
    assert!(decode_reply(&consumed, nonce(), other).is_err());

    let mut bad_status = consumed;
    bad_status[6] = 0xff;
    assert!(decode_reply(&bad_status, nonce(), selector()).is_err());
    let mut consumed_with_watch = consumed;
    consumed_with_watch[83] = 1;
    assert!(decode_reply(&consumed_with_watch, nonce(), selector()).is_err());
    let mut watching_without_watch =
        encode_reply::watching(nonce(), binding(), instance(), WatchId::try_new(9).unwrap());
    watching_without_watch[76..84].fill(0);
    assert!(decode_reply(&watching_without_watch, nonce(), selector()).is_err());

    let mut zero_epoch = consumed;
    zero_epoch[52..60].fill(0);
    assert!(decode_reply(&zero_epoch, nonce(), selector()).is_err());
}

#[test]
fn event_is_non_authorizing_and_pinned_to_request_watch_and_service_instance() {
    let watch = WatchId::try_new(9).unwrap();
    let frame = encode_event(nonce(), binding(), instance(), watch);
    let event = decode_event(&frame, nonce(), binding(), instance(), watch).unwrap();
    assert_eq!(event.watch_id(), watch);
    assert_eq!(event.service_instance(), instance());
    assert!(decode_reply(&frame, nonce(), selector()).is_err());
    assert!(
        decode_event(
            &frame,
            nonce(),
            binding(),
            ServiceInstanceId::try_new([0xa6; 16]).unwrap(),
            watch
        )
        .is_err()
    );
    assert!(
        decode_event(
            &frame,
            nonce(),
            binding(),
            instance(),
            WatchId::try_new(10).unwrap()
        )
        .is_err()
    );

    let zero_epoch = SessionBinding::new(
        LockEpoch::new(0),
        binding().audit_session_id(),
        binding().console_uid(),
    );
    let invalid = encode_event(nonce(), zero_epoch, instance(), watch);
    assert!(decode_event(&invalid, nonce(), zero_epoch, instance(), watch).is_err());
}

#[test]
fn watch_keepalive_is_non_authorizing_and_uses_full_watch_correlation() {
    let watch = WatchId::try_new(9).unwrap();
    let frame = encode_keepalive(nonce(), binding(), instance(), watch);
    assert_eq!(frame[5], 2);
    assert_eq!(frame[6], 5);
    assert_eq!(frame[7], 0);
    let keepalive = decode_keepalive(&frame, nonce(), binding(), instance(), watch).unwrap();
    assert_eq!(keepalive.watch_id(), watch);
    assert_eq!(keepalive.service_instance(), instance());
    assert!(decode_reply(&frame, nonce(), selector()).is_err());
    assert!(decode_event(&frame, nonce(), binding(), instance(), watch).is_err());
    assert!(
        decode_keepalive(
            &frame,
            RequestNonce::try_new([0x56; 32]).unwrap(),
            binding(),
            instance(),
            watch,
        )
        .is_err()
    );
    let other_binding = SessionBinding::new(
        LockEpoch::new(binding().lock_epoch().get() + 1),
        binding().audit_session_id(),
        binding().console_uid(),
    );
    assert!(decode_keepalive(&frame, nonce(), other_binding, instance(), watch).is_err());
    assert!(
        decode_keepalive(
            &frame,
            nonce(),
            binding(),
            ServiceInstanceId::try_new([0xa6; 16]).unwrap(),
            watch,
        )
        .is_err()
    );
    assert!(
        decode_keepalive(
            &frame,
            nonce(),
            binding(),
            instance(),
            WatchId::try_new(10).unwrap(),
        )
        .is_err()
    );

    let zero_epoch = SessionBinding::new(
        LockEpoch::new(0),
        binding().audit_session_id(),
        binding().console_uid(),
    );
    let invalid = encode_keepalive(nonce(), zero_epoch, instance(), watch);
    assert!(decode_keepalive(&invalid, nonce(), zero_epoch, instance(), watch).is_err());
}

#[test]
fn c_header_constants_match_rust_wire_layout() {
    let header = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("include/repose_unlock_ipc.h"),
    )
    .unwrap();
    for (name, value) in [
        ("REPOSE_UNLOCK_IPC_FRAME_LEN", FRAME_LEN),
        ("REPOSE_UNLOCK_IPC_HEADER_LEN", HEADER_LEN),
        ("REPOSE_UNLOCK_IPC_PAYLOAD_LEN", PAYLOAD_LEN),
        ("REPOSE_UNLOCK_IPC_NONCE_OFFSET", 12),
        ("REPOSE_UNLOCK_IPC_CONSOLE_UID_OFFSET", 44),
        ("REPOSE_UNLOCK_IPC_AUDIT_SESSION_OFFSET", 48),
        ("REPOSE_UNLOCK_IPC_LOCK_EPOCH_OFFSET", 52),
        ("REPOSE_UNLOCK_IPC_INSTANCE_OFFSET", 60),
        ("REPOSE_UNLOCK_IPC_WATCH_ID_OFFSET", 76),
    ] {
        assert!(
            header.contains(&format!("#define {name} {value}u")),
            "C header missing synchronized {name}={value}"
        );
    }
    for line in [
        "#define REPOSE_UNLOCK_IPC_MAGIC_0 0x52u",
        "#define REPOSE_UNLOCK_IPC_MAGIC_1 0x50u",
        "#define REPOSE_UNLOCK_IPC_MAGIC_2 0x55u",
        "#define REPOSE_UNLOCK_IPC_MAGIC_3 0x49u",
        "#define REPOSE_UNLOCK_IPC_VERSION 1u",
        "#define REPOSE_UNLOCK_IPC_OP_CONSUME_OR_WATCH 1u",
        "#define REPOSE_UNLOCK_IPC_OP_PERMIT_AVAILABLE 2u",
        "#define REPOSE_UNLOCK_IPC_STATUS_REQUEST 0u",
        "#define REPOSE_UNLOCK_IPC_STATUS_CONSUMED 1u",
        "#define REPOSE_UNLOCK_IPC_STATUS_WATCHING 2u",
        "#define REPOSE_UNLOCK_IPC_STATUS_DENIED 3u",
        "#define REPOSE_UNLOCK_IPC_STATUS_EVENT 4u",
        "#define REPOSE_UNLOCK_IPC_STATUS_KEEPALIVE 5u",
        "#define REPOSE_UNLOCK_IPC_REQUEST_REQUIRES_WRITE_HALF_CLOSE 1u",
    ] {
        assert!(header.contains(line), "C header missing `{line}`");
    }
}
