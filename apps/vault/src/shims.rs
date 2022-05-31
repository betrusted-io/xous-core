// shims for Tock calls, to avoid having to recode some fairly pervasive patterns in OpenSK
use std::ops::{Sub, Add, AddAssign};


#[derive(Copy, Clone, Debug)]
pub struct ClockValue {
    num_ticks: i64,
}

impl ClockValue {
    pub const fn new(num_ticks: i64, _clock_hz: usize) -> ClockValue {
        ClockValue {
            num_ticks,
        }
    }

    pub fn num_ticks(&self) -> i64 {
        self.num_ticks
    }

    pub fn ms(&self) -> i64 {
        self.num_ticks
    }

    pub fn ms_f64(&self) -> f64 {
        self.num_ticks as f64
    }

    pub fn wrapping_add(self, duration: Duration<i64>) -> ClockValue {
        ClockValue {
            num_ticks: self.num_ticks.wrapping_add(duration.ms),
        }
    }

    pub fn wrapping_sub(self, other: ClockValue) -> Option<Duration<i64>> {
        Some(Duration::from_ms(self.num_ticks - other.num_ticks))
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

impl Timestamp<i64> {
    pub fn from_clock_value(value: ClockValue) -> Timestamp<i64> {
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
