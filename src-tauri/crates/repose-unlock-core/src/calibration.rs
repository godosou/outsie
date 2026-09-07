use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::domain::RssiDbm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalibrationPolicy {
    minimum_samples: usize,
    near_percentile: u8,
    far_percentile: u8,
    minimum_separation_db: i16,
}

impl CalibrationPolicy {
    pub fn new(
        minimum_samples: usize,
        near_percentile: u8,
        far_percentile: u8,
        minimum_separation_db: i16,
    ) -> Result<Self, CalibrationPolicyError> {
        if minimum_samples == 0 {
            return Err(CalibrationPolicyError::MinimumSamplesZero);
        }
        if near_percentile > 100 {
            return Err(CalibrationPolicyError::PercentileOutOfRange {
                field: PercentileField::Near,
                value: near_percentile,
            });
        }
        if far_percentile > 100 {
            return Err(CalibrationPolicyError::PercentileOutOfRange {
                field: PercentileField::Far,
                value: far_percentile,
            });
        }
        if near_percentile >= far_percentile {
            return Err(CalibrationPolicyError::PercentilesOutOfOrder {
                near: near_percentile,
                far: far_percentile,
            });
        }
        if minimum_separation_db <= 0 {
            return Err(CalibrationPolicyError::MinimumSeparationNotPositive {
                value: minimum_separation_db,
            });
        }

        Ok(Self {
            minimum_samples,
            near_percentile,
            far_percentile,
            minimum_separation_db,
        })
    }

    #[must_use]
    pub const fn prototype() -> Self {
        Self {
            minimum_samples: 8,
            near_percentile: 25,
            far_percentile: 75,
            minimum_separation_db: 8,
        }
    }

    #[must_use]
    pub const fn minimum_samples(self) -> usize {
        self.minimum_samples
    }

    #[must_use]
    pub const fn near_percentile(self) -> u8 {
        self.near_percentile
    }

    #[must_use]
    pub const fn far_percentile(self) -> u8 {
        self.far_percentile
    }

    #[must_use]
    pub const fn minimum_separation_db(self) -> i16 {
        self.minimum_separation_db
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PercentileField {
    Near,
    Far,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationPolicyError {
    MinimumSamplesZero,
    PercentileOutOfRange { field: PercentileField, value: u8 },
    PercentilesOutOfOrder { near: u8, far: u8 },
    MinimumSeparationNotPositive { value: i16 },
}

impl Display for CalibrationPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MinimumSamplesZero => write!(formatter, "minimum_samples must be positive"),
            Self::PercentileOutOfRange { field, value } => {
                write!(formatter, "{field:?} percentile {value} is outside 0..=100")
            }
            Self::PercentilesOutOfOrder { near, far } => write!(
                formatter,
                "near percentile {near} must be lower than far percentile {far}"
            ),
            Self::MinimumSeparationNotPositive { value } => write!(
                formatter,
                "minimum separation must be positive, received {value} dB"
            ),
        }
    }
}

impl Error for CalibrationPolicyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalibrationProfile {
    near_median_dbm: i16,
    far_median_dbm: i16,
    near_threshold_dbm: i16,
    far_threshold_dbm: i16,
    minimum_separation_db: i16,
}

impl CalibrationProfile {
    pub fn new(
        near_median_dbm: i16,
        far_median_dbm: i16,
        near_threshold_dbm: i16,
        far_threshold_dbm: i16,
        minimum_separation_db: i16,
    ) -> Result<Self, CalibrationProfileError> {
        if minimum_separation_db <= 0 {
            return Err(CalibrationProfileError::MinimumSeparationNotPositive {
                value: minimum_separation_db,
            });
        }

        validate_profile_rssi(CalibrationProfileField::NearMedian, near_median_dbm)?;
        validate_profile_rssi(CalibrationProfileField::FarMedian, far_median_dbm)?;
        validate_profile_rssi(CalibrationProfileField::NearThreshold, near_threshold_dbm)?;
        validate_profile_rssi(CalibrationProfileField::FarThreshold, far_threshold_dbm)?;

        if near_threshold_dbm <= far_threshold_dbm {
            return Err(CalibrationProfileError::ThresholdsOutOfOrder {
                near_threshold_dbm,
                far_threshold_dbm,
            });
        }
        let observed_db = near_threshold_dbm - far_threshold_dbm;
        if observed_db < minimum_separation_db {
            return Err(CalibrationProfileError::InsufficientSeparation {
                required_db: minimum_separation_db,
                observed_db,
            });
        }

        Ok(Self {
            near_median_dbm,
            far_median_dbm,
            near_threshold_dbm,
            far_threshold_dbm,
            minimum_separation_db,
        })
    }

    #[must_use]
    pub const fn near_median_dbm(self) -> i16 {
        self.near_median_dbm
    }

    #[must_use]
    pub const fn far_median_dbm(self) -> i16 {
        self.far_median_dbm
    }

    #[must_use]
    pub const fn near_threshold_dbm(self) -> i16 {
        self.near_threshold_dbm
    }

    #[must_use]
    pub const fn far_threshold_dbm(self) -> i16 {
        self.far_threshold_dbm
    }

