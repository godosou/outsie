use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use repose_unlock_core::domain::MonoMillis;
use repose_unlock_core::permit::{ConsumeError, InstallError, PermitStore};
use repose_unlock_core::replay::{CounterStore, DurableReplayGuard};
use repose_unlock_core::state_machine::{
    Event, Permit, SessionBinding, UnlockPhase, UnlockState, transition,
};
use repose_unlock_ipc::{RequestNonce, ServiceInstanceId, SessionSelector, WatchId};

pub trait MonotonicClock: Send + Sync {
    fn now(&self) -> MonoMillis;
}

pub struct SystemMonotonicClock {
    origin: Instant,
}

impl SystemMonotonicClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemMonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for SystemMonotonicClock {
    fn now(&self) -> MonoMillis {
        let elapsed = self.origin.elapsed().as_millis();
        MonoMillis::new(u64::try_from(elapsed).unwrap_or(u64::MAX))
    }
}

pub struct PermitBroker<S, C> {
    authority: DurableReplayGuard<S>,
    clock: C,
    inner: Arc<Mutex<BrokerState>>,
}

struct BrokerState {
    permits: PermitStore,
    reducer: Option<UnlockState>,
    authoritative_binding: Option<SessionBinding>,
    service_instance: ServiceInstanceId,
    next_watch_id: u64,
    revision: u64,
    watchers: HashMap<u64, WatchEntry>,
    maximum_watchers: usize,
    last_now: Option<MonoMillis>,
}

struct WatchEntry {
    nonce: RequestNonce,
    binding: SessionBinding,
    cell: Arc<WatchCell>,
}

struct WatchCell {
    state: Mutex<WatchCellState>,
    changed: Condvar,
}

enum WatchCellState {
    Pending,
    Ready(WatchReady),
    Invalidated,
    Delivered,
}

impl WatchCell {
    fn pending() -> Self {
        Self {
            state: Mutex::new(WatchCellState::Pending),
            changed: Condvar::new(),
        }
    }

    fn set_ready(&self, ready: WatchReady) -> bool {
        let mut state = self.state.lock();
        if matches!(*state, WatchCellState::Pending) {
            *state = WatchCellState::Ready(ready);
            true
        } else {
            false
        }
    }

    fn invalidate(&self) {
        let mut state = self.state.lock();
        if !matches!(*state, WatchCellState::Delivered) {
            *state = WatchCellState::Invalidated;
        }
    }

    fn is_delivered(&self) -> bool {
        matches!(*self.state.lock(), WatchCellState::Delivered)
    }
}

impl<S: CounterStore, C: MonotonicClock> PermitBroker<S, C> {
    #[must_use]
    pub fn new(
        authority: DurableReplayGuard<S>,
        clock: C,
        service_instance: ServiceInstanceId,
        reducer: UnlockState,
        maximum_watchers: usize,
    ) -> Self {
        let authoritative_binding = reducer.binding();
        let last_now = reducer.last_observed_at();
        Self {
            authority,
            clock,
            inner: Arc::new(Mutex::new(BrokerState {
                permits: PermitStore::new(),
                reducer: Some(reducer),
                authoritative_binding,
                service_instance,
                next_watch_id: 1,
                revision: 0,
                watchers: HashMap::new(),
                maximum_watchers,
                last_now,
            })),
        }
    }

