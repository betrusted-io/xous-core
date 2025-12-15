pub const FREQ_OSC_MHZ: u32 = 48;
const FD_MAX: u32 = 256;

/// This takes in the FD input frequency (the frequency to be divided) in MHz
/// and the fd value, and returns the resulting divided frequency.
pub fn divide_by_fd(fd: u32, in_freq_hz: u32) -> u32 {
    // shift by 1_000 to prevent overflow
    let in_freq_khz = in_freq_hz / 1_000;
    let out_freq_khz = (in_freq_khz * (fd + 1)) / FD_MAX;
    // restore to Hz
    out_freq_khz * 1_000
}

/// Takes in the FD input frequency in MHz, and then the desired frequency.
/// Returns Some((fd value, deviation in *hz*, not MHz)) if the requirement is satisfiable
/// Returns None if the equation is ill-formed.
/// *not tested*
#[allow(dead_code)]
pub fn clk_to_fd(fd_in_mhz: u32, desired_mhz: u32) -> Option<(u32, i32)> {
    let platonic_fd: u32 = (desired_mhz * 256) / fd_in_mhz;
    if platonic_fd > 0 {
        let actual_fd = platonic_fd - 1;
        let actual_clk = divide_by_fd(actual_fd, fd_in_mhz * 1_000_000);
        Some((actual_fd, desired_mhz as i32 - actual_clk as i32))
    } else {
        None
    }
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum PowerOp {
    Wfi,
    Invalid,
}

const FDW: u32 = 8;
const MODULUS: u64 = 1 << FDW; // 256

#[derive(Debug, Clone, Copy)]
pub struct ClockDividerParams {
    pub fd0: u8,
    pub fd2: u8,
    pub actual_freq_hz: u32,
    pub error_ppm: u32,
}

/// Integer division with rounding: round(a / b)
#[inline]
pub fn div_round(a: u64, b: u64) -> u64 { (a + b / 2) / b }

/// Find optimal fd0/fd2 using only integer arithmetic.
///
/// Equation: f_out = f_base × (fd2 + 1) / ((fd0 + 1) × 256)
/// Valid range: f_base/65536 ≤ f_target ≤ f_base
pub fn find_optimal_divider(base_freq_hz: u32, target_freq_hz: u32) -> Option<ClockDividerParams> {
    let base = base_freq_hz as u64;
    let target = target_freq_hz as u64;

    if target == 0 || base == 0 {
        return None;
    }

    // Check bounds: max_div = 65536, min_div = 1
    // target must be in [base/65536, base]
    if target > base || base > target * 65536 {
        return None;
    }

    let mut best: Option<ClockDividerParams> = None;

    for fd0 in 0u32..=255 {
        let fd0_plus_1 = (fd0 as u64) + 1;

        // Solve: target/base = (fd2+1) / ((fd0+1) × 256)
        //    =>  fd2+1 = target × (fd0+1) × 256 / base
        let fd2_plus_1 = div_round(target * fd0_plus_1 * MODULUS, base);

        if fd2_plus_1 < 1 || fd2_plus_1 > 256 {
            continue;
        }

        let fd2 = (fd2_plus_1 - 1) as u8;

        // actual = base × (fd2+1) / ((fd0+1) × 256)
        let divisor = fd0_plus_1 * MODULUS;
        let actual_freq_hz = div_round(base * fd2_plus_1, divisor) as u32;

        // error_ppm = |actual - target| / target × 1_000_000
        //           = |base×(fd2+1) - target×divisor| × 1_000_000 / (target × divisor)
        let actual_scaled = base * fd2_plus_1;
        let target_scaled = target * divisor;
        let diff = if actual_scaled >= target_scaled {
            actual_scaled - target_scaled
        } else {
            target_scaled - actual_scaled
        };
        let error_ppm = div_round(diff * 1_000_000, target_scaled) as u32;

        let is_better = match &best {
            None => true,
            Some(b) => error_ppm < b.error_ppm || (error_ppm == b.error_ppm && fd0 < b.fd0 as u32),
        };

        if is_better {
            best = Some(ClockDividerParams { fd0: fd0 as u8, fd2, actual_freq_hz, error_ppm });
        }
    }

    best
}

const FREF_HZ: u64 = 48_000_000;
const VCO_MIN_HZ: u64 = 1_000_000_000;
const VCO_MAX_HZ: u64 = 3_000_000_000;
const FRAC_BITS: u32 = 24;
const FRAC_SCALE: u64 = 1 << FRAC_BITS; // 16777216

#[derive(Debug, Clone, Copy)]
pub struct PllParams {
    pub m: u8,     // prediv: 1-4 (for 48 MHz ref)
    pub n: u16,    // fbdiv: 8-4095 (excluding 11)
    pub frac: u32, // 0 to 2^24-1
    pub q0: u8,    // postdiv0: 1-8
    pub q1: u8,    // postdiv1: 1-8
    pub vco_freq_hz: u32,
    pub actual_freq_hz: u32,
    pub error_ppm: u32,
}

/// Check if N is valid per spec: 8, 9, 10, 12, 13, ... 4095 (11 excluded)
#[inline]
fn is_valid_n(n: u16) -> bool { n >= 8 && n <= 4095 && n != 11 }

/// Calculate VCO frequency: Fvco = Fref × (N + frac/2^24) / M
/// Returns Hz, or None on overflow
fn calc_vco_hz(m: u8, n: u16, frac: u32) -> Option<u64> {
    // Fvco = Fref × (N × 2^24 + frac) / (M × 2^24)
    let n_plus_f_scaled = (n as u64) * FRAC_SCALE + (frac as u64);
    let numerator = FREF_HZ.checked_mul(n_plus_f_scaled)?;
    Some(numerator / ((m as u64) * FRAC_SCALE))
}

/// Check if VCO frequency is within valid range (1-3 GHz)
fn is_vco_valid(m: u8, n: u16, frac: u32) -> bool {
    calc_vco_hz(m, n, frac).map(|vco| vco >= VCO_MIN_HZ && vco <= VCO_MAX_HZ).unwrap_or(false)
}

/// Find optimal PLL parameters for target frequency.
///
/// If `allow_frac` is false, only integer solutions (frac=0) are considered.
/// If `allow_frac` is true, fractional solutions are allowed but integer
/// solutions are preferred when they achieve the same error.
pub fn find_pll_params(target_freq_hz: u32, allow_frac: bool) -> Option<PllParams> {
    let target = target_freq_hz as u64;

    if target == 0 {
        return None;
    }

    let mut best: Option<PllParams> = None;

    // TODO: reduce this search space somewhat? need to see if this overhead is acceptable.
    // alternatively we can use this to pre-compute params that are frequently re-used.

    // M is constrained by PFD frequency: 10 MHz ≤ 48 MHz / M ≤ 100 MHz
    // With Fref = 48 MHz: M ∈ {1, 2, 3, 4}
    for m in 1u8..=4 {
        for q0 in 1u8..=8 {
            for q1 in 1u8..=8 {
                let total_div = (m as u64) * (q0 as u64) * (q1 as u64);

                // From: Fout = Fref × (N + F) / (M × Q0 × Q1)
                // Solve: N + F = Fout × M × Q0 × Q1 / Fref
                //
                // In fixed point (scaled by 2^24):
                // N × 2^24 + frac = target × total_div × 2^24 / Fref

                let n_plus_f_scaled = div_round(target * total_div * FRAC_SCALE, FREF_HZ);

                let n_base = (n_plus_f_scaled / FRAC_SCALE) as u16;
                let frac_remainder = (n_plus_f_scaled % FRAC_SCALE) as u32;

                // Try integer solution first (frac = 0), then fractional if allowed
                let candidates: &[(u16, u32)] = if allow_frac {
                    &[
                        (n_base, 0),              // Round down, no frac
                        (n_base + 1, 0),          // Round up, no frac
                        (n_base, frac_remainder), // Exact fractional
                    ]
                } else {
                    &[(n_base, 0), (n_base + 1, 0)]
                };

                for &(n, frac) in candidates {
                    if !is_valid_n(n) {
                        continue;
                    }

                    if !is_vco_valid(m, n, frac) {
                        continue;
                    }

                    // Calculate actual output frequency
                    let vco_hz = calc_vco_hz(m, n, frac).unwrap();
                    // crate::println!("vco_hz {}", vco_hz);
                    let actual_hz = vco_hz / (q0 as u64) / (q1 as u64);
                    // crate::println!("pll0 freq {}, q0 {} q1 {}", actual_hz, q0, q1);

                    // Clamp to u32 range
                    if actual_hz > u32::MAX as u64 {
                        continue;
                    }

                    let actual_freq_hz = actual_hz as u32;
                    let vco_freq_hz = vco_hz as u32;

                    // Calculate error in PPM
                    let diff = actual_freq_hz.abs_diff(target_freq_hz) as u64;
                    let error_ppm = (diff * 1_000_000 / target) as u32;

                    let candidate = PllParams { m, n, frac, q0, q1, vco_freq_hz, actual_freq_hz, error_ppm };

                    // Determine if this candidate is better
                    let dominated = best.as_ref().is_some_and(|b| {
                        if error_ppm != b.error_ppm {
                            error_ppm > b.error_ppm
                        } else {
                            // Same error: prefer no frac, then lower Q (less jitter)
                            if frac > 0 && b.frac == 0 {
                                true
                            } else if frac == 0 && b.frac > 0 {
                                false
                            } else {
                                // Prefer lower total post-division (lower jitter)
                                (q0 as u16) * (q1 as u16) >= (b.q0 as u16) * (b.q1 as u16)
                            }
                        }
                    });

                    if !dominated {
                        best = Some(candidate);
                    }
                }
            }
        }
    }

    best
}