    #[must_use]
    pub const fn minimum_separation_db(self) -> i16 {
        self.minimum_separation_db
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationProfileField {
    NearMedian,
    FarMedian,
    NearThreshold,
    FarThreshold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationProfileError {
    InvalidRssi {
        field: CalibrationProfileField,
        value: i16,
    },
    MinimumSeparationNotPositive {
        value: i16,
    },
    ThresholdsOutOfOrder {
        near_threshold_dbm: i16,
        far_threshold_dbm: i16,
    },
    InsufficientSeparation {
        required_db: i16,
        observed_db: i16,
    },
}

impl Display for CalibrationProfileError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRssi { field, value } => {
                write!(formatter, "{field:?} has invalid RSSI {value}")
            }
            Self::MinimumSeparationNotPositive { value } => write!(
                formatter,
                "profile minimum separation must be positive, received {value} dB"
            ),
            Self::ThresholdsOutOfOrder {
                near_threshold_dbm,
                far_threshold_dbm,
            } => write!(
                formatter,
                "near threshold {near_threshold_dbm} must exceed far threshold {far_threshold_dbm}"
            ),
            Self::InsufficientSeparation {
                required_db,
                observed_db,
            } => write!(
                formatter,
                "profile threshold separation is {observed_db} dB; at least {required_db} dB is required"
            ),
        }
    }
}

impl Error for CalibrationProfileError {}

fn validate_profile_rssi(
    field: CalibrationProfileField,
    value: i16,
) -> Result<(), CalibrationProfileError> {
    RssiDbm::try_new(value)
        .map(|_| ())
        .map_err(|_| CalibrationProfileError::InvalidRssi { field, value })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleGroup {
    Near,
    Far,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationError {
    InsufficientSamples {
        group: SampleGroup,
        required: usize,
        actual: usize,
    },
    InvalidRssi {
        group: SampleGroup,
        index: usize,
        value: i16,
    },
    OverlappingDistributions,
    InsufficientSeparation {
        required_db: i16,
        observed_db: i16,
    },
    InvalidProfile(CalibrationProfileError),
}

impl Display for CalibrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsufficientSamples {
                group,
                required,
                actual,
            } => write!(
                formatter,
                "{group:?} calibration requires {required} samples, received {actual}"
            ),
            Self::InvalidRssi {
                group,
                index,
                value,
            } => write!(
                formatter,
                "{group:?} sample {index} has invalid RSSI {value}"
            ),
            Self::OverlappingDistributions => {
                write!(formatter, "near and far calibration distributions overlap")
            }
            Self::InsufficientSeparation {
                required_db,
                observed_db,
            } => write!(
                formatter,
                "calibration separation is {observed_db} dB; at least {required_db} dB is required"
            ),
            Self::InvalidProfile(error) => {
                write!(formatter, "invalid calibration profile: {error}")
            }
        }
    }
}

impl Error for CalibrationError {}

pub fn calibrate(
    near_samples: &[i16],
    far_samples: &[i16],
    policy: &CalibrationPolicy,
) -> Result<CalibrationProfile, CalibrationError> {
    ensure_sample_count(near_samples, SampleGroup::Near, policy.minimum_samples)?;
    ensure_sample_count(far_samples, SampleGroup::Far, policy.minimum_samples)?;

    let near = sorted_valid_samples(near_samples, SampleGroup::Near)?;
    let far = sorted_valid_samples(far_samples, SampleGroup::Far)?;
    let near_threshold_dbm = percentile(&near, policy.near_percentile);
    let far_threshold_dbm = percentile(&far, policy.far_percentile);

    if near_threshold_dbm <= far_threshold_dbm {
        return Err(CalibrationError::OverlappingDistributions);
    }

    let observed_db = near_threshold_dbm - far_threshold_dbm;
    if observed_db < policy.minimum_separation_db {
        return Err(CalibrationError::InsufficientSeparation {
            required_db: policy.minimum_separation_db,
            observed_db,
        });
    }

    CalibrationProfile::new(
        median(&near),
        median(&far),
        near_threshold_dbm,
        far_threshold_dbm,
        policy.minimum_separation_db,
    )
    .map_err(CalibrationError::InvalidProfile)
}

fn ensure_sample_count(
    samples: &[i16],
    group: SampleGroup,
    required: usize,
) -> Result<(), CalibrationError> {
    if samples.len() < required {
        Err(CalibrationError::InsufficientSamples {
            group,
            required,
            actual: samples.len(),
        })
    } else {
        Ok(())
    }
}

fn sorted_valid_samples(samples: &[i16], group: SampleGroup) -> Result<Vec<i16>, CalibrationError> {
    let mut sorted = Vec::with_capacity(samples.len());
    for (index, value) in samples.iter().copied().enumerate() {
        let rssi = RssiDbm::try_new(value).map_err(|_| CalibrationError::InvalidRssi {
            group,
            index,
            value,
        })?;
        sorted.push(rssi.get());
    }
    sorted.sort_unstable();
    Ok(sorted)
}

fn percentile(sorted: &[i16], percentile: u8) -> i16 {
    let last_index = sorted.len() - 1;
    let index = last_index * usize::from(percentile) / 100;
    sorted[index]
}

fn median(sorted: &[i16]) -> i16 {
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        let pair_sum = i32::from(sorted[middle - 1]) + i32::from(sorted[middle]);
        pair_sum.div_euclid(2) as i16
    }
}