    pub fn consume_or_watch(
        &self,
        selector: SessionSelector,
        nonce: RequestNonce,
    ) -> Result<BrokerOutcome, BrokerError> {
        let registry = Arc::downgrade(&self.inner);
        let mut inner = self.inner.lock();
        let now = self.clock.now();
        if clock_rolled_back(&inner, now) {
            reset_after_clock_rollback(&mut inner, now);
            return Err(BrokerError::ClockRollback);
        }
        inner.last_now = Some(now);
        prune_delivered(&mut inner);

        let binding = inner
            .authoritative_binding
            .filter(|binding| selector.matches(*binding))
            .ok_or(BrokerError::SessionMismatch)?;
        if reducer(&inner)
            .permit_expires_at()
            .is_some_and(|expires_at| now >= expires_at)
        {
            inner.permits.clear();
            advance_expired_reducer(&mut inner, now)?;
        }
        if matches!(
            reducer(&inner).phase(),
            UnlockPhase::Unlocked | UnlockPhase::Unlocking
        ) {
            return Err(BrokerError::NotAuthorizable);
        }

        match inner
            .permits
            .consume(&self.authority, binding, Some(binding), now)
        {
            Ok(consumed) => {
                let state = inner.reducer.take().expect("broker always owns reducer");
                let (state, effects) = transition(state, Event::PermitConsumed(consumed), now)
                    .map_err(|error| {
                        inner.reducer = Some(error.into_state());
                        BrokerError::AuthorizationRejected
                    })?;
                let authorized = state.phase() == UnlockPhase::Unlocking && effects.is_empty();
                inner.authoritative_binding = state.binding();
                inner.reducer = Some(state);
                let service_instance = inner.service_instance;
                let revision = inner.revision;
                let watchers = take_all_watchers(&mut inner);
                drop(inner);
                invalidate_watchers(watchers);
                if authorized {
                    Ok(BrokerOutcome::Authorized(AuthorizationReceipt {
                        nonce,
                        binding,
                        service_instance,
                        revision,
                    }))
                } else {
                    Err(BrokerError::AuthorizationRejected)
                }
            }
            Err(ConsumeError::Empty) => register_watch(&mut inner, nonce, binding, registry),
            Err(ConsumeError::Expired) => {
                advance_expired_reducer(&mut inner, now)?;
                register_watch(&mut inner, nonce, binding, registry)
            }
            Err(error) => {
                reset_after_security_failure(&mut inner, now, binding);
                Err(BrokerError::Permit(error))
            }
        }
    }

    pub fn publish(&self, permit: Permit) -> Result<PublishOutcome, BrokerError> {
        let mut inner = self.inner.lock();
        let now = self.clock.now();
        if clock_rolled_back(&inner, now) {
            reset_after_clock_rollback(&mut inner, now);
            return Err(BrokerError::ClockRollback);
        }
        inner.last_now = Some(now);
        prune_delivered(&mut inner);

        if inner.authoritative_binding != Some(permit.binding())
            || reducer(&inner).phase() != UnlockPhase::PermitReady
            || reducer(&inner).challenge_id() != Some(permit.challenge_id())
            || reducer(&inner).permit_expires_at() != Some(permit.expires_at())
        {
            return Err(BrokerError::PermitStateMismatch);
        }
        if now >= permit.expires_at() {
            advance_expired_reducer(&mut inner, now)?;
            return Err(BrokerError::ExpiredPermit);
        }
        let next_revision = inner
            .revision
            .checked_add(1)
            .ok_or(BrokerError::RevisionExhausted)?;
        inner
            .permits
            .install(permit, now)
            .map_err(BrokerError::Install)?;
        inner.revision = next_revision;
        let revision = inner.revision;
        let service_instance = inner.service_instance;
        let mut notified = Vec::new();
        for (raw_id, entry) in &inner.watchers {
            if entry.binding == inner.authoritative_binding.expect("permit binding checked") {
                let watch_id = WatchId::try_new(*raw_id).expect("broker allocates nonzero ids");
                let ready = WatchReady {
                    nonce: entry.nonce,
                    binding: entry.binding,
                    service_instance,
                    watch_id,
                    revision,
                };
                if entry.cell.set_ready(ready) {
                    notified.push(Arc::clone(&entry.cell));
                }
            }
        }
        let count = notified.len();
        drop(inner);
        for cell in notified {
            cell.changed.notify_all();
        }
        Ok(PublishOutcome {
            notified_watchers: count,
            revision,
        })
    }

