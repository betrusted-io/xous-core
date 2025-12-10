pub const FREQ_OSC_MHZ: u32 = 48;

/// This takes in the FD input frequency (the frequency to be divided) in MHz
/// and the fd value, and returns the resulting divided frequency.
/// *not tested*
#[allow(dead_code)]
pub fn fd_to_clk(fd_in_mhz: u32, fd_val: u32) -> u32 { (fd_in_mhz * (fd_val + 1)) / 256 }

/// Takes in the FD input frequencyin MHz, and then the desired frequency.
/// Returns Some((fd value, deviation in *hz*, not MHz)) if the requirement is satisfiable
/// Returns None if the equation is ill-formed.
/// *not tested*
#[allow(dead_code)]
pub fn clk_to_fd(fd_in_mhz: u32, desired_mhz: u32) -> Option<(u32, i32)> {
    let platonic_fd: u32 = (desired_mhz * 256) / fd_in_mhz;
    if platonic_fd > 0 {
        let actual_fd = platonic_fd - 1;
        let actual_clk = fd_to_clk(fd_in_mhz, actual_fd);
        Some((actual_fd, desired_mhz as i32 - actual_clk as i32))
    } else {
        None
    }
}

/// Takes in the top clock in MHz, desired perclk in MHz, and returns a tuple of
/// (min cycle, fd, actual freq)
/// *tested*
pub fn clk_to_per(top_in_mhz: u32, perclk_in_mhz: u32) -> Option<(u8, u8, u32)> {
    let fd_platonic = ((256 * perclk_in_mhz) / (top_in_mhz / 2)).min(256);
    if fd_platonic > 0 {
        let fd = fd_platonic - 1;
        let min_cycle = (2 * (256 / (fd + 1))).max(1);
        let min_freq = top_in_mhz / min_cycle;
        let target_freq = top_in_mhz * (fd + 1) / 512;
        let actual_freq = target_freq.max(min_freq);
        if fd < 256 && min_cycle < 256 && min_cycle > 0 {
            Some(((min_cycle - 1) as u8, fd as u8, actual_freq))
        } else {
            None
        }
    } else {
        None
    }
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum PowerOp {
    Wfi,
    Invalid,
}
