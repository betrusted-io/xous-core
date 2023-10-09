#[cfg(any(feature = "precursor", feature = "renode"))]
#[macro_use]
pub mod precursor;
use core::ops::{Add, AddAssign};

#[cfg(any(feature = "precursor", feature = "renode"))]
pub use precursor::*;

#[cfg(any(
    not(target_os = "xous"),
    not(any(
        feature = "precursor",
        feature = "renode",
        feature = "atsama5d27",
        feature = "cramium-fpga",
        feature = "cramium-soc",
        not(target_os = "xous")
    ))
))]
pub mod hosted;
#[cfg(any(
    not(target_os = "xous"),
    not(any(
        feature = "precursor",
        feature = "renode",
        feature = "atsama5d27",
        feature = "cramium-fpga",
        feature = "cramium-soc",
        not(target_os = "xous")
    ))
))]
pub use hosted::*;

#[cfg(any(feature = "atsama5d27"))]
#[macro_use]
pub mod atsama5d2;
#[cfg(any(feature = "atsama5d27"))]
pub use atsama5d2::*;

#[cfg(any(feature = "cramium-fpga", feature = "cramium-soc"))]
#[macro_use]
pub mod cramium;
#[cfg(any(feature = "cramium-fpga", feature = "cramium-soc"))]
pub use cramium::*;

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub(crate) struct TimeoutExpiry(i64);
impl TimeoutExpiry {
    pub fn to_i64(&self) -> i64 {
        self.0
    }
}

impl Add<i64> for TimeoutExpiry {
    type Output = TimeoutExpiry;
    fn add(self, rhs: i64) -> Self::Output {
        TimeoutExpiry(self.0 + rhs)
    }
}

impl AddAssign<i64> for TimeoutExpiry {
    fn add_assign(&mut self, rhs: i64) {
        self.0 += rhs;
    }
}

impl core::fmt::Display for TimeoutExpiry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}ms", self.0)
    }
}

impl core::fmt::Debug for TimeoutExpiry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}ms", self.0)
    }
}

impl From<i64> for TimeoutExpiry {
    fn from(item: i64) -> Self {
        TimeoutExpiry(item)
    }
}

impl From<usize> for TimeoutExpiry {
    fn from(item: usize) -> Self {
        TimeoutExpiry(item as i64)
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum RequestKind {
    Sleep = 0,
    Timeout = 1,
}

#[derive(Eq)]
pub struct TimerRequest {
    pub(crate) msec: TimeoutExpiry,
    pub(crate) sender: xous::MessageSender,
    pub(crate) kind: RequestKind,
    pub(crate) data: usize,
}

impl core::fmt::Display for TimerRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "TimerRequest {{ msec: {}, {} }}", self.msec, self.sender)
    }
}

impl core::fmt::Debug for TimerRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "TimerRequest {{ msec: {}, {} }}", self.msec, self.sender)
    }
}

impl core::cmp::Ord for TimerRequest {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        if self.msec < other.msec {
            core::cmp::Ordering::Less
        } else if self.msec > other.msec {
            core::cmp::Ordering::Greater
        } else {
            self.sender.cmp(&other.sender)
        }
    }
}

impl core::cmp::PartialOrd for TimerRequest {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl core::cmp::PartialEq for TimerRequest {
    fn eq(&self, other: &Self) -> bool {
        self.msec == other.msec && self.sender == other.sender
    }
}