    pub fn update_session(
        &self,
        authoritative_binding: Option<SessionBinding>,
    ) -> Result<(), BrokerError> {
        let mut inner = self.inner.lock();
        let now = self.clock.now();
        if clock_rolled_back(&inner, now) {
            reset_after_clock_rollback(&mut inner, now);
            return Err(BrokerError::ClockRollback);
        }
        inner.last_now = Some(now);
        if inner.authoritative_binding == authoritative_binding {
            return Ok(());
        }
        let next_revision = match inner.revision.checked_add(1) {
            Some(next) => next,
            None => {
                inner.service_instance = successor(inner.service_instance);
                inner.next_watch_id = 1;
                0
            }
        };
        inner.permits.clear();
        let watchers = take_all_watchers(&mut inner);
        let state = inner.reducer.take().expect("broker always owns reducer");
        let (state, _) = transition(
            state,
            Event::FastUserSwitch {
                locked_binding: authoritative_binding,
            },
            now,
        )
        .map_err(|error| {
            inner.reducer = Some(error.into_state());
            BrokerError::AuthorizationRejected
        })?;
        inner.authoritative_binding = state.binding();
        inner.reducer = Some(state);
        inner.revision = next_revision;
        drop(inner);
        invalidate_watchers(watchers);
        Ok(())
    }

    pub fn restart(
        &self,
        new_instance: ServiceInstanceId,
        authoritative_binding: Option<SessionBinding>,
    ) -> Result<(), BrokerError> {
        let mut inner = self.inner.lock();
        let now = self.clock.now();
        let reused_instance = new_instance == inner.service_instance;
        let prior_epoch = reducer(&inner).highest_lock_epoch();
        let epoch_advanced = match (prior_epoch, authoritative_binding) {
            (Some(previous), Some(replacement)) => replacement.lock_epoch() > previous,
            (None, Some(_)) | (_, None) => true,
        };
        inner.service_instance = if reused_instance {
            successor(inner.service_instance)
        } else {
            new_instance
        };
        inner.next_watch_id = 1;
        inner.revision = 0;
        inner.permits.service_restarted();
        let watchers = take_all_watchers(&mut inner);
        let state = inner.reducer.take().expect("broker always owns reducer");
        let locked_binding = if epoch_advanced && !reused_instance {
            authoritative_binding
        } else {
            None
        };
        let (state, _) = transition(state, Event::ServiceRestarted { locked_binding }, now)
            .map_err(|error| {
                inner.reducer = Some(error.into_state());
                BrokerError::AuthorizationRejected
            })?;
        inner.authoritative_binding = state.binding();
        inner.reducer = Some(state);
        inner.last_now = Some(now);
        drop(inner);
        invalidate_watchers(watchers);
        if reused_instance {
            Err(BrokerError::ReusedServiceInstance)
        } else if epoch_advanced {
            Ok(())
        } else {
            Err(BrokerError::RestartEpochNotAdvanced)
        }
    }

    pub fn cancel_watch(&self, key: WatchKey) -> bool {
        let mut inner = self.inner.lock();
        if key.service_instance != inner.service_instance {
            return false;
        }
        let removed = inner.watchers.remove(&key.watch_id.get());
        drop(inner);
        if let Some(entry) = removed {
            entry.cell.invalidate();
            entry.cell.changed.notify_all();
            true
        } else {
            false
        }
    }

    #[must_use]
    pub fn service_instance(&self) -> ServiceInstanceId {
        self.inner.lock().service_instance
    }

    #[must_use]
    pub fn is_unlocking(&self) -> bool {
        reducer(&self.inner.lock()).phase() == UnlockPhase::Unlocking
    }

    #[must_use]
    pub fn phase(&self) -> UnlockPhase {
        reducer(&self.inner.lock()).phase()
    }

    #[must_use]
    pub fn watch_count(&self) -> usize {
        self.inner.lock().watchers.len()
    }
}

