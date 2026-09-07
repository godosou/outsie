use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::calibration::CalibrationProfile;
use crate::domain::{MonoMillis, RssiDbm};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProximityPolicy {
    sample_window: usize,
    dwell: MonoMillis,
}

impl ProximityPolicy {
    pub fn new(sample_window: usize, dwell: MonoMillis) -> Result<Self, ProximityPolicyError> {
        if sample_window == 0 {
            return Err(ProximityPolicyError::EmptySampleWindow);
        }
        if dwell.get() == 0 {
            return Err(ProximityPolicyError::ZeroDwell);
        }
        Ok(Self {
            sample_window,
            dwell,
        })
    }

    #[must_use]
    pub const fn sample_window(self) -> usize {
        self.sample_window
    }

    #[must_use]
    pub const fn dwell(self) -> MonoMillis {
        self.dwell
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximityPolicyError {
    EmptySampleWindow,
    ZeroDwell,
}

impl Display for ProximityPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySampleWindow => write!(formatter, "sample window must not be empty"),
            Self::ZeroDwell => write!(formatter, "dwell must be positive"),
        }
    }
}

impl Error for ProximityPolicyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximityEvent {
    FarStable,
    NearStable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProximityState {
    Far,
    Near,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingState {
    state: ProximityState,
    since: MonoMillis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximityError {
    InvalidProfile {
        near_threshold_dbm: i16,
        far_threshold_dbm: i16,
    },
    InvalidRssi {
        value: i16,
    },
    NonMonotonicTime {
        previous: MonoMillis,
        current: MonoMillis,
    },
}

impl Display for ProximityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProfile {
                near_threshold_dbm,
                far_threshold_dbm,
            } => write!(
                formatter,
                "near threshold {near_threshold_dbm} must exceed far threshold {far_threshold_dbm}"
            ),
            Self::InvalidRssi { value } => write!(formatter, "invalid RSSI {value}"),
            Self::NonMonotonicTime { previous, current } => write!(
                formatter,
                "monotonic time moved backwards from {} to {}",
                previous.get(),
                current.get()
            ),
        }
    }
}

impl Error for ProximityError {}

#[derive(Debug)]
pub struct ProximityFilter {
    profile: CalibrationProfile,
    policy: ProximityPolicy,
    samples: VecDeque<i16>,
    last_now: Option<MonoMillis>,
    pending: Option<PendingState>,
    stable: Option<ProximityState>,
}

impl ProximityFilter {
    pub fn new(
        profile: CalibrationProfile,
        policy: ProximityPolicy,
    ) -> Result<Self, ProximityError> {
        if profile.near_threshold_dbm <= profile.far_threshold_dbm {
            return Err(ProximityError::InvalidProfile {
                near_threshold_dbm: profile.near_threshold_dbm,
                far_threshold_dbm: profile.far_threshold_dbm,
            });
        }

        Ok(Self {
            profile,
            policy,
            samples: VecDeque::with_capacity(policy.sample_window),
            last_now: None,
            pending: None,
            stable: None,
        })
    }

    pub fn push(
        &mut self,
        sample: i16,
        now: MonoMillis,
    ) -> Result<Option<ProximityEvent>, ProximityError> {
        let sample = RssiDbm::try_new(sample)
            .map_err(|error| ProximityError::InvalidRssi { value: error.value })?;
        if let Some(previous) = self.last_now
            && now < previous
        {
            return Err(ProximityError::NonMonotonicTime {
                previous,
                current: now,
            });
        }
        self.last_now = Some(now);

        if self.samples.len() == self.policy.sample_window {
            self.samples.pop_front();
        }
        self.samples.push_back(sample.get());
        if self.samples.len() < self.policy.sample_window {
            self.pending = None;
            return Ok(None);
        }

        let state = self.classify_window();
        let Some(state) = state else {
            self.pending = None;
            return Ok(None);
        };
        if self.stable == Some(state) {
            self.pending = None;
            return Ok(None);
        }

        match self.pending {
            Some(pending) if pending.state == state => {
                if now.get() - pending.since.get() >= self.policy.dwell.get() {
                    self.stable = Some(state);
                    self.pending = None;
                    Ok(Some(match state {
                        ProximityState::Far => ProximityEvent::FarStable,
                        ProximityState::Near => ProximityEvent::NearStable,
                    }))
                } else {
                    Ok(None)
                }
            }
            _ => {
                self.pending = Some(PendingState { state, since: now });
                Ok(None)
            }
        }
    }

    fn classify_window(&self) -> Option<ProximityState> {
        let mut sorted: Vec<_> = self.samples.iter().copied().collect();
        sorted.sort_unstable();
        let median = sorted[(sorted.len() - 1) / 2];
        if median >= self.profile.near_threshold_dbm {
            Some(ProximityState::Near)
        } else if median <= self.profile.far_threshold_dbm {
            Some(ProximityState::Far)
        } else {
            None
        }
    }
}
