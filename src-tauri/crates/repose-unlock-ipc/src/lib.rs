#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch};
use repose_unlock_core::state_machine::SessionBinding;

pub const HEADER_LEN: usize = 12;
pub const PAYLOAD_LEN: usize = 72;
pub const FRAME_LEN: usize = HEADER_LEN + PAYLOAD_LEN;
pub const REQUEST_NONCE_LEN: usize = 32;
pub const SERVICE_INSTANCE_LEN: usize = 16;

const MAGIC: &[u8; 4] = b"RPUI";
const VERSION: u8 = 1;
const OP_CONSUME_OR_WATCH: u8 = 1;
const OP_PERMIT_AVAILABLE: u8 = 2;
const STATUS_REQUEST: u8 = 0;
const STATUS_CONSUMED: u8 = 1;
const STATUS_WATCHING: u8 = 2;
const STATUS_DENIED: u8 = 3;
const STATUS_EVENT: u8 = 4;
const STATUS_KEEPALIVE: u8 = 5;

const NONCE_OFFSET: usize = 12;
const CONSOLE_UID_OFFSET: usize = 44;
const AUDIT_SESSION_OFFSET: usize = 48;
const LOCK_EPOCH_OFFSET: usize = 52;
const INSTANCE_OFFSET: usize = 60;
const WATCH_ID_OFFSET: usize = 76;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestNonce([u8; REQUEST_NONCE_LEN]);

impl RequestNonce {
    pub fn try_new(bytes: [u8; REQUEST_NONCE_LEN]) -> Result<Self, ValueError> {
        if bytes == [0; REQUEST_NONCE_LEN] {
            Err(ValueError::AllZero)
        } else {
            Ok(Self(bytes))
        }
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; REQUEST_NONCE_LEN] {
        &self.0
    }
}

impl Debug for RequestNonce {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("RequestNonce(<redacted>)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServiceInstanceId([u8; SERVICE_INSTANCE_LEN]);

impl ServiceInstanceId {
    pub fn try_new(bytes: [u8; SERVICE_INSTANCE_LEN]) -> Result<Self, ValueError> {
        if bytes == [0; SERVICE_INSTANCE_LEN] {
            Err(ValueError::AllZero)
        } else {
            Ok(Self(bytes))
        }
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; SERVICE_INSTANCE_LEN] {
        &self.0
    }
}

impl Debug for ServiceInstanceId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServiceInstanceId(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WatchId(u64);

impl WatchId {
    pub fn try_new(value: u64) -> Result<Self, ValueError> {
        if value == 0 {
            Err(ValueError::Zero)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueError {
    Zero,
    AllZero,
}

impl Display for ValueError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid zero IPC correlation value")
    }
}

impl Error for ValueError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    WrongSize,
    Magic,
    Version,
    Operation,
    Status,
    Flags,
    PayloadLength,
    Correlation,
    Reserved,
}

impl Display for DecodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid unlock IPC frame: {self:?}")
    }
}

impl Error for DecodeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsumeOrWatch {
    nonce: RequestNonce,
    selector: SessionSelector,
}

impl ConsumeOrWatch {
    #[must_use]
    pub const fn nonce(self) -> RequestNonce {
        self.nonce
    }

