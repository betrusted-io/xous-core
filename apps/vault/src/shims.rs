// shims for Tock calls, to avoid having to recode some fairly pervasive patterns in OpenSK
use std::ops::{Sub, Add, AddAssign};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ClockFrequency {
    hz: usize,
}

impl ClockFrequency {
    pub fn hz(&self) -> usize {
        self.hz
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ClockValue {
    num_ticks: isize,
    clock_frequency: ClockFrequency,
}

impl ClockValue {
    pub const fn new(num_ticks: isize, clock_hz: usize) -> ClockValue {
        ClockValue {
            num_ticks,
            clock_frequency: ClockFrequency { hz: clock_hz },
        }
    }

    pub fn num_ticks(&self) -> isize {
        self.num_ticks
    }

    // Computes (value * factor) / divisor, even when value * factor >= isize::MAX.
    fn scale_int(value: isize, factor: isize, divisor: isize) -> isize {
        // As long as isize is not i64, this should be fine. If not, this is an alternative:
        // factor * (value / divisor) + ((value % divisor) * factor) / divisor
        ((value as i64 * factor as i64) / divisor as i64) as isize
    }

    pub fn ms(&self) -> isize {
        ClockValue::scale_int(self.num_ticks, 1000, self.clock_frequency.hz() as isize)
    }

    pub fn ms_f64(&self) -> f64 {
        1000.0 * (self.num_ticks as f64) / (self.clock_frequency.hz() as f64)
    }

    pub fn wrapping_add(self, duration: Duration<isize>) -> ClockValue {
        // This is a precision preserving formula for scaling an isize.
        let duration_ticks =
            ClockValue::scale_int(duration.ms, self.clock_frequency.hz() as isize, 1000);
        ClockValue {
            num_ticks: self.num_ticks.wrapping_add(duration_ticks),
            clock_frequency: self.clock_frequency,
        }
    }

    pub fn wrapping_sub(self, other: ClockValue) -> Option<Duration<isize>> {
        if self.clock_frequency == other.clock_frequency {
            let clock_duration = ClockValue {
                num_ticks: self.num_ticks - other.num_ticks,
                clock_frequency: self.clock_frequency,
            };
            Some(Duration::from_ms(clock_duration.ms()))
        } else {
            None
        }
    }
}


#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Duration<T> {
    ms: T,
}

impl<T> Duration<T> {
    pub const fn from_ms(ms: T) -> Duration<T> {
        Duration { ms }
    }
}

impl<T> Duration<T>
where
    T: Copy,
{
    pub fn ms(&self) -> T {
        self.ms
    }
}

impl<T> Sub for Duration<T>
where
    T: Sub<Output = T>,
{
    type Output = Duration<T>;

    fn sub(self, other: Duration<T>) -> Duration<T> {
        Duration {
            ms: self.ms - other.ms,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Timestamp<T> {
    ms: T,
}

impl<T> Timestamp<T> {
    pub const fn from_ms(ms: T) -> Timestamp<T> {
        Timestamp { ms }
    }
}

impl<T> Timestamp<T>
where
    T: Copy,
{
    pub fn ms(&self) -> T {
        self.ms
    }
}

impl Timestamp<isize> {
    pub fn from_clock_value(value: ClockValue) -> Timestamp<isize> {
        Timestamp { ms: value.ms() }
    }
}

impl Timestamp<f64> {
    pub fn from_clock_value(value: ClockValue) -> Timestamp<f64> {
        Timestamp { ms: value.ms_f64() }
    }
}

impl<T> Sub for Timestamp<T>
where
    T: Sub<Output = T>,
{
    type Output = Duration<T>;

    fn sub(self, other: Timestamp<T>) -> Duration<T> {
        Duration::from_ms(self.ms - other.ms)
    }
}

impl<T> Add<Duration<T>> for Timestamp<T>
where
    T: Copy + Add<Output = T>,
{
    type Output = Timestamp<T>;

    fn add(self, duration: Duration<T>) -> Timestamp<T> {
        Timestamp {
            ms: self.ms + duration.ms(),
        }
    }
}

impl<T> AddAssign<Duration<T>> for Timestamp<T>
where
    T: Copy + AddAssign,
{
    fn add_assign(&mut self, duration: Duration<T>) {
        self.ms += duration.ms();
    }
}
