//! FIFO - 8-deep fifo head/tail access. Cores halt on overflow/underflow.
//! - x16 r/w  fifo[0]
//! - x17 r/w  fifo[1]
//! - x18 r/w  fifo[2]
//! - x19 r/w  fifo[3]
//!
//! Quantum - core will halt until host-configured clock divider pules occurs,
//! or an external event comes in on a host-specified GPIO pin.
//! - x20 -/w  halt to quantum
//!
//! GPIO - note clear-on-0 semantics for bit-clear for data pins!
//!   This is done so we can do a shift-and-move without an invert to
//!   bitbang a data pin. Direction retains a more "conventional" meaning
//!   where a write of `1` to either clear or set will cause the action,
//!   as pin direction toggling is less likely to be in a tight inner loop.
//! - x21 r/w  write: (x26 & x21) -> gpio pins; read: gpio pins -> x21
//! - x22 -/w  (x26 & x22) -> `1` will set corresponding pin on gpio
//! - x23 -/w  (x26 & x23) -> `0` will clear corresponding pin on gpio
//! - x24 -/w  (x26 & x24) -> `1` will make corresponding gpio pin an output
//! - x25 -/w  (x26 & x25) -> `1` will make corresponding gpio pin an input
//! - x26 r/w  mask GPIO action outputs
//!
//! Events - operate on a shared event register. Bits [31:24] are hard-wired to FIFO
//! level flags, configured by the host; writes to bits [31:24] are ignored.
//! - x27 -/w  mask event sensitivity bits
//! - x28 -/w  `1` will set the corresponding event bit. Only [23:0] are wired up.
//! - x29 -/w  `1` will clear the corresponding event bit Only [23:0] are wired up.
//! - x30 r/-  halt until ((x27 & events) != 0), and return unmasked `events` value
//!
//! Core ID & debug:
//! - x31 r/-  [31:30] -> core ID; [29:0] -> cpu clocks since reset

#![cfg_attr(feature = "baremetal", no_std)]
use core::mem::size_of;

use utralib::generated::*;

#[cfg(feature = "tests")]
pub mod bio_tests;

pub mod i2c;

#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BioCore {
    Core0 = 0,
    Core1 = 1,
    Core2 = 2,
    Core3 = 3,
}

impl From<usize> for BioCore {
    fn from(value: usize) -> Self {
        match value {
            0 => BioCore::Core0,
            1 => BioCore::Core1,
            2 => BioCore::Core2,
            3 => BioCore::Core3,
            _ => panic!("Invalid BioCore value: {}", value),
        }
    }
}

#[derive(Debug)]
pub enum BioError {
    /// specified state machine is not valid
    InvalidSm,
    /// program can't fit in memory, for one reason or another
    Oom,
    /// no more machines available
    NoFreeMachines,
    /// Loaded code did not match, first error at argument
    CodeCheck(usize),
}

pub fn get_id() -> u32 {
    let bio_ss = BioSharedState::new();
    #[cfg(feature = "tests")]
    bio_tests::report_api(utra::bio_bdma::SFR_CFGINFO.offset() as u32);
    #[cfg(feature = "tests")]
    bio_tests::report_api(utra::bio_bdma::HW_BIO_BDMA_BASE as u32);
    bio_ss.bio.r(utra::bio_bdma::SFR_CFGINFO)
}

/// used to generate some test vectors
pub fn lfsr_next(state: u16) -> u16 {
    let bit = ((state >> 8) ^ (state >> 4)) & 1;

    ((state << 1) + bit) & 0x1_FF
}

/// used to generate some test vectors
pub fn lfsr_next_u32(state: u32) -> u32 {
    let bit = ((state >> 31) ^ (state >> 21) ^ (state >> 1) ^ (state >> 0)) & 1;

    (state << 1) + bit
}

pub const BIO_PRIVATE_MEM_LEN: usize = 4096;