fn register_watch(
    inner: &mut BrokerState,
    nonce: RequestNonce,
    binding: SessionBinding,
    registry: Weak<Mutex<BrokerState>>,
) -> Result<BrokerOutcome, BrokerError> {
    if inner.watchers.values().any(|entry| entry.nonce == nonce) {
        return Err(BrokerError::DuplicateNonce);
    }
    if inner.watchers.len() >= inner.maximum_watchers {
        return Err(BrokerError::WatchLimit);
    }
    let raw_id = inner.next_watch_id;
    let next_id = raw_id.checked_add(1).ok_or(BrokerError::WatchIdExhausted)?;
    let next_revision = inner
        .revision
        .checked_add(1)
        .ok_or(BrokerError::RevisionExhausted)?;
    let watch_id = WatchId::try_new(raw_id).map_err(|_| BrokerError::WatchIdExhausted)?;
    let cell = Arc::new(WatchCell::pending());
    inner.watchers.insert(
        raw_id,
        WatchEntry {
            nonce,
            binding,
            cell: Arc::clone(&cell),
        },
    );
    inner.next_watch_id = next_id;
    inner.revision = next_revision;
    Ok(BrokerOutcome::Watching(WatchRegistration {
        key: WatchKey {
            service_instance: inner.service_instance,
            watch_id,
        },
        nonce,
        binding,
        cell,
        registry,
    }))
}

fn advance_expired_reducer(inner: &mut BrokerState, now: MonoMillis) -> Result<(), BrokerError> {
    let state = inner.reducer.take().expect("broker always owns reducer");
    match transition(state, Event::Tick, now) {
        Ok((state, _)) => {
            inner.authoritative_binding = state.binding();
            inner.reducer = Some(state);
            Ok(())
        }
        Err(error) => {
            inner.reducer = Some(error.into_state());
            Err(BrokerError::AuthorizationRejected)
        }
    }
}

fn clock_rolled_back(inner: &BrokerState, now: MonoMillis) -> bool {
    inner.last_now.is_some_and(|previous| now < previous)
}

fn reset_after_clock_rollback(inner: &mut BrokerState, now: MonoMillis) {
    inner.service_instance = successor(inner.service_instance);
    inner.next_watch_id = 1;
    inner.permits.service_restarted();
    let watchers = take_all_watchers(inner);
    let binding = inner.authoritative_binding;
    let state = inner.reducer.take().expect("broker always owns reducer");
    let (state, _) = transition(
        state,
        Event::ServiceRestarted {
            locked_binding: binding,
        },
        now,
    )
    .expect("service reset events cannot fail");
    inner.authoritative_binding = state.binding();
    inner.reducer = Some(state);
    inner.last_now = Some(now);
    inner.revision = 0;
    invalidate_watchers(watchers);
}

fn reset_after_security_failure(inner: &mut BrokerState, now: MonoMillis, binding: SessionBinding) {
    inner.service_instance = successor(inner.service_instance);
    inner.next_watch_id = 1;
    inner.revision = 0;
    inner.permits.service_restarted();
    let watchers = take_all_watchers(inner);
    let state = inner.reducer.take().expect("broker always owns reducer");
    let (state, _) = transition(
        state,
        Event::ServiceRestarted {
            locked_binding: Some(binding),
        },
        now,
    )
    .expect("service reset events cannot fail");
    inner.authoritative_binding = state.binding();
    inner.reducer = Some(state);
    inner.last_now = Some(now);
    invalidate_watchers(watchers);
}

fn successor(current: ServiceInstanceId) -> ServiceInstanceId {
    let value = u128::from_be_bytes(*current.as_bytes());
    let next = value.checked_add(1).unwrap_or(1);
    ServiceInstanceId::try_new(next.to_be_bytes()).expect("successor is nonzero")
}

fn reducer(inner: &BrokerState) -> &UnlockState {
    inner.reducer.as_ref().expect("broker always owns reducer")
}

fn prune_delivered(inner: &mut BrokerState) {
    inner.watchers.retain(|_, entry| !entry.cell.is_delivered());
}

fn take_all_watchers(inner: &mut BrokerState) -> Vec<Arc<WatchCell>> {
    inner
        .watchers
        .drain()
        .map(|(_, entry)| entry.cell)
        .collect()
}

fn invalidate_watchers(watchers: Vec<Arc<WatchCell>>) {
    for cell in watchers {
        cell.invalidate();
        cell.changed.notify_all();
    }
}

