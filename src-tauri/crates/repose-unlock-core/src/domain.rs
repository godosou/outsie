use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonoMillis(u64);

impl MonoMillis {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LockEpoch(u64);

impl LockEpoch {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuditSessionId(u32);

impl AuditSessionId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConsoleUid(u32);

impl ConsoleUid {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RssiDbm(i16);

impl RssiDbm {
    pub fn try_new(value: i16) -> Result<Self, RssiError> {
        if (-127..=-1).contains(&value) {
            Ok(Self(value))
        } else {
            Err(RssiError { value })
        }
    }

    #[must_use]
    pub const fn get(self) -> i16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RssiError {
    pub value: i16,
}

impl Display for RssiError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "RSSI {} is outside the valid -127..=-1 dBm range",
            self.value
        )
    }
}

impl Error for RssiError {}