pub struct BioSharedState {
    pub bio: CSR<u32>,
    pub imem_slice: [&'static mut [u32]; 4],
}
impl BioSharedState {
    #[cfg(feature = "baremetal")]
    pub fn new() -> Self {
        // map the instruction memory
        let imem_slice = unsafe {
            [
                core::slice::from_raw_parts_mut(
                    utralib::generated::HW_BIO_IMEM0_MEM as *mut u32,
                    HW_BIO_IMEM0_MEM_LEN / size_of::<u32>(),
                ),
                core::slice::from_raw_parts_mut(
                    utralib::generated::HW_BIO_IMEM1_MEM as *mut u32,
                    HW_BIO_IMEM1_MEM_LEN / size_of::<u32>(),
                ),
                core::slice::from_raw_parts_mut(
                    utralib::generated::HW_BIO_IMEM2_MEM as *mut u32,
                    HW_BIO_IMEM2_MEM_LEN / size_of::<u32>(),
                ),
                core::slice::from_raw_parts_mut(
                    utralib::generated::HW_BIO_IMEM3_MEM as *mut u32,
                    HW_BIO_IMEM3_MEM_LEN / size_of::<u32>(),
                ),
            ]
        };

        BioSharedState { bio: CSR::new(utra::bio_bdma::HW_BIO_BDMA_BASE as *mut u32), imem_slice }
    }

