#[cfg(any(feature="precursor", feature="renode"))]
#[macro_use]
pub mod precursor;
#[cfg(any(feature="precursor", feature="renode"))]
pub use precursor::*;

#[cfg(any(not(target_os = "xous"),
    not(any(feature="precursor", feature="renode", feature="atsama5d27", not(target_os = "xous")))
))]
pub mod hosted;
#[cfg(any(not(target_os = "xous"),
    not(any(feature="precursor", feature="renode", feature="atsama5d27", not(target_os = "xous")))
))]
pub use hosted::*;

pub(crate) type TimeoutExpiry = i64;

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