    #[must_use]
    pub const fn selector(self) -> SessionSelector {
        self.selector
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionSelector {
    console_uid: ConsoleUid,
    audit_session_id: AuditSessionId,
}

impl SessionSelector {
    #[must_use]
    pub const fn new(console_uid: ConsoleUid, audit_session_id: AuditSessionId) -> Self {
        Self {
            console_uid,
            audit_session_id,
        }
    }

    #[must_use]
    pub const fn console_uid(self) -> ConsoleUid {
        self.console_uid
    }

    #[must_use]
    pub const fn audit_session_id(self) -> AuditSessionId {
        self.audit_session_id
    }

    #[must_use]
    pub const fn matches(self, binding: SessionBinding) -> bool {
        self.console_uid.get() == binding.console_uid().get()
            && self.audit_session_id.get() == binding.audit_session_id().get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyKind {
    Consumed,
    Watching { watch_id: WatchId },
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reply {
    service_instance: ServiceInstanceId,
    binding: SessionBinding,
    kind: ReplyKind,
}

impl Reply {
    #[must_use]
    pub const fn is_consumed(self) -> bool {
        matches!(self.kind, ReplyKind::Consumed)
    }

    #[must_use]
    pub const fn is_watching(self) -> bool {
        matches!(self.kind, ReplyKind::Watching { .. })
    }

    #[must_use]
    pub const fn is_denied(self) -> bool {
        matches!(self.kind, ReplyKind::Denied)
    }

    #[must_use]
    pub const fn watch_id(self) -> Option<WatchId> {
        match self.kind {
            ReplyKind::Watching { watch_id } => Some(watch_id),
            ReplyKind::Consumed | ReplyKind::Denied => None,
        }
    }

    #[must_use]
    pub const fn service_instance(self) -> ServiceInstanceId {
        self.service_instance
    }

    #[must_use]
    pub const fn binding(self) -> SessionBinding {
        self.binding
    }

    #[must_use]
    pub const fn kind(self) -> ReplyKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermitAvailableEvent {
    service_instance: ServiceInstanceId,
    watch_id: WatchId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchKeepalive {
    service_instance: ServiceInstanceId,
    watch_id: WatchId,
}

impl WatchKeepalive {
    #[must_use]
    pub const fn service_instance(self) -> ServiceInstanceId {
        self.service_instance
    }

    #[must_use]
    pub const fn watch_id(self) -> WatchId {
        self.watch_id
    }
}

impl PermitAvailableEvent {
    #[must_use]
    pub const fn service_instance(self) -> ServiceInstanceId {
        self.service_instance
    }

    #[must_use]
    pub const fn watch_id(self) -> WatchId {
        self.watch_id
    }
}

#[must_use]
pub fn encode_consume_or_watch(nonce: RequestNonce, selector: SessionSelector) -> [u8; FRAME_LEN] {
    let binding = SessionBinding::new(
        LockEpoch::new(0),
        selector.audit_session_id(),
        selector.console_uid(),
    );
    encode_frame(
        OP_CONSUME_OR_WATCH,
        STATUS_REQUEST,
        nonce,
        binding,
        None,
        None,
    )
}

pub fn decode_consume_or_watch(frame: &[u8]) -> Result<ConsumeOrWatch, DecodeError> {
    let fields = decode_fields(frame)?;
    if fields.operation != OP_CONSUME_OR_WATCH {
        return Err(DecodeError::Operation);
    }
    if fields.status != STATUS_REQUEST {
        return Err(DecodeError::Status);
    }
    if fields.service_instance.is_some()
        || fields.watch_id.is_some()
        || fields.binding.lock_epoch().get() != 0
    {
        return Err(DecodeError::Reserved);
    }
    Ok(ConsumeOrWatch {
        nonce: fields.nonce,
        selector: SessionSelector::new(
            fields.binding.console_uid(),
            fields.binding.audit_session_id(),
        ),
    })
}

pub fn validate_consume_or_watch_header(header: &[u8]) -> Result<(), DecodeError> {
    validate_common_header(header)?;
    if header[5] != OP_CONSUME_OR_WATCH {
        return Err(DecodeError::Operation);
    }
    if header[6] != STATUS_REQUEST {
        return Err(DecodeError::Status);
    }
    Ok(())
}

pub mod encode_reply {
    use super::{FRAME_LEN, RequestNonce, ServiceInstanceId, SessionBinding, WatchId};

    #[must_use]
    pub fn consumed(
        nonce: RequestNonce,
        binding: SessionBinding,
        service_instance: ServiceInstanceId,
    ) -> [u8; FRAME_LEN] {
        super::encode_frame(
            super::OP_CONSUME_OR_WATCH,
            super::STATUS_CONSUMED,
            nonce,
            binding,
            Some(service_instance),
            None,
        )
    }

    #[must_use]
    pub fn watching(
        nonce: RequestNonce,
        binding: SessionBinding,
        service_instance: ServiceInstanceId,
        watch_id: WatchId,
    ) -> [u8; FRAME_LEN] {
        super::encode_frame(
            super::OP_CONSUME_OR_WATCH,
            super::STATUS_WATCHING,
            nonce,
            binding,
            Some(service_instance),
            Some(watch_id),
        )
    }

    #[must_use]
    pub fn denied(
        nonce: RequestNonce,
        binding: SessionBinding,
        service_instance: ServiceInstanceId,
    ) -> [u8; FRAME_LEN] {
        super::encode_frame(
            super::OP_CONSUME_OR_WATCH,
            super::STATUS_DENIED,
            nonce,
            binding,
            Some(service_instance),
            None,
        )
    }
}

pub fn decode_reply(
    frame: &[u8],
    expected_nonce: RequestNonce,
    expected_selector: SessionSelector,
) -> Result<Reply, DecodeError> {
    let fields = decode_fields(frame)?;
    if fields.operation != OP_CONSUME_OR_WATCH {
        return Err(DecodeError::Operation);
    }
    if fields.nonce != expected_nonce
        || !expected_selector.matches(fields.binding)
        || fields.binding.lock_epoch().get() == 0
    {
        return Err(DecodeError::Correlation);
    }
    let service_instance = fields.service_instance.ok_or(DecodeError::Correlation)?;
    let kind = match fields.status {
        STATUS_CONSUMED if fields.watch_id.is_none() => ReplyKind::Consumed,
        STATUS_WATCHING => ReplyKind::Watching {
            watch_id: fields.watch_id.ok_or(DecodeError::Correlation)?,
        },
        STATUS_DENIED if fields.watch_id.is_none() => ReplyKind::Denied,
        STATUS_CONSUMED | STATUS_DENIED => return Err(DecodeError::Reserved),
        _ => return Err(DecodeError::Status),
    };
    Ok(Reply {
        service_instance,
        binding: fields.binding,
        kind,
    })
}

#[must_use]
pub fn encode_event(
    nonce: RequestNonce,
    binding: SessionBinding,
    service_instance: ServiceInstanceId,
    watch_id: WatchId,
) -> [u8; FRAME_LEN] {
    encode_frame(
        OP_PERMIT_AVAILABLE,
        STATUS_EVENT,
        nonce,
        binding,
        Some(service_instance),
        Some(watch_id),
    )
}

pub fn decode_event(
    frame: &[u8],
    expected_nonce: RequestNonce,
    expected_binding: SessionBinding,
    expected_instance: ServiceInstanceId,
    expected_watch_id: WatchId,
) -> Result<PermitAvailableEvent, DecodeError> {
    let fields = decode_fields(frame)?;
    if fields.operation != OP_PERMIT_AVAILABLE {
        return Err(DecodeError::Operation);
    }
    if fields.status != STATUS_EVENT {
        return Err(DecodeError::Status);
    }
    if fields.binding.lock_epoch().get() == 0 {
        return Err(DecodeError::Correlation);
    }
    if fields.nonce != expected_nonce
        || fields.binding != expected_binding
        || fields.service_instance != Some(expected_instance)
        || fields.watch_id != Some(expected_watch_id)
    {
        return Err(DecodeError::Correlation);
    }
    Ok(PermitAvailableEvent {
        service_instance: expected_instance,
        watch_id: expected_watch_id,
    })
}

#[must_use]
pub fn encode_keepalive(
    nonce: RequestNonce,
    binding: SessionBinding,
    service_instance: ServiceInstanceId,
    watch_id: WatchId,
) -> [u8; FRAME_LEN] {
    encode_frame(
        OP_PERMIT_AVAILABLE,
        STATUS_KEEPALIVE,
        nonce,
        binding,
        Some(service_instance),
        Some(watch_id),
    )
}

pub fn decode_keepalive(
    frame: &[u8],
    expected_nonce: RequestNonce,
    expected_binding: SessionBinding,
    expected_instance: ServiceInstanceId,
    expected_watch_id: WatchId,
) -> Result<WatchKeepalive, DecodeError> {
    let fields = decode_fields(frame)?;
    if fields.operation != OP_PERMIT_AVAILABLE {
        return Err(DecodeError::Operation);
    }
    if fields.status != STATUS_KEEPALIVE {
        return Err(DecodeError::Status);
    }
    if fields.binding.lock_epoch().get() == 0
        || fields.nonce != expected_nonce
        || fields.binding != expected_binding
        || fields.service_instance != Some(expected_instance)
        || fields.watch_id != Some(expected_watch_id)
    {
        return Err(DecodeError::Correlation);
    }
    Ok(WatchKeepalive {
        service_instance: expected_instance,
        watch_id: expected_watch_id,
    })
}

fn encode_frame(
    operation: u8,
    status: u8,
    nonce: RequestNonce,
    binding: SessionBinding,
    service_instance: Option<ServiceInstanceId>,
    watch_id: Option<WatchId>,
) -> [u8; FRAME_LEN] {
    let mut frame = [0_u8; FRAME_LEN];
    frame[0..4].copy_from_slice(MAGIC);
    frame[4] = VERSION;
    frame[5] = operation;
    frame[6] = status;
    frame[8..12].copy_from_slice(&(PAYLOAD_LEN as u32).to_be_bytes());
    frame[NONCE_OFFSET..CONSOLE_UID_OFFSET].copy_from_slice(nonce.as_bytes());
    frame[CONSOLE_UID_OFFSET..AUDIT_SESSION_OFFSET]
        .copy_from_slice(&binding.console_uid().get().to_be_bytes());
    frame[AUDIT_SESSION_OFFSET..LOCK_EPOCH_OFFSET]
        .copy_from_slice(&binding.audit_session_id().get().to_be_bytes());
    frame[LOCK_EPOCH_OFFSET..INSTANCE_OFFSET]
        .copy_from_slice(&binding.lock_epoch().get().to_be_bytes());
    if let Some(service_instance) = service_instance {
        frame[INSTANCE_OFFSET..WATCH_ID_OFFSET].copy_from_slice(service_instance.as_bytes());
    }
    if let Some(watch_id) = watch_id {
        frame[WATCH_ID_OFFSET..FRAME_LEN].copy_from_slice(&watch_id.get().to_be_bytes());
    }
    frame
}

struct Fields {
    operation: u8,
    status: u8,
    nonce: RequestNonce,
    binding: SessionBinding,
    service_instance: Option<ServiceInstanceId>,
    watch_id: Option<WatchId>,
}

fn decode_fields(frame: &[u8]) -> Result<Fields, DecodeError> {
    if frame.len() != FRAME_LEN {
        return Err(DecodeError::WrongSize);
    }
    validate_common_header(&frame[..HEADER_LEN])?;
    let nonce = RequestNonce::try_new(
        frame[NONCE_OFFSET..CONSOLE_UID_OFFSET]
            .try_into()
            .expect("fixed nonce slice"),
    )
    .map_err(|_| DecodeError::Correlation)?;
    let binding = SessionBinding::new(
        LockEpoch::new(read_u64(&frame[LOCK_EPOCH_OFFSET..INSTANCE_OFFSET])),
        AuditSessionId::new(read_u32(&frame[AUDIT_SESSION_OFFSET..LOCK_EPOCH_OFFSET])),
        ConsoleUid::new(read_u32(&frame[CONSOLE_UID_OFFSET..AUDIT_SESSION_OFFSET])),
    );
    let instance_bytes: [u8; SERVICE_INSTANCE_LEN] = frame[INSTANCE_OFFSET..WATCH_ID_OFFSET]
        .try_into()
        .expect("fixed service instance slice");
    let service_instance = if instance_bytes == [0; SERVICE_INSTANCE_LEN] {
        None
    } else {
        Some(
            ServiceInstanceId::try_new(instance_bytes)
                .expect("nonzero service instance was prevalidated"),
        )
    };
    let raw_watch = read_u64(&frame[WATCH_ID_OFFSET..FRAME_LEN]);
    let watch_id = if raw_watch == 0 {
        None
    } else {
        Some(WatchId::try_new(raw_watch).expect("nonzero watch id was prevalidated"))
    };
    Ok(Fields {
        operation: frame[5],
        status: frame[6],
        nonce,
        binding,
        service_instance,
        watch_id,
    })
}

fn validate_common_header(header: &[u8]) -> Result<(), DecodeError> {
    if header.len() != HEADER_LEN {
        return Err(DecodeError::WrongSize);
    }
    if &header[0..4] != MAGIC {
        return Err(DecodeError::Magic);
    }
    if header[4] != VERSION {
        return Err(DecodeError::Version);
    }
    if header[7] != 0 {
        return Err(DecodeError::Flags);
    }
    let payload_len = read_u32(&header[8..12]);
    if payload_len != PAYLOAD_LEN as u32 {
        return Err(DecodeError::PayloadLength);
    }
    Ok(())
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("fixed u32 slice"))
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().expect("fixed u64 slice"))
}