    #[cfg(not(feature = "baremetal"))]
    pub fn new() -> Self {
        let csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::bio_bdma::HW_BIO_BDMA_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        let imem0 = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_BIO_IMEM0_MEM),
            None,
            utralib::HW_BIO_IMEM0_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();
        let imem1 = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_BIO_IMEM1_MEM),
            None,
            utralib::HW_BIO_IMEM1_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();
        let imem2 = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_BIO_IMEM2_MEM),
            None,
            utralib::HW_BIO_IMEM2_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();
        let imem3 = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_BIO_IMEM3_MEM),
            None,
            utralib::HW_BIO_IMEM3_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        BioSharedState {
            bio: CSR::new(csr.as_mut_ptr() as *mut u32),
            imem_slice: unsafe {
                [imem0.as_slice_mut(), imem1.as_slice_mut(), imem2.as_slice_mut(), imem3.as_slice_mut()]
            },
        }
    }

    /// This will overwrite *all* of the current core states with
    /// the states specified in the `cores` argument, such that
    /// `true` indicates the core should be enabled and running.
    ///
    /// Notably, this will also turn off any cores that are marked
    /// as `false`. A different method needs to be written if
    /// we want to independently manipulate core states without
    /// affecting others. However, it's envisioned to be mostly
    /// the case that users will manage the full set of programs
    /// running on all four cores and not necessarily have a
    /// dynamically-loaded, multi-tenant situation, so this simpler
    /// API is more ergonomic to use than e.g. the generic case
    /// of passing Option<bool> for every core state to additionally
    /// specify if the core should be changed or left alone.
    #[inline(never)]
    pub fn set_core_run_states(&mut self, cores: [bool; 4]) {
        self.bio.wo(utra::bio_bdma::SFR_CTRL, 0); // turn off all the cores first
        let mut core_code = 0;
        for (i, &core) in cores.iter().enumerate() {
            if core {
                core_code |= 1 << i;
            }
        }
        let core_mask = core_code | core_code << 4 | core_code << 8;
        self.bio.wo(utra::bio_bdma::SFR_CTRL, core_mask);
        let mut timeout = 0;
        loop {
            let ctrl = self.bio.r(utra::bio_bdma::SFR_CTRL) & 0xFF0;
            if ctrl == 0 {
                break;
            }
            timeout += 1;
            if timeout > 1000 {
                crate::println!("Timeout on set_core_run_states: req {:x} != rbk {:x}", core_code, ctrl);
                break;
            }
        }
        let check = self.bio.r(utra::bio_bdma::SFR_CTRL);
        if check != core_code {
            crate::println!("run-state check failed: {:x}", check);
        }
    }

    pub fn init(&mut self) {
        // set clocking mode to 3
        self.bio.wfo(utra::bio_bdma::SFR_CONFIG_CLOCKING_MODE, 3);
        self.bio.wo(utra::bio_bdma::SFR_EXTCLOCK, 0);
        self.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x0_0000);
        self.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x0_0000);
        self.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x0_0000);
        self.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x0_0000);

        for core in 0..4 {
            // crate::println!("ldst trial");
            let core_num = 1 << (core as usize);
            self.load_code(mem_init_code(), 0, BioCore::from(core));
            self.bio.wo(
                utra::bio_bdma::SFR_CTRL,
                self.bio.r(utra::bio_bdma::SFR_CTRL) | (core_num | core_num << 4 | core_num << 8),
            );
            for _ in 0..16 {
                let _ = self.bio.r(utra::bio_bdma::SFR_RXF0);
                let _ = self.bio.r(utra::bio_bdma::SFR_RXF1);
                let _ = self.bio.r(utra::bio_bdma::SFR_RXF2);
                let _ = self.bio.r(utra::bio_bdma::SFR_RXF3);
            }
            // crate::println!("ldst trial end");
        }
        self.bio.wfo(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR, 0xf);

        self.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
        for imem in self.imem_slice.iter_mut() {
            // jump to current location
            imem.fill(0xA001_A001);
        }
        for (i, imem) in self.imem_slice.iter().enumerate() {
            for (j, &d) in imem.iter().enumerate() {
                if d != 0xA001_A001 {
                    crate::println!("imem{}[{:x}]: {:x}", i, j, d);
                }
            }
        }

        self.bio.wo(utra::bio_bdma::SFR_CTRL, 0xFFF);
        for _ in 0..16 {
            let _ = self.bio.r(utra::bio_bdma::SFR_RXF0);
            let _ = self.bio.r(utra::bio_bdma::SFR_RXF1);
            let _ = self.bio.r(utra::bio_bdma::SFR_RXF2);
            let _ = self.bio.r(utra::bio_bdma::SFR_RXF3);
        }
        self.bio.wfo(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR, 0xf);
        self.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
    }

    pub fn load_code(&mut self, prog: &[u8], offset_bytes: usize, core: BioCore) {
        // turn off just the target core
        let core_num = 1 << (core as usize);
        self.bio.wo(
            utra::bio_bdma::SFR_CTRL,
            self.bio.r(utra::bio_bdma::SFR_CTRL) & !(core_num | core_num << 4 | core_num << 8),
        );
        // crate::println!("load code from {:x}", prog.as_ptr() as usize);
        let offset = offset_bytes / core::mem::size_of::<u32>();
        for (i, chunk) in prog.chunks(4).enumerate() {
            if chunk.len() == 4 {
                let word: u32 = u32::from_le_bytes(chunk.try_into().unwrap());
                self.imem_slice[core as usize][i + offset] = word;
            } else {
                // copy the last word as a "ragged word"
                let mut ragged_word = 0;
                for (j, &b) in chunk.iter().enumerate() {
                    ragged_word |= (b as u32) << (4 - chunk.len() + j);
                }
                self.imem_slice[core as usize][i + offset] = ragged_word;
            }
        }
        match self.verify_code(&prog, offset_bytes, core) {
            Err(BioError::CodeCheck(offset)) => {
                crate::println!("Code verification error at {:x}", offset)
            }
            _ => (),
        }
    }

    pub fn verify_code(&mut self, prog: &[u8], offset_bytes: usize, core: BioCore) -> Result<(), BioError> {
        let offset = offset_bytes / core::mem::size_of::<u32>();
        for (i, chunk) in prog.chunks(4).enumerate() {
            if chunk.len() == 4 {
                let word: u32 = u32::from_le_bytes(chunk.try_into().unwrap());
                let rbk = self.imem_slice[core as usize][i + offset];
                if rbk != word {
                    // print!("{:?} expected {:x} got {:x} at {}\r", core, word, rbk, i + offset);
                    return Err(BioError::CodeCheck(i + offset));
                }
            } else {
                // copy the last word as a "ragged word"
                let mut ragged_word = 0;
                for (j, &b) in chunk.iter().enumerate() {
                    ragged_word |= (b as u32) << (4 - chunk.len() + j);
                }
                if self.imem_slice[core as usize][i + offset] != ragged_word {
                    return Err(BioError::CodeCheck(i + offset));
                };
            }
        }
        Ok(())
    }

    pub fn set_pin(&mut self, pin: u32, state: bool, core: Option<BioCore>) {
        let target_core = core.unwrap_or(BioCore::Core0);
        let core_mask = 1 << (target_core as usize);

        // --- Debug ---
        crate::println!("\n--- Running set_pin for {:?} ---", target_core);
        let initial_ctrl_state = self.bio.r(utra::bio_bdma::SFR_CTRL);
        crate::println!("[0] Initial SFR_CTRL value: {:#010x}", initial_ctrl_state);
        // --- End Debug ---

        // 1. Stop the target core, leaving other cores running.
        let ctrl_with_target_stopped = initial_ctrl_state & !core_mask;
        self.bio.wo(utra::bio_bdma::SFR_CTRL, ctrl_with_target_stopped);

        // --- Debug ---
        crate::println!("[1] Stopping core with EN mask: {:#010x}", core_mask);
        crate::println!("    SFR_CTRL after stop:    {:#010x}", ctrl_with_target_stopped);
        // --- End Debug ---

        // 2. Clear the target core's FIFO.
        self.bio.wfo(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR, core_mask as u32);

        // --- Debug ---
        crate::println!("[2] Cleared FIFO for core with mask: {:#010x}", core_mask);
        // --- End Debug ---

        // 3. Load the correct, dedicated program for the target core.
        let prog = match target_core {
            BioCore::Core0 => pin_control_core0(),
            BioCore::Core1 => pin_control_core1(),
            BioCore::Core2 => pin_control_core2(),
            BioCore::Core3 => pin_control_core3(),
        };
        self.load_code(prog, 0, target_core);

        // --- Debug ---
        crate::println!("[3] Loaded dedicated program onto {:?}", target_core);
        // --- End Debug ---

        // 4. Calculate the start mask for the target core AND reset Core 0's clocks.
        let target_start_mask = self.bio.ms(utra::bio_bdma::SFR_CTRL_EN, core_mask);
        // | self.bio.ms(utra::bio_bdma::SFR_CTRL_RESTART, core_mask)
        self.bio.ms(utra::bio_bdma::SFR_CTRL_CLKDIV_RESTART, core_mask);
        let final_start_mask = target_start_mask;
        let final_ctrl_state = ctrl_with_target_stopped | final_start_mask;

        // --- Debug Lines Start ---
        // crate::println!("--- Mask Calculation ---");
        // crate::println!("  Target Core Mask (raw):   {:012b}", core_mask);
        // crate::println!("  Target Start Mask:        {:012b}", target_start_mask);
        // crate::println!("  Final Start Mask (+C0):   {:012b}", final_start_mask);
        // crate::println!("  Previous CTRL State:      {:012b}", ctrl_with_target_stopped);
        // crate::println!("  Final CTRL Value to Write:{:012b}", final_ctrl_state);
        // // --- Debug Lines End ---

        self.bio.wo(utra::bio_bdma::SFR_CTRL, final_ctrl_state);

        // --- Debug ---
        crate::println!("[4] Starting core with final_start_mask: {:012b}", final_start_mask);
        crate::println!("    SFR_CTRL after start:     {:012b}", final_ctrl_state);
        // --- End Debug ---

        // 5. Reset the clock divider for the selected core.
        match target_core {
            BioCore::Core0 => self.bio.wo(utra::bio_bdma::SFR_QDIV0, 0),
            BioCore::Core1 => self.bio.wo(utra::bio_bdma::SFR_QDIV1, 0),
            BioCore::Core2 => self.bio.wo(utra::bio_bdma::SFR_QDIV2, 0),
            BioCore::Core3 => self.bio.wo(utra::bio_bdma::SFR_QDIV3, 0),
        }

        // 6. Prepare and send the command to the target core's FIFO.
        let state_val = if state { 0xFFFFFFFF } else { 0 };
        let pin_bitmask = 1 << pin;

        // --- Debug ---
        // MODIFIED: Removed `format!` macro.
        crate::println!("[5] Sending command to FIFO for core {}:", target_core as usize);
        crate::println!("    pin_bitmask: {:#010x}, state_val: {:#010x}", pin_bitmask, state_val);
        // --- End Debug ---

        match target_core {
            BioCore::Core0 => {
                self.bio.wo(utra::bio_bdma::SFR_TXF0, pin_bitmask);
                self.bio.wo(utra::bio_bdma::SFR_TXF0, state_val);
            }
            BioCore::Core1 => {
                self.bio.wo(utra::bio_bdma::SFR_TXF1, pin_bitmask);
                self.bio.wo(utra::bio_bdma::SFR_TXF1, state_val);
            }
            BioCore::Core2 => {
                self.bio.wo(utra::bio_bdma::SFR_TXF2, pin_bitmask);
                self.bio.wo(utra::bio_bdma::SFR_TXF2, state_val);
            }
            BioCore::Core3 => {
                self.bio.wo(utra::bio_bdma::SFR_TXF3, pin_bitmask);
                self.bio.wo(utra::bio_bdma::SFR_TXF3, state_val);
            }
        }
    }

    // Add these two new functions to your `impl BioSharedState` block.

    /// Starts a continuous square wave on a given pin using a dedicated BIO core.
    ///
    /// This function is NOT cooperative. The core it uses will take exclusive
    /// control of the GPIO data bus via the `x21` register.
    ///
    /// # Arguments
    ///
    /// * `pin` - The GPIO pin number to toggle (0-31).
    /// * `core` - The `BioCore` to run the generator program on.
    /// * `clock_divisor` - The value for the `QDIV` register to set the frequency.
    /// * `delay_count` - The number of BIO clock cycles for each half-period (high/low).
    pub fn start_wave_generator(&mut self, pin: u32, core: BioCore, clock_divisor: u32, delay_count: u32) {
        let core_mask = 1 << (core as usize);

        // 1. Stop the target core to ensure a clean state.
        let mut ctrl = self.bio.r(utra::bio_bdma::SFR_CTRL);
        ctrl &= !core_mask;
        self.bio.wo(utra::bio_bdma::SFR_CTRL, ctrl);

        // 2. Clear any stale data from the core's FIFO.
        self.bio.wfo(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR, core_mask as u32);

        // 3. Load the slow wave generator program.
        let prog = slow_wave_generator_code();
        self.load_code(prog, 0, core);

        // 4. Start the core.
        // Note: This uses a simple start and does not include the master clock reset
        // from the cooperative pin_set function, as this function is exclusive.
        let start_mask = core_mask | (core_mask << 4) | (core_mask << 8);
        ctrl |= start_mask;
        self.bio.wo(utra::bio_bdma::SFR_CTRL, ctrl);

        // 5. Set the clock divider for the selected core.
        match core {
            BioCore::Core0 => self.bio.wo(utra::bio_bdma::SFR_QDIV0, clock_divisor),
            BioCore::Core1 => self.bio.wo(utra::bio_bdma::SFR_QDIV1, clock_divisor),
            BioCore::Core2 => self.bio.wo(utra::bio_bdma::SFR_QDIV2, clock_divisor),
            BioCore::Core3 => self.bio.wo(utra::bio_bdma::SFR_QDIV3, clock_divisor),
        }

        // 6. Send parameters to the BIO core via its FIFO.
        let pin_mask = 1 << pin;
        match core {
            BioCore::Core0 => {
                self.bio.wo(utra::bio_bdma::SFR_TXF0, pin_mask);
                self.bio.wo(utra::bio_bdma::SFR_TXF0, delay_count);
            }
            BioCore::Core1 => {
                self.bio.wo(utra::bio_bdma::SFR_TXF1, pin_mask);
                self.bio.wo(utra::bio_bdma::SFR_TXF1, delay_count);
            }
            BioCore::Core2 => {
                self.bio.wo(utra::bio_bdma::SFR_TXF2, pin_mask);
                self.bio.wo(utra::bio_bdma::SFR_TXF2, delay_count);
            }
            BioCore::Core3 => {
                self.bio.wo(utra::bio_bdma::SFR_TXF3, pin_mask);
                self.bio.wo(utra::bio_bdma::SFR_TXF3, delay_count);
            }
        }
    }

    /// Stops a BIO core that is running a program (like the wave generator).
    ///
    /// # Arguments
    ///
    /// * `core` - The `BioCore` to halt.
    pub fn stop_wave_generator(&mut self, core: BioCore) {
        let core_mask = 1 << (core as usize);
        let mut ctrl = self.bio.r(utra::bio_bdma::SFR_CTRL);
        // Clear only the enable bit for the target core.
        ctrl &= !core_mask;
        self.bio.wo(utra::bio_bdma::SFR_CTRL, ctrl);
    }

    pub fn debug_pc(&self) {
        crate::println!(
            "c0:{:04x} c1:{:04x} c2:{:04x} c3:{:04x}",
            self.bio.r(utra::bio_bdma::SFR_DBG0),
            self.bio.r(utra::bio_bdma::SFR_DBG1),
            self.bio.r(utra::bio_bdma::SFR_DBG2),
            self.bio.r(utra::bio_bdma::SFR_DBG3),
        );
    }

    pub fn debug_fifo(&self) {
        crate::println!(
            "f0:{:04x} f1:{:04x} f2:{:04x} f3:{:04x}",
            self.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0),
            self.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1),
            self.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2),
            self.bio.rf(utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3),
        );
    }
}

