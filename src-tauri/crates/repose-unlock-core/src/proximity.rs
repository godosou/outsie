use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::calibration::CalibrationProfile;
use crate::domain::{MonoMillis, RssiDbm};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProximityPolicy {
    sample_window: usize,
    dwell: MonoMillis,
    maximum_sample_gap: MonoMillis,
}

impl ProximityPolicy {
    pub fn new(
        sample_window: usize,
        dwell: MonoMillis,
        maximum_sample_gap: MonoMillis,
    ) -> Result<Self, ProximityPolicyError> {
        if sample_window == 0 {
            return Err(ProximityPolicyError::EmptySampleWindow);
        }
        if sample_window < 3 {
            return Err(ProximityPolicyError::SampleWindowTooSmall {
                value: sample_window,
            });
        }
        if sample_window.is_multiple_of(2) {
            return Err(ProximityPolicyError::EvenSampleWindow {
                value: sample_window,
            });
        }
        if dwell.get() == 0 {
            return Err(ProximityPolicyError::ZeroDwell);
        }
        if maximum_sample_gap.get() == 0 {
            return Err(ProximityPolicyError::ZeroMaximumSampleGap);
        }
        Ok(Self {
            sample_window,
            dwell,
            maximum_sample_gap,
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

    #[must_use]
    pub const fn maximum_sample_gap(self) -> MonoMillis {
        self.maximum_sample_gap
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximityPolicyError {
    EmptySampleWindow,
    SampleWindowTooSmall { value: usize },
    EvenSampleWindow { value: usize },
    ZeroDwell,
    ZeroMaximumSampleGap,
}

impl Display for ProximityPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySampleWindow => write!(formatter, "sample window must not be empty"),
            Self::SampleWindowTooSmall { value } => write!(
                formatter,
                "sample window {value} must contain at least three samples"
            ),
            Self::EvenSampleWindow { value } => write!(
                formatter,
                "sample window {value} must be odd to produce a strict median majority"
            ),
            Self::ZeroDwell => write!(formatter, "dwell must be positive"),
            Self::ZeroMaximumSampleGap => write!(formatter, "maximum sample gap must be positive"),
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
        if let Some(previous) = self.last_now {
            if now < previous {
                self.clear_continuity();
                return Err(ProximityError::NonMonotonicTime {
                    previous,
                    current: now,
                });
            }
            if now.get() - previous.get() > self.policy.maximum_sample_gap.get() {
                self.clear_continuity();
            }
        }
        self.last_now = Some(now);

        let sample = match RssiDbm::try_new(sample) {
            Ok(sample) => sample,
            Err(error) => {
                self.clear_continuity();
                return Err(ProximityError::InvalidRssi { value: error.value });
            }
        };

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
        if median >= self.profile.near_threshold_dbm() {
            Some(ProximityState::Near)
        } else if median <= self.profile.far_threshold_dbm() {
            Some(ProximityState::Far)
        } else {
            None
        }
    }

    fn clear_continuity(&mut self) {
        self.samples.clear();
        self.pending = None;
        self.stable = None;
    }
}