pub enum BrokerOutcome {
    Authorized(AuthorizationReceipt),
    Watching(WatchRegistration),
}

impl Debug for BrokerOutcome {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authorized(_) => formatter.write_str("Authorized(<opaque>)"),
            Self::Watching(_) => formatter.write_str("Watching(<opaque>)"),
        }
    }
}

pub struct AuthorizationReceipt {
    nonce: RequestNonce,
    binding: SessionBinding,
    service_instance: ServiceInstanceId,
    revision: u64,
}

impl AuthorizationReceipt {
    #[must_use]
    pub const fn nonce(&self) -> RequestNonce {
        self.nonce
    }

    #[must_use]
    pub const fn binding(&self) -> SessionBinding {
        self.binding
    }

    #[must_use]
    pub const fn service_instance(&self) -> ServiceInstanceId {
        self.service_instance
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

impl Debug for AuthorizationReceipt {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizationReceipt(<opaque>)")
    }
}

pub struct WatchRegistration {
    key: WatchKey,
    nonce: RequestNonce,
    binding: SessionBinding,
    cell: Arc<WatchCell>,
    registry: Weak<Mutex<BrokerState>>,
}

impl WatchRegistration {
    #[must_use]
    pub const fn key(&self) -> WatchKey {
        self.key
    }

    #[must_use]
    pub const fn watch_id(&self) -> WatchId {
        self.key.watch_id
    }

    #[must_use]
    pub const fn service_instance(&self) -> ServiceInstanceId {
        self.key.service_instance
    }

    #[must_use]
    pub const fn nonce(&self) -> RequestNonce {
        self.nonce
    }