#[macro_export]
/// This macro takes three identifiers and assembly code:
///   - name of the function to call to retrieve the assembled code
///   - a unique identifier that serves as label name for the start of the code
///   - a unique identifier that serves as label name for the end of the code
///   - a comma separated list of strings that form the assembly itself
///
///   *** The comma separated list must *not* end in a comma. ***
///
///   The macro is unable to derive names of functions or identifiers for labels
///   due to the partially hygienic macro rules of Rust, so you have to come
///   up with a list of unique names by yourself.
macro_rules! bio_code {
    ($fn_name:ident, $name_start:ident, $name_end:ident, $($item:expr),*) => {
        pub fn $fn_name() -> &'static [u8] {
            extern "C" {
                static $name_start: *const u8;
                static $name_end: *const u8;
            }
            /*
            unsafe {
                report_api($name_start as u32);
                report_api($name_end as u32);
            }
            */
            // skip the first 4 bytes, as they contain the loading offset
            unsafe { core::slice::from_raw_parts($name_start.add(4), ($name_end as usize) - ($name_start as usize) - 4)}
        }

        core::arch::global_asm!(
            ".align 4",
            concat!(".globl ", stringify!($name_start)),
            concat!(stringify!($name_start), ":"),
            ".word .",
            $($item),*
            , ".align 4",
            concat!(".globl ", stringify!($name_end)),
            concat!(stringify!($name_end), ":"),
            ".word .",
        );
    };
}

