pub mod lightgenes;

/// 15-bit signed integer + 16-bit fraction
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FpQ16_16(pub i32);

#[allow(dead_code)]
impl FpQ16_16 {
    pub fn frac_bits() -> u32 { 16 }

    pub fn frac_scale() -> i32 { 1 << Self::frac_bits() }

    pub fn from_int(x: i32) -> Self { Self(x << Self::frac_bits()) }

    pub fn from_float(x: f32) -> Self { Self((x * Self::frac_scale() as f32 + 0.5) as i32) }

    pub fn to_int(self) -> i32 { self.0 >> Self::frac_bits() }

    pub fn to_float(self) -> f32 { self.0 as f32 / Self::frac_scale() as f32 }

    /// Wrap a raw i32 coming directly off the hardware register.
    pub fn from_raw(raw: i32) -> Self { Self(raw) }
}
