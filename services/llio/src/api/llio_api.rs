pub(crate) const SERVER_NAME_LLIO: &str = "_Low Level I/O manager_";
// //////////////////////////////// VIBE
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum VibePattern {
    Short,
    Long,
    Double,
}
impl From<usize> for VibePattern {
    fn from(pattern: usize) -> Self {
        match pattern {
            0 => VibePattern::Long,
            1 => VibePattern::Double,
            _ => VibePattern::Short,
        }
    }
}
impl From<VibePattern> for usize {
    fn from(pat: VibePattern) -> usize {
        match pat {
            VibePattern::Long => 0,
            VibePattern::Double => 1,
            VibePattern::Short => 0xffff_ffff,
        }
    }
}

// ////////////////////////////// CLOCK GATING (placeholder)
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum ClockMode {
    Low,
    AllOn,
}
impl From<usize> for ClockMode {
    fn from(mode: usize) -> Self {
        match mode {
            0 => ClockMode::Low,
            _ => ClockMode::AllOn,
        }
    }
}
impl From<ClockMode> for usize {
    fn from(mode: ClockMode) -> usize {
        match mode {
            ClockMode::Low => 0,
            ClockMode::AllOn => 0xffff_ffff,
        }
    }
}