#[rustfmt::skip]
bio_code!(mem_init_code, MEM_INIT_START, MEM_INIT_END,
    "sw x0, 0x20(x0)",
    "li sp, 0x61200000",
    "addi sp, sp, -4",
    "sw x0, 0(sp)",
  "10:",
    "j 10b"
);

#[rustfmt::skip]
bio_code!(
    pin_control_core0,
    PIN_CONTROL_START_0,
    PIN_CONTROL_END_0,
    // For Core 0: Reads from fifo[0] (x16)
    "li    t0, 0xFFFFFFFF",
    "mv    x24, t0",
"80:",
    "mv    t1, x16",        // Read pin_mask
    "mv    t2, x16",        // Read state_val
    "mv    x26, t1",        // Set write-mask to the target pin
    "mv    x21, t2",        // Write state, masked by x26
    "j     80b"
);

#[rustfmt::skip]
bio_code!(
    pin_control_core1,
    PIN_CONTROL_START_1,
    PIN_CONTROL_END_1,
    // For Core 1: Reads from fifo[1] (x17)
    "li    t0, 0xFFFFFFFF",
    "mv    x24, t0",
"82:",
    "mv    t1, x17",        // Read pin_mask
    "mv    t2, x17",        // Read state_val
    "mv    x26, t1",        // Set write-mask to the target pin
    "mv    x21, t2",        // Write state, masked by x26
    "j     82b"
);