    #[must_use]
    pub const fn binding(&self) -> SessionBinding {
        self.binding
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<WatchReady, WatchWaitError> {
        let mut state = self.cell.state.lock();
        if matches!(*state, WatchCellState::Pending) {
            self.cell.changed.wait_for(&mut state, timeout);
        }
        match std::mem::replace(&mut *state, WatchCellState::Delivered) {
            WatchCellState::Ready(ready) => Ok(ready),
            WatchCellState::Invalidated => Err(WatchWaitError::Invalidated),
            WatchCellState::Pending => {
                *state = WatchCellState::Pending;
                Err(WatchWaitError::Timeout)
            }
            WatchCellState::Delivered => Err(WatchWaitError::Closed),
        }
    }
}

impl Debug for WatchRegistration {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("WatchRegistration(<opaque>)")
    }
}

impl Drop for WatchRegistration {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            let mut inner = registry.lock();
            if self.key.service_instance == inner.service_instance
                && inner
                    .watchers
                    .get(&self.key.watch_id.get())
                    .is_some_and(|entry| Arc::ptr_eq(&entry.cell, &self.cell))
            {
                inner.watchers.remove(&self.key.watch_id.get());
            }
        }
        let mut state = self.cell.state.lock();
        if matches!(*state, WatchCellState::Pending | WatchCellState::Ready(_)) {
            *state = WatchCellState::Invalidated;
            self.cell.changed.notify_all();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WatchKey {
    service_instance: ServiceInstanceId,
    watch_id: WatchId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchReady {
    nonce: RequestNonce,
    binding: SessionBinding,
    service_instance: ServiceInstanceId,
    watch_id: WatchId,
    revision: u64,
}

impl WatchReady {
    #[must_use]
    pub const fn nonce(self) -> RequestNonce {
        self.nonce
    }

    #[must_use]
    pub const fn binding(self) -> SessionBinding {
        self.binding
    }

    #[must_use]
    pub const fn service_instance(self) -> ServiceInstanceId {
        self.service_instance
    }

    #[must_use]
    pub const fn watch_id(self) -> WatchId {
        self.watch_id
    }

    #[must_use]
    pub const fn revision(self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishOutcome {
    notified_watchers: usize,
    revision: u64,
}

impl PublishOutcome {
    #[must_use]
    pub const fn notified_watchers(self) -> usize {
        self.notified_watchers
    }

    #[must_use]
    pub const fn revision(self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchWaitError {
    Timeout,
    Invalidated,
    Closed,
}

impl Display for WatchWaitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "watch did not produce an authorization event: {self:?}"
        )
    }
}

impl Error for WatchWaitError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerError {
    SessionMismatch,
    NotAuthorizable,
    ClockRollback,
    DuplicateNonce,
    WatchLimit,
    WatchIdExhausted,
    RevisionExhausted,
    ReusedServiceInstance,
    RestartEpochNotAdvanced,
    ExpiredPermit,
    PermitStateMismatch,
    Install(InstallError),
    Permit(ConsumeError),
    AuthorizationRejected,
}

impl Display for BrokerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "unlock broker denied operation: {self:?}")
    }
}

impl Error for BrokerError {}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Barrier, Weak};
    use std::time::Duration;

    use repose_unlock_core::domain::{AuditSessionId, ConsoleUid, LockEpoch, MonoMillis};
    use repose_unlock_core::replay::{DurableReplayGuard, MemoryCounterStore, ReplayPolicy};
    use repose_unlock_core::state_machine::{
        Event, SessionBinding, TimingPolicy, UnlockState, transition,
    };
    use repose_unlock_ipc::{RequestNonce, ServiceInstanceId, SessionSelector};

    use super::{
        BrokerError, BrokerOutcome, BrokerState, MonotonicClock, PermitBroker, WatchWaitError,
    };

    #[derive(Clone)]
    struct Clock(Arc<AtomicU64>);

    impl MonotonicClock for Clock {
        fn now(&self) -> MonoMillis {
            MonoMillis::new(self.0.load(Ordering::SeqCst))
        }
    }

    #[derive(Clone)]
    struct InversionClock {
        next: Arc<AtomicU64>,
        armed: Arc<AtomicBool>,
        rendezvous: Arc<Barrier>,
        sampled_outside_lock: Arc<AtomicBool>,
        broker: Arc<parking_lot::Mutex<Option<Weak<parking_lot::Mutex<BrokerState>>>>>,
    }

    impl InversionClock {
        fn new(first: u64) -> Self {
            Self {
                next: Arc::new(AtomicU64::new(first)),
                armed: Arc::new(AtomicBool::new(false)),
                rendezvous: Arc::new(Barrier::new(2)),
                sampled_outside_lock: Arc::new(AtomicBool::new(false)),
                broker: Arc::new(parking_lot::Mutex::new(None)),
            }
        }

        fn attach(&self, broker: Weak<parking_lot::Mutex<BrokerState>>) {
            *self.broker.lock() = Some(broker);
        }

        fn arm(&self) {
            assert!(!self.armed.swap(true, Ordering::AcqRel));
        }

        fn wait_until_sampled(&self) {
            self.rendezvous.wait();
        }

        fn release(&self) {
            self.rendezvous.wait();
        }

        fn sampled_outside_lock(&self) -> bool {
            self.sampled_outside_lock.load(Ordering::Acquire)
        }
    }

    impl MonotonicClock for InversionClock {
        fn now(&self) -> MonoMillis {
            let sampled = self.next.fetch_add(1, Ordering::AcqRel);
            if self.armed.swap(false, Ordering::AcqRel) {
                let broker = self
                    .broker
                    .lock()
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .expect("clock attached to live broker");
                self.sampled_outside_lock
                    .store(broker.try_lock().is_some(), Ordering::Release);
                self.rendezvous.wait();
                self.rendezvous.wait();
            }
            MonoMillis::new(sampled)
        }
    }

    fn binding(epoch: u64) -> SessionBinding {
        SessionBinding::new(
            LockEpoch::new(epoch),
            AuditSessionId::new(7),
            ConsoleUid::new(501),
        )
    }

    fn broker() -> PermitBroker<MemoryCounterStore, Clock> {
        let (state, _) = transition(
            UnlockState::unlocked(TimingPolicy::new(5_000, 3_000, 1_000).unwrap()),
            Event::SessionLocked {
                binding: binding(1),
            },
            MonoMillis::new(10),
        )
        .unwrap();
        PermitBroker::new(
            DurableReplayGuard::new(MemoryCounterStore::new(), ReplayPolicy::default()),
            Clock(Arc::new(AtomicU64::new(11))),
            ServiceInstanceId::try_new([1; 16]).unwrap(),
            state,
            8,
        )
    }

    #[test]
    fn revision_exhaustion_never_blocks_session_invalidation() {
        let broker = broker();
        let selector = SessionSelector::new(ConsoleUid::new(501), AuditSessionId::new(7));
        let BrokerOutcome::Watching(watch) = broker
            .consume_or_watch(selector, RequestNonce::try_new([1; 32]).unwrap())
            .unwrap()
        else {
            panic!("expected watch");
        };
        broker.inner.lock().revision = u64::MAX;
        broker.update_session(None).unwrap();
        assert_eq!(
            watch.recv_timeout(Duration::from_millis(1)).unwrap_err(),
            WatchWaitError::Invalidated
        );
        assert_eq!(broker.watch_count(), 0);
    }

    #[test]
    fn exhausted_watch_or_revision_counter_does_not_partially_register() {
        let broker = broker();
        let selector = SessionSelector::new(ConsoleUid::new(501), AuditSessionId::new(7));
        broker.inner.lock().next_watch_id = u64::MAX;
        assert!(
            broker
                .consume_or_watch(selector, RequestNonce::try_new([1; 32]).unwrap())
                .is_err()
        );
        assert_eq!(broker.watch_count(), 0);

        broker.inner.lock().next_watch_id = 1;
        broker.inner.lock().revision = u64::MAX;
        assert!(
            broker
                .consume_or_watch(selector, RequestNonce::try_new([2; 32]).unwrap())
                .is_err()
        );
        assert_eq!(broker.watch_count(), 0);
    }

    #[test]
    fn inverse_thread_arrival_cannot_turn_monotonic_time_into_a_false_rollback() {
        let (state, _) = transition(
            UnlockState::unlocked(TimingPolicy::new(5_000, 3_000, 1_000).unwrap()),
            Event::SessionLocked {
                binding: binding(1),
            },
            MonoMillis::new(10),
        )
        .unwrap();
        let clock = InversionClock::new(11);
        let broker = Arc::new(PermitBroker::new(
            DurableReplayGuard::new(MemoryCounterStore::new(), ReplayPolicy::default()),
            clock.clone(),
            ServiceInstanceId::try_new([1; 16]).unwrap(),
            state,
            8,
        ));
        clock.attach(Arc::downgrade(&broker.inner));
        let selector = SessionSelector::new(ConsoleUid::new(501), AuditSessionId::new(7));

        clock.arm();
        let first = {
            let broker = Arc::clone(&broker);
            std::thread::spawn(move || {
                broker.consume_or_watch(selector, RequestNonce::try_new([1; 32]).unwrap())
            })
        };
        clock.wait_until_sampled();

        // In the buggy implementation the first thread has sampled 11 without
        // holding the broker lock, so let the second invocation linearize with
        // 12 before releasing it. With in-lock sampling, release the first lock
        // holder before making the second invocation.
        let second_before_release = clock
            .sampled_outside_lock()
            .then(|| broker.consume_or_watch(selector, RequestNonce::try_new([2; 32]).unwrap()));
        clock.release();
        let first = first.join().unwrap();
        let second = second_before_release.unwrap_or_else(|| {
            broker.consume_or_watch(selector, RequestNonce::try_new([2; 32]).unwrap())
        });

        let false_rollback = matches!(&first, Err(BrokerError::ClockRollback));
        for outcome in [first, second] {
            if let Ok(BrokerOutcome::Watching(watch)) = outcome {
                drop(watch);
            }
        }
        assert!(
            !false_rollback,
            "lock order must define monotonic time order"
        );
        assert_eq!(
            broker.service_instance(),
            ServiceInstanceId::try_new([1; 16]).unwrap()
        );
        assert_eq!(broker.watch_count(), 0);
    }
}