#[rustfmt::skip]
bio_code!(
    pin_control_core2,
    PIN_CONTROL_START_2,
    PIN_CONTROL_END_2,
    // For Core 2: Reads from fifo[2] (x18)
    "li    t0, 0xFFFFFFFF",
    "mv    x24, t0",
"84:",
    "mv    t1, x18",        // Read pin_mask
    "mv    t2, x18",        // Read state_val
    "mv    x26, t1",        // Set write-mask to the target pin
    "mv    x21, t2",        // Write state, masked by x26
    "j     84b"
);

#[rustfmt::skip]
bio_code!(
    pin_control_core3,
    PIN_CONTROL_START_3,
    PIN_CONTROL_END_3,
    // For Core 3: Reads from fifo[3] (x19)
    "li    t0, 0xFFFFFFFF",
    "mv    x24, t0",
"86:",
    "mv    t1, x19",        // Read pin_mask
    "mv    t2, x19",        // Read state_val
    "mv    x26, t1",        // Set write-mask to the target pin
    "mv    x21, t2",        // Write state, masked by x26
    "j     86b"
);

#[rustfmt::skip]
bio_code!(slow_wave_generator_code, SLOW_WAVE_START, SLOW_WAVE_END,
    // Configure all GPIOs as outputs.
    "li    t0, 0xFFFFFFFF",
    "mv    x24, t0",
    // Read the pin mask from FIFO0 into t1.
    "mv    t1, x16",
    // Read the delay count from FIFO0 into t2.
    "mv    t2, x16",
    // Set the GPIO mask register to the pin mask.
    "mv    x26, t1",
  "10:", // Main loop
    // --- HIGH PULSE ---
    "mv    x21, t1",      // Set pin high
    "mv    t3, t2",       // Load counter into t3 for the delay loop
  "11:", // Delay loop 1
    "mv    x20, zero",      // << Wait for one (slow) BIO clock cycle
    "addi  t3, t3, -1",   // Decrement counter
    "bne   t3, zero, 11b",  // Loop if not zero
    // --- LOW PULSE ---
    "mv    x21, zero",    // Set pin low
    "mv    t3, t2",       // Re-load counter for the delay loop
  "12:", // Delay loop 2
    "mv    x20, zero",      // << Wait for one (slow) BIO clock cycle
    "addi  t3, t3, -1",   // Decrement counter
    "bne   t3, zero, 12b",  // Loop if not zero
    "j     10b"           // Repeat the whole cycle
);
