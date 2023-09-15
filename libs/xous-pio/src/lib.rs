#![cfg_attr(target_os = "none", no_std)]

use pio::Program;
use pio::RP2040_MAX_PROGRAM_SIZE;

use utralib::generated::*;
use utralib::generated::utra::rp_pio;

#[cfg(feature="tests")]
pub mod pio_tests;

#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum PioRawIntSource {
    Sm3         = 0b1000_0000_0000,
    Sm2         = 0b0100_0000_0000,
    Sm1         = 0b0010_0000_0000,
    Sm0         = 0b0001_0000_0000,
    TxNotFull3  = 0b0000_1000_0000,
    TxNotFull2  = 0b0000_0100_0000,
    TxNotFull1  = 0b0000_0010_0000,
    TxNotFull0  = 0b0000_0001_0000,
    RxNotEmpty3 = 0b0000_0000_1000,
    RxNotEmpty2 = 0b0000_0000_0100,
    RxNotEmpty1 = 0b0000_0000_0010,
    RxNotEmpty0 = 0b0000_0000_0001,
}
#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum PioIntSource {
    Sm,
    TxNotFull,
    RxNotEmpty,
}
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u32)]
pub enum MovStatusType {
    StatusTxLessThan = 0,
    StatusRxLessThan = 1,
}
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum PioFifoJoin {
    None = 0,
    JoinTx = 1,
    JoinRx = 2,
}
#[derive(Debug)]
pub enum PioError {
    /// specified state machine is not valid
    InvalidSm,
    /// program can't fit in memory, for one reason or another
    Oom,
    /// no more machines available
    NoFreeMachines,
}

#[derive(Debug)]
pub struct SmConfig {
    pub clkdiv: u32,
    pub execctl: u32,
    pub shiftctl: u32,
    pub pinctl: u32,
}
impl SmConfig {
    pub fn default() -> SmConfig {
        // FIXME: use "proper" getters and setters to create the default config.
        SmConfig {
            clkdiv: 0x1_0000,
            execctl: 31 << 12,
            shiftctl: (1 << 18) | (1 << 19),
            pinctl: 0,
        }
    }
}

pub fn get_id() -> u32 {
    let pio_ss = PioSharedState::new();
    #[cfg(feature="tests")]
    pio_tests::report_api(rp_pio::SFR_DBG_CFGINFO.offset() as u32);
    #[cfg(feature="tests")]
    pio_tests::report_api(rp_pio::HW_RP_PIO_BASE as u32);
    pio_ss.pio.r(rp_pio::SFR_DBG_CFGINFO)
}

#[derive(Debug)]
pub struct LoadedProg {
    program: Program::<RP2040_MAX_PROGRAM_SIZE>,
    offset: usize,
    entry_point: Option<usize>,
}
impl LoadedProg {
    pub fn load(program: Program::<RP2040_MAX_PROGRAM_SIZE>, pio_sm: &mut PioSharedState) -> Result<Self, PioError> {
        let offset = pio_sm.add_program(&program)?;
        Ok({
            LoadedProg {
                program,
                offset: offset as usize,
                entry_point: None,
            }
        })
    }
    pub fn load_with_entrypoint(program: Program::<RP2040_MAX_PROGRAM_SIZE>, entry_point: usize, pio_sm: &mut PioSharedState) -> Result<Self, PioError> {
        let offset = pio_sm.add_program(&program)?;
        Ok({
            LoadedProg {
                program,
                offset: offset as usize,
                entry_point: Some(entry_point)
            }
        })
    }
    pub fn entry(&self) -> usize {
        if let Some(ep) = self.entry_point {
            ep + self.offset
        } else {
            self.start()
        }
    }
    pub fn start(&self) -> usize {
        self.program.wrap.target as usize + self.offset
    }
    pub fn end(&self) -> usize {
        self.program.wrap.source as usize + self.offset
    }
    pub fn setup_default_config(&self, pio_sm: &mut PioSm) {
        pio_sm.config_set_defaults();
        pio_sm.config_set_wrap(self.start(), self.end());
        if self.program.side_set.bits() > 0 {
            pio_sm.config_set_sideset(
                self.program.side_set.bits() as usize,
                self.program.side_set.optional(),
                self.program.side_set.pindirs(),
            )
        }
    }
}

pub struct PioSharedState {
    pub pio: CSR<u32>,
    // using a 32-bit wide bitmask to track used locations pins this implementation
    // to a 32-instruction PIO memory. ¯\_(ツ)_/¯
    // 0 means unused; 1 means used. LSB is lowest address.
    used_mask: u32,
    used_machines: [bool; 4],
}
impl PioSharedState {
    #[cfg(all(not(target_os="xous"),not(feature="rp2040")))]
    pub fn new() -> Self {
        PioSharedState {
            pio: CSR::new(rp_pio::HW_RP_PIO_BASE as *mut u32),
            used_mask: 0,
            used_machines: [false; 4],
        }
    }
    #[cfg(all(not(target_os="xous"),feature="rp2040"))]
    pub fn new() -> Self {
        crate::pio_tests::report_api(0x5020_0000);
        PioSharedState {
            pio: CSR::new(0x5020_0000 as *mut u32),
            used_mask: 0,
            used_machines: [false; 4],
        }
    }
    #[cfg(target_os="xous")]
    pub fn new() -> Self {
        // Note: this requires a memory region window to be manually specified in create-image
        // so that the loader maps the pages for the PIO block. This is because the PIO block is
        // an IP block that is created *outside* of the normal LiteX ecosystem. Specifically look in
        // xtask/src/builder.rs for a "--extra-svd" argument that refers to precursors/pio.svd.
        let csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(rp_pio::HW_RP_PIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        PioSharedState {
            pio: CSR::new(csr.as_mut_ptr() as *mut u32),
            used_mask: 0,
            used_machines: [false; 4],
        }
    }
    pub fn alloc_sm(&mut self) -> Result<PioSm, PioError> {
        if let Some(sm_index) = self.used_machines.iter().position(|&used| used == false) {
            self.used_machines[sm_index] = true;
            let sm = match sm_index {
                0 => SmBit::Sm0,
                1 => SmBit::Sm1,
                2 => SmBit::Sm2,
                3 => SmBit::Sm3,
                _ => return Err(PioError::InvalidSm),
            };
            Ok(PioSm {
                // safety: this routine checks the allocations of the various state machines and
                // helps to ensure we don't overlap regions. However, there is always the possibility
                // that the global shared-state handle is used to smash into a machine's private
                // state. Just don't.
                pio: unsafe{CSR::new(self.pio.base() as *mut u32)},
                sm,
                config: SmConfig::default(),
            })
        } else {
            return Err(PioError::NoFreeMachines)
        }
    }
    /// Safety: it's up to the caller to make sure the SM index is not used. This function
    /// will happily double-allocate an SM if it is already used.
    ///
    /// There are situations where this can be useful for test & validation routines where
    /// we really actually want to allocate a particular SM to force edge cases, but this routine
    /// probably should not be used in any normal user facing code.
    pub unsafe fn force_alloc_sm(&mut self, sm: usize) -> Result<PioSm, PioError> {
        let sm_bit = match sm {
            0 => SmBit::Sm0,
            1 => SmBit::Sm1,
            2 => SmBit::Sm2,
            3 => SmBit::Sm3,
            _ => return Err(PioError::InvalidSm),
        };
        self.used_machines[sm] = true;

        #[cfg(target_os="xous")]
        let csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(rp_pio::HW_RP_PIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();
        #[cfg(target_os="xous")]
        let pio = CSR::new(csr.as_mut_ptr() as *mut u32);
        #[cfg(all(not(target_os="xous"),not(feature="rp2040")))]
        let pio = CSR::new(rp_pio::HW_RP_PIO_BASE as *mut u32);
        #[cfg(all(not(target_os="xous"),feature="rp2040"))]
        let pio = CSR::new(0x5020_0000 as *mut u32);

        Ok(PioSm {
            pio,
            sm: sm_bit,
            config: SmConfig::default(),
        })
    }
    /// Safety: the user must make sure that the SM in question is no longer running.
    #[allow(dead_code)]
    pub unsafe fn free_sm(&mut self, sm: PioSm) {
        self.used_machines[sm.sm_index()] = false;
    }

    fn find_offset_for_program(&self, program: &Program<RP2040_MAX_PROGRAM_SIZE>) -> Option<usize> {
        let prog_mask = (1 << program.code.len() as u32) - 1;
        if let Some(origin) = program.origin {
            if origin as usize > RP2040_MAX_PROGRAM_SIZE - program.code.len() {
                None
            } else {
                if (self.used_mask & (prog_mask << origin as u32)) != 0 {
                    None
                } else {
                    Some(origin as usize)
                }
            }
        } else {
            for i in (0..=(32 - program.code.len())).rev() {
                if (self.used_mask & (prog_mask << i)) == 0 {
                    return Some(i)
                }
            }
            None
        }
    }
    pub fn can_add_program(&self, program: &Program<RP2040_MAX_PROGRAM_SIZE>) -> bool {
        self.find_offset_for_program(program).is_some()
    }
    /// Write an instruction to program memory.
    fn write_progmem(&mut self, offset: usize, data: u16) {
        assert!(offset < 32);
        unsafe {
            self.pio.base().add(offset + rp_pio::SFR_INSTR_MEM0.offset()).write_volatile(data as _);
        }
    }
    /// returns the offset of the program once loaded
    pub fn add_program(
        &mut self,
        program: &Program<RP2040_MAX_PROGRAM_SIZE>,
    ) -> Result<usize, PioError> {
        if self.can_add_program(&program) {
            if let Some(origin) = self.find_offset_for_program(&program) {
                for (i, &instr) in program.code.iter().enumerate() {
                    // I feel like if I were somehow more clever I could find somewhere in one of these
                    // libraries a macro that defines the jump instruction coding. But I can't. So,
                    // this function literally just masks off the opcode (top 3 bits) and checks if
                    // it's a jump instrution (3b000).
                    let located_instr = if instr & 0xE000 != 0x0000 {
                        instr
                    } else {
                        // this works because the offset is the LSB, and, generally the code is
                        // assembled to address 0. Gross, but that's how the API is defined.
                        instr + origin as u16
                    };
                    self.write_progmem(origin + i, located_instr);
                }
                let prog_mask = (1 << program.code.len()) - 1;
                self.used_mask |= prog_mask << origin as u32;
                Ok(origin as usize)
            } else {
                Err(PioError::Oom)
            }
        } else {
            Err(PioError::Oom)
        }
    }
    /// This merely de-allocates the space but it does not actually change the contents.
    #[allow(dead_code)]
    pub fn remove_program(
        &mut self,
        program: &Program<RP2040_MAX_PROGRAM_SIZE>,
        loaded_offset: usize,
    ) {
        let prog_mask = (((1 << program.code.len()) - 1) << loaded_offset) as u32;
        self.used_mask &= !prog_mask;
    }
    /// Clears all allocations and fills program memory with a set of instructions
    /// that jump to themselves (this mirrors the pattern in the Pi SDK)
    #[allow(dead_code)]
    pub fn clear_instruction_memory(
        &mut self,
    ) {
        self.used_mask = 0;

        // jump is instruction 0; so a jump to yourself is simply your address
        for i in 0..RP2040_MAX_PROGRAM_SIZE {
            self.write_progmem(i, i as u16);
        }
    }
}
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum SmBit {
    Sm0 = 1,
    Sm1 = 2,
    Sm2 = 4,
    Sm3 = 8
}
/// Allocate a PioSm using PioSharedState. There is no way to create this object
/// without going through that gate-keeper object.
#[derive(Debug)]
pub struct PioSm {
    pub pio: CSR<u32>,
    sm: SmBit,
    config: SmConfig,
}
impl PioSm {
    #[allow(dead_code)]
    pub fn dbg_get_shiftctl(&self) -> u32 {
        self.config.shiftctl
    }
    pub fn sm_index(&self) -> usize {
        match self.sm {
            SmBit::Sm0 => 0,
            SmBit::Sm1 => 1,
            SmBit::Sm2 => 2,
            SmBit::Sm3 => 3,
        }
    }
    pub fn sm_bitmask(&self) -> u32 {
        self.sm as u32
    }
    pub fn sm_txfifo_is_full(&self) -> bool {
        (self.pio.rf(rp_pio::SFR_FSTAT_TX_FULL) & (self.sm_bitmask())) != 0
    }
    pub fn sm_txfifo_is_empty(&self) -> bool {
        (self.pio.rf(rp_pio::SFR_FSTAT_TX_EMPTY) & (self.sm_bitmask())) != 0
    }
    pub fn sm_txfifo_level(&self) -> usize {
        match self.sm {
            SmBit::Sm0 => self.pio.rf(rp_pio::SFR_FLEVEL_TX_LEVEL0) as usize,
            SmBit::Sm1 => self.pio.rf(rp_pio::SFR_FLEVEL_TX_LEVEL1) as usize,
            SmBit::Sm2 => self.pio.rf(rp_pio::SFR_FLEVEL_TX_LEVEL2) as usize,
            SmBit::Sm3 => self.pio.rf(rp_pio::SFR_FLEVEL_TX_LEVEL3) as usize,
        }
    }
    pub fn sm_txfifo_push_u32(&mut self, data: u32) {
        match self.sm {
            SmBit::Sm0 => self.pio.wo(rp_pio::SFR_TXF0, data),
            SmBit::Sm1 => self.pio.wo(rp_pio::SFR_TXF1, data),
            SmBit::Sm2 => self.pio.wo(rp_pio::SFR_TXF2, data),
            SmBit::Sm3 => self.pio.wo(rp_pio::SFR_TXF3, data),
        }
    }
    pub fn sm_txfifo_push_u16_msb(&mut self, data: u16) {
        match self.sm {
            SmBit::Sm0 => self.pio.wo(rp_pio::SFR_TXF0, (data as u32) << 16),
            SmBit::Sm1 => self.pio.wo(rp_pio::SFR_TXF1, (data as u32) << 16),
            SmBit::Sm2 => self.pio.wo(rp_pio::SFR_TXF2, (data as u32) << 16),
            SmBit::Sm3 => self.pio.wo(rp_pio::SFR_TXF3, (data as u32) << 16),
        }
    }
    #[allow(dead_code)]
    pub fn sm_txfifo_push_u16_lsb(&mut self, data: u16) {
        match self.sm {
            SmBit::Sm0 => self.pio.wo(rp_pio::SFR_TXF0, data as u32),
            SmBit::Sm1 => self.pio.wo(rp_pio::SFR_TXF1, data as u32),
            SmBit::Sm2 => self.pio.wo(rp_pio::SFR_TXF2, data as u32),
            SmBit::Sm3 => self.pio.wo(rp_pio::SFR_TXF3, data as u32),
        }
    }
    pub fn sm_txfifo_push_u8_msb(&mut self, data: u8) {
        match self.sm {
            SmBit::Sm0 => self.pio.wo(rp_pio::SFR_TXF0, (data as u32) << 24),
            SmBit::Sm1 => self.pio.wo(rp_pio::SFR_TXF1, (data as u32) << 24),
            SmBit::Sm2 => self.pio.wo(rp_pio::SFR_TXF2, (data as u32) << 24),
            SmBit::Sm3 => self.pio.wo(rp_pio::SFR_TXF3, (data as u32) << 24),
        }
    }
    pub fn sm_rxfifo_is_empty(&self) -> bool {
        (self.pio.rf(rp_pio::SFR_FSTAT_RX_EMPTY) & (self.sm_bitmask())) != 0
    }
    pub fn sm_rxfifo_is_full(&self) -> bool {
        (self.pio.rf(rp_pio::SFR_FSTAT_RX_FULL) & (self.sm_bitmask())) != 0
    }
    pub fn sm_rxfifo_level(&self) -> usize {
        match self.sm {
            SmBit::Sm0 => self.pio.rf(rp_pio::SFR_FLEVEL_RX_LEVEL0) as usize,
            SmBit::Sm1 => self.pio.rf(rp_pio::SFR_FLEVEL_RX_LEVEL1) as usize,
            SmBit::Sm2 => self.pio.rf(rp_pio::SFR_FLEVEL_RX_LEVEL2) as usize,
            SmBit::Sm3 => self.pio.rf(rp_pio::SFR_FLEVEL_RX_LEVEL3) as usize,
        }
    }
    #[allow(dead_code)]
    pub fn sm_rxfifo_pull_u32(&mut self) -> u32 {
        match self.sm {
            SmBit::Sm0 => self.pio.r(rp_pio::SFR_RXF0),
            SmBit::Sm1 => self.pio.r(rp_pio::SFR_RXF1),
            SmBit::Sm2 => self.pio.r(rp_pio::SFR_RXF2),
            SmBit::Sm3 => self.pio.r(rp_pio::SFR_RXF3),
        }
    }
    pub fn sm_rxfifo_pull_u8_lsb(&mut self) -> u8 {
        match self.sm {
            SmBit::Sm0 => self.pio.r(rp_pio::SFR_RXF0) as u8,
            SmBit::Sm1 => self.pio.r(rp_pio::SFR_RXF1) as u8,
            SmBit::Sm2 => self.pio.r(rp_pio::SFR_RXF2) as u8,
            SmBit::Sm3 => self.pio.r(rp_pio::SFR_RXF3) as u8,
        }
    }
    pub fn sm_put_blocking(&mut self, data: u32) {
        while self.sm_txfifo_is_full() {
            // idle
        }
        self.sm_txfifo_push_u32(data);
    }
    pub fn sm_get_blocking(&mut self) -> u32 {
        while self.sm_rxfifo_is_empty() {
            // idle
        }
        self.sm_rxfifo_pull_u32()
    }
    pub fn sm_set_tx_fifo_margin(&mut self, margin: usize) {
        let checked_margin = if margin > 3 {3u32} else {margin as u32};
        match self.sm {
            SmBit::Sm0 => self.pio.rmwf(rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN0, checked_margin),
            SmBit::Sm1 => self.pio.rmwf(rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN1, checked_margin),
            SmBit::Sm2 => self.pio.rmwf(rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN2, checked_margin),
            SmBit::Sm3 => self.pio.rmwf(rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN3, checked_margin),
        }
    }
    pub fn sm_get_tx_fifo_margin(&mut self) -> u32 {
        match self.sm {
            SmBit::Sm0 => self.pio.rf(rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN0),
            SmBit::Sm1 => self.pio.rf(rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN1),
            SmBit::Sm2 => self.pio.rf(rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN2),
            SmBit::Sm3 => self.pio.rf(rp_pio::SFR_FIFO_MARGIN_FIFO_TX_MARGIN3),
        }
    }
    pub fn sm_set_rx_fifo_margin(&mut self, margin: usize) {
        let checked_margin = if margin > 3 {3u32} else {margin as u32};
        match self.sm {
            SmBit::Sm0 => self.pio.rmwf(rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN0, checked_margin),
            SmBit::Sm1 => self.pio.rmwf(rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN1, checked_margin),
            SmBit::Sm2 => self.pio.rmwf(rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN2, checked_margin),
            SmBit::Sm3 => self.pio.rmwf(rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN3, checked_margin),
        }
    }
    pub fn sm_get_rx_fifo_margin(&mut self) -> u32 {
        match self.sm {
            SmBit::Sm0 => self.pio.rf(rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN0),
            SmBit::Sm1 => self.pio.rf(rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN1),
            SmBit::Sm2 => self.pio.rf(rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN2),
            SmBit::Sm3 => self.pio.rf(rp_pio::SFR_FIFO_MARGIN_FIFO_RX_MARGIN3),
        }
    }
    pub fn sm_to_stride_offset(&self) -> usize {
        // derive the constant value of the stride between SMs
        const STRIDE: usize = rp_pio::SFR_SM1_EXECCTRL.offset() - rp_pio::SFR_SM0_EXECCTRL.offset();
        match self.sm {
            SmBit::Sm0 => STRIDE * 0,
            SmBit::Sm1 => STRIDE * 1,
            SmBit::Sm2 => STRIDE * 2,
            SmBit::Sm3 => STRIDE * 3,
        }
    }
    pub fn sm_address(&self) -> usize {
        match self.sm {
            SmBit::Sm0 => self.pio.rf(rp_pio::SFR_SM0_ADDR_PC) as usize,
            SmBit::Sm1 => self.pio.rf(rp_pio::SFR_SM1_ADDR_PC) as usize,
            SmBit::Sm2 => self.pio.rf(rp_pio::SFR_SM2_ADDR_PC) as usize,
            SmBit::Sm3 => self.pio.rf(rp_pio::SFR_SM3_ADDR_PC) as usize,
        }
    }
    pub fn config_set_out_pins(&mut self, out_base: usize, out_count: usize) {
        assert!(out_base < 32);
        assert!(out_count <= 32);
        // note a feature of UTRA is that for multi-bank operations, you can
        // refer to the base bank (SM0) and add an offset to it. All the SMn
        // field macros (.zf(), .ms()) are identical, so we can just use the SM0 macro
        // without type conflict or error.
        self.config.pinctl =
            // zero the PINS_OUT_COUNT field...
            self.pio.zf(rp_pio::SFR_SM0_PINCTRL_PINS_OUT_COUNT,
                // ... and zero the PINS_OUT_BASE field ...
                self.pio.zf(rp_pio::SFR_SM0_PINCTRL_PINS_OUT_BASE,
                    // ... from the existing value of PINCTL
                    self.config.pinctl
                )
            )
            // OR with the new values of the fields, masked and shifted
            | self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_OUT_BASE, out_base as _)
            | self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_OUT_COUNT, out_count as _);
    }
    #[allow(dead_code)]
    pub fn config_set_set_pins(&mut self, set_base: usize, set_count: usize) {
        assert!(set_base < 32);
        assert!(set_count <= 5);
        self.config.pinctl =
            self.pio.zf(rp_pio::SFR_SM0_PINCTRL_PINS_SET_COUNT,
                self.pio.zf(rp_pio::SFR_SM0_PINCTRL_PINS_SET_BASE,
                    self.config.pinctl
                )
            )
            | self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_SET_BASE, set_base as _)
            | self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_SET_COUNT, set_count as _);
    }
    pub fn config_set_in_pins(&mut self, in_base: usize) {
        assert!(in_base < 32);
        self.config.pinctl =
                self.pio.zf(rp_pio::SFR_SM0_PINCTRL_PINS_IN_BASE,
                    self.config.pinctl
                )
                | self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_IN_BASE, in_base as _);
    }
    pub fn config_set_sideset_pins(&mut self, sideset_base: usize) {
        assert!(sideset_base < 32);
        self.config.pinctl =
            self.pio.zf(rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_BASE,
                self.config.pinctl
            )
            | self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_BASE, sideset_base as _);
    }
    #[allow(dead_code)]
    pub fn config_set_sideset(&mut self, bit_count: usize, optional: bool, pindirs: bool) {
        assert!(bit_count <= 5);
        assert!(!optional || bit_count >= 1);
        self.config.pinctl =
            self.pio.zf(rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_COUNT,
                self.config.pinctl
            )
            | self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_SIDE_COUNT, bit_count as _);

        self.config.execctl =
            self.pio.zf(rp_pio::SFR_SM0_EXECCTRL_SIDE_PINDIR,
                self.pio.zf(rp_pio::SFR_SM0_EXECCTRL_SIDESET_ENABLE_BIT,
                    self.config.execctl
                )
            )
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_SIDESET_ENABLE_BIT, if optional {1} else {0})
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_SIDE_PINDIR, if pindirs {1} else {0});
    }
    pub fn config_set_out_shift(&mut self, shift_right: bool, autopull: bool, pull_threshold: usize) {
        assert!(pull_threshold <= 32);
        self.config.shiftctl =
            self.pio.zf(rp_pio::SFR_SM0_SHIFTCTRL_OSR_THRESHOLD,
                self.pio.zf(rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PULL,
                    self.pio.zf(rp_pio::SFR_SM0_SHIFTCTRL_OUT_SHIFT_DIR,
                        self.config.shiftctl
                    )
                )
            )
            | self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_OSR_THRESHOLD, if pull_threshold == 32 {0} else {pull_threshold as _})
            | self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PULL, if autopull {1} else {0})
            | self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_OUT_SHIFT_DIR, if shift_right {1} else {0});
    }
    pub fn config_set_in_shift(&mut self, shift_right: bool, autopush: bool, push_threshold: usize) {
        assert!(push_threshold <= 32);
        self.config.shiftctl =
            self.pio.zf(rp_pio::SFR_SM0_SHIFTCTRL_ISR_THRESHOLD,
                self.pio.zf(rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PUSH,
                    self.pio.zf(rp_pio::SFR_SM0_SHIFTCTRL_IN_SHIFT_DIR,
                        self.config.shiftctl
                    )
                )
            )
            | self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_ISR_THRESHOLD, if push_threshold == 32 {0} else {push_threshold as _})
            | self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PUSH, if autopush {1} else {0})
            | self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_IN_SHIFT_DIR, if shift_right {1} else {0});
    }
    pub fn config_set_defaults(&mut self) {
        self.config = SmConfig::default();
    }
    pub fn config_set_jmp_pin(&mut self, pin: usize) {
        assert!(pin < 32);
        self.config.execctl =
            self.pio.zf(
                rp_pio::SFR_SM0_EXECCTRL_JMP_PIN,
                self.config.execctl
            )
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_JMP_PIN, pin as _);
    }

    /// returns tuple as (int, frac)
    pub fn clkdiv_from_float(&self, div: f32) -> (u16, u8) {
        assert!(div >= 1.0);
        assert!(div <= 65536.0);
        let div_int = div as u16;
        let div_frac = if div_int == 0 {
            0u8
        } else {
            ((div - div_int as f32) * (1 << 8) as f32) as u8
        };
        (div_int, div_frac)
    }
    pub fn config_set_clkdiv_int_frac(&mut self, div_int: u16, div_frac: u8) {
        assert!(!(div_int == 0 && (div_frac != 0)));
        self.config.clkdiv =
            self.pio.ms(rp_pio::SFR_SM0_CLKDIV_DIV_INT, div_int as _)
            | self.pio.ms(rp_pio::SFR_SM0_CLKDIV_DIV_FRAC, div_frac as _);
    }
    pub fn config_set_clkdiv(&mut self, div: f32) {
        let (div_int, div_frac) = self.clkdiv_from_float(div);
        self.config_set_clkdiv_int_frac(div_int, div_frac);
    }
    pub fn config_set_wrap(&mut self, start: usize, end: usize) {
        self.config.execctl =
            self.pio.zf(rp_pio::SFR_SM0_EXECCTRL_WRAP_TARGET,
                self.pio.zf(rp_pio::SFR_SM0_EXECCTRL_PEND, self.config.execctl)
            )
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_PEND, end as _)
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_WRAP_TARGET, start as _)
            ;
    }
    pub fn config_set_out_special(&mut self, sticky: bool, has_enable: bool, enable_index: usize) {
        self.config.execctl =
            self.pio.zf(
                rp_pio::SFR_SM0_EXECCTRL_OUT_STICKY,
                self.pio.zf(
                    rp_pio::SFR_SM0_EXECCTRL_OUT_EN_SEL,
                    self.pio.zf(
                        rp_pio::SFR_SM0_EXECCTRL_INLINE_OUT_EN,
                        self.config.execctl
                    )
                )
            )
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_OUT_STICKY, if sticky {1} else {0})
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_INLINE_OUT_EN, if has_enable {1} else {0})
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_OUT_EN_SEL, enable_index as u32)
            ;
    }
    pub fn config_set_fifo_join(&mut self, join: PioFifoJoin) {
        self.config.shiftctl =
            self.pio.zf(
                rp_pio::SFR_SM0_SHIFTCTRL_JOIN_RX,
                self.pio.zf(
                    rp_pio::SFR_SM0_SHIFTCTRL_JOIN_TX,
                    self.config.shiftctl
                )
            )
            |
            match join {
                PioFifoJoin::None => 0,
                PioFifoJoin::JoinTx => self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_JOIN_TX, 1),
                PioFifoJoin::JoinRx => self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_JOIN_RX, 1),
            }
    }
    pub fn config_set_mov_status(&mut self, status_sel: MovStatusType, level: usize) {
        self.config.execctl =
            self.pio.zf(
                rp_pio::SFR_SM0_EXECCTRL_STATUS_SEL,
                self.pio.zf(rp_pio::SFR_SM0_EXECCTRL_STATUS_N,
                    self.config.execctl
                )
            )
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_STATUS_SEL, status_sel as u32)
            | self.pio.ms(rp_pio::SFR_SM0_EXECCTRL_STATUS_N, level as u32)
            ;
    }

    pub fn sm_exec(&mut self, instr: u16) {
        let sm_offset = self.sm_to_stride_offset();
        unsafe {
            self.pio.base().add(rp_pio::SFR_SM0_INSTR.offset() + sm_offset)
            .write_volatile(instr as u32);
        }
    }
    pub fn sm_set_pindirs_with_mask(&mut self, pindirs: usize, mut pin_mask: usize) {
        let sm_offset = self.sm_to_stride_offset();
        unsafe {
            let pinctrl_saved = self.pio.base().add(rp_pio::SFR_SM0_PINCTRL.offset() + sm_offset).read_volatile();
            let exectrl_saved = self.pio.base().add(rp_pio::SFR_SM0_EXECCTRL.offset() + sm_offset).read_volatile();
            self.pio.base().add(rp_pio::SFR_SM0_EXECCTRL.offset() + sm_offset).write_volatile(
                self.pio.zf(
                    rp_pio::SFR_SM0_EXECCTRL_OUT_STICKY,
                    self.pio.base().add(rp_pio::SFR_SM0_EXECCTRL.offset() + sm_offset).read_volatile()
                )
            );
            while pin_mask != 0 {
                let base = pin_mask.trailing_zeros();
                self.pio.base().add(rp_pio::SFR_SM0_PINCTRL.offset() + sm_offset).write_volatile(
                    self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_SET_COUNT, 1)
                    | self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_SET_BASE, base as _)
                );
                let mut a = pio::Assembler::<32>::new();
                a.set(pio::SetDestination::PINDIRS, ((pindirs >> base) & 1) as u8);
                let p= a.assemble_program();
                self.sm_exec(p.code[p.origin.unwrap_or(0) as usize]);
                pin_mask &= pin_mask - 1;
            }
            self.pio.base().add(rp_pio::SFR_SM0_PINCTRL.offset() + sm_offset).write_volatile(pinctrl_saved);
            self.pio.base().add(rp_pio::SFR_SM0_EXECCTRL.offset() + sm_offset).write_volatile(exectrl_saved);
        }
    }
    pub fn sm_set_pins_with_mask(&mut self, pinvals: usize, mut pin_mask: usize) {
        let sm_offset = self.sm_to_stride_offset();
        unsafe {
            let pinctrl_saved = self.pio.base().add(rp_pio::SFR_SM0_PINCTRL.offset() + sm_offset).read_volatile();
            let exectrl_saved = self.pio.base().add(rp_pio::SFR_SM0_EXECCTRL.offset() + sm_offset).read_volatile();
            self.pio.base().add(rp_pio::SFR_SM0_EXECCTRL.offset() + sm_offset).write_volatile(
                self.pio.zf(
                    rp_pio::SFR_SM0_EXECCTRL_OUT_STICKY,
                    self.pio.base().add(rp_pio::SFR_SM0_EXECCTRL.offset() + sm_offset).read_volatile()
                )
            );
            while pin_mask != 0 {
                let base = pin_mask.trailing_zeros();
                self.pio.base().add(rp_pio::SFR_SM0_PINCTRL.offset() + sm_offset).write_volatile(
                    self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_SET_COUNT, 1)
                    | self.pio.ms(rp_pio::SFR_SM0_PINCTRL_PINS_SET_BASE, base as _)
                );
                let mut a = pio::Assembler::<32>::new();
                a.set(pio::SetDestination::PINS, ((pinvals >> base) & 1) as u8);
                let p= a.assemble_program();
                self.sm_exec(p.code[p.origin.unwrap_or(0) as usize]);
                pin_mask &= pin_mask - 1;
            }
            self.pio.base().add(rp_pio::SFR_SM0_PINCTRL.offset() + sm_offset).write_volatile(pinctrl_saved);
            self.pio.base().add(rp_pio::SFR_SM0_EXECCTRL.offset() + sm_offset).write_volatile(exectrl_saved);
        }
    }
    #[allow(dead_code)]
    pub fn global_irq0_source_enabled(&mut self, source: PioRawIntSource, enabled: bool) {
        self.pio.wo(rp_pio::SFR_IRQ0_INTE,
            if enabled {source as u32} else {0}
            | self.pio.r(rp_pio::SFR_IRQ0_INTE) & !(source as u32)
        );
    }
    #[allow(dead_code)]
    pub fn global_irq1_source_enabled(&mut self, source: PioRawIntSource, enabled: bool) {
        self.pio.wo(rp_pio::SFR_IRQ1_INTE,
            if enabled {source as u32} else {0}
            | self.pio.r(rp_pio::SFR_IRQ1_INTE) & !(source as u32)
        );
    }
    pub fn sm_irq0_source_enabled(&mut self, source: PioIntSource, enabled: bool) {
        let mask = match source {
            PioIntSource::Sm => (self.sm_bitmask()) << 8,
            PioIntSource::TxNotFull => (self.sm_bitmask()) << 4,
            PioIntSource::RxNotEmpty => (self.sm_bitmask()) << 0,
        };
        self.pio.wo(rp_pio::SFR_IRQ0_INTE,
            if enabled {mask} else {0}
            | self.pio.r(rp_pio::SFR_IRQ0_INTE) & !mask
        );
    }
    pub fn sm_irq1_source_enabled(&mut self, source: PioIntSource, enabled: bool) {
        let mask = match source {
            PioIntSource::Sm => (self.sm_bitmask()) << 8,
            PioIntSource::TxNotFull => (self.sm_bitmask()) << 4,
            PioIntSource::RxNotEmpty => (self.sm_bitmask()) << 0,
        };
        self.pio.wo(rp_pio::SFR_IRQ1_INTE,
            if enabled{mask} else {0}
            | self.pio.r(rp_pio::SFR_IRQ1_INTE) & !mask
        );
    }
    #[allow(dead_code)]
    pub fn sm_irq0_status(&mut self, source: Option<PioIntSource>) -> bool {
        let mask = if let Some(source) = source {
            match source {
                PioIntSource::Sm => (self.sm_bitmask()) << 8,
                PioIntSource::TxNotFull => (self.sm_bitmask()) << 4,
                PioIntSource::RxNotEmpty => (self.sm_bitmask()) << 0,
            }
        } else {
            0xFFF
        };
        (self.pio.r(rp_pio::SFR_IRQ0_INTS) & mask) != 0
    }
    pub fn sm_irq1_status(&mut self, source: Option<PioIntSource>) -> bool {
        let mask = if let Some(source) = source {
            match source {
                PioIntSource::Sm => (self.sm_bitmask()) << 0,
                PioIntSource::TxNotFull => (self.sm_bitmask()) << 4,
                PioIntSource::RxNotEmpty => (self.sm_bitmask()) << 8,
            }
        } else {
            0xFFF
        };
        (self.pio.r(rp_pio::SFR_IRQ1_INTS) & mask) != 0
    }

    pub fn sm_set_enabled(&mut self, enabled: bool) {
        if enabled {
            self.pio.rmwf(rp_pio::SFR_CTRL_EN,
                self.pio.rf(rp_pio::SFR_CTRL_EN) | (self.sm_bitmask())
            )
        } else {
            self.pio.rmwf(rp_pio::SFR_CTRL_EN,
                self.pio.rf(rp_pio::SFR_CTRL_EN) & !(self.sm_bitmask())
            )
        }
    }

    pub fn sm_set_config(&mut self) {
        let sm_offset = self.sm_to_stride_offset();
        unsafe {
            self.pio.base().add(rp_pio::SFR_SM0_CLKDIV.offset() + sm_offset).write_volatile(self.config.clkdiv);
            self.pio.base().add(rp_pio::SFR_SM0_EXECCTRL.offset() + sm_offset).write_volatile(self.config.execctl);
            self.pio.base().add(rp_pio::SFR_SM0_SHIFTCTRL.offset() + sm_offset).write_volatile(self.config.shiftctl);
            self.pio.base().add(rp_pio::SFR_SM0_PINCTRL.offset() + sm_offset).write_volatile(self.config.pinctl);
        }
    }
    /// Clears the FIFOs by flipping the RX join bit
    pub fn sm_clear_fifos(&mut self) {
        let sm_offset = self.sm_to_stride_offset();
        unsafe {
            let baseval = self.pio.base().add(rp_pio::SFR_SM0_SHIFTCTRL.offset() + sm_offset).read_volatile();
            let bitval = self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_JOIN_RX, 1);
            self.pio.base().add(rp_pio::SFR_SM0_SHIFTCTRL.offset() + sm_offset).write_volatile(
                baseval ^ bitval
            );
            self.pio.base().add(rp_pio::SFR_SM0_SHIFTCTRL.offset() + sm_offset).write_volatile(
                baseval
            );
        }
    }
    pub fn sm_init(&mut self, initial_pc: usize) {
        self.sm_set_enabled(false);
        self.sm_set_config();

        self.sm_clear_fifos();

        // Clear FIFO debug flags
        self.pio.wo(
            rp_pio::SFR_FDEBUG,
            self.pio.ms(rp_pio::SFR_FDEBUG_TXSTALL, self.sm_bitmask())
            | self.pio.ms(rp_pio::SFR_FDEBUG_TXOVER, self.sm_bitmask())
            | self.pio.ms(rp_pio::SFR_FDEBUG_RXUNDER, self.sm_bitmask())
            | self.pio.ms(rp_pio::SFR_FDEBUG_RXSTALL, self.sm_bitmask())
        );

        // Finally, clear some internal SM state
        // these *must* be combined together, because the CPU runs much faster than the
        // state machines -- if you write clkdiv_restart and then restart one after another,
        // one of the commands will likely get lost, because the CPU is changing command
        // state before the PIO block can even recognize it.
        self.pio.wo(
            rp_pio::SFR_CTRL,
            self.pio.rf(rp_pio::SFR_CTRL_EN)
            | self.pio.ms(rp_pio::SFR_CTRL_CLKDIV_RESTART, self.sm_bitmask())
            | self.pio.ms(rp_pio::SFR_CTRL_RESTART, self.sm_bitmask())
        );
        while (self.pio.r(rp_pio::SFR_CTRL) & !0xF) != 0 {
            // wait for the bits to self-reset to acknowledge that the clears have executed
        }

        let mut a = pio::Assembler::<32>::new();
        let mut initial_label = a.label_at_offset(initial_pc as u8);
        a.jmp(pio::JmpCondition::Always, &mut initial_label);
        let p= a.assemble_program();

        self.sm_exec(p.code[p.origin.unwrap_or(0) as usize]);
    }
    pub fn sm_interrupt_get(&self, int_number: usize) -> bool {
        assert!(int_number < 8);
        (self.pio.r(rp_pio::SFR_IRQ) & (1 << int_number)) != 0
    }
    pub fn sm_drain_tx_fifo(&mut self) {
        let sm_offset = self.sm_to_stride_offset();
        let instr = {
            if (unsafe { self.pio.base().add(rp_pio::SFR_SM0_SHIFTCTRL.offset() + sm_offset).read_volatile() }
            & self.pio.ms(rp_pio::SFR_SM0_SHIFTCTRL_AUTO_PULL, 1)) != 0 {
                // autopull is true
                let mut a = pio::Assembler::<32>::new();
                a.out(pio::OutDestination::NULL, 32);
                let p= a.assemble_program();
                p.code[0]
            } else {
                // autopull is false
                let mut a = pio::Assembler::<32>::new();
                a.pull(false, false);
                let p= a.assemble_program();
                p.code[0]
            }
        };
        while !self.sm_txfifo_is_empty() {
            self.sm_exec(instr);
        }
    }
    /// Changes the PC to the lowest address of the wrap range.
    /// Also restarts the relevant state machine.
    pub fn sm_jump_to_wrap_bottom(&mut self) {
        // disable the machine
        // and restart it
        self.pio.wo(
            rp_pio::SFR_CTRL,
            (self.pio.rf(rp_pio::SFR_CTRL_EN) // this is aligned to 0 so we're skipping the alignment
              & !self.pio.ms(rp_pio::SFR_CTRL_EN, self.sm_bitmask())
            )
            | self.pio.ms(rp_pio::SFR_CTRL_RESTART, self.sm_bitmask())
        );
        while (self.pio.r(rp_pio::SFR_CTRL) & !0xF) != 0 {
            // wait for the bits to self-reset to acknowledge that the clears have executed
        }

        // HACK: a jump instruction is just the address of the location you want to run
        // so we can just extract the wrap target and "use that as an instruction".
        let instr = match self.sm {
            SmBit::Sm0 => self.pio.rf(rp_pio::SFR_SM0_EXECCTRL_WRAP_TARGET),
            SmBit::Sm1 => self.pio.rf(rp_pio::SFR_SM1_EXECCTRL_WRAP_TARGET),
            SmBit::Sm2 => self.pio.rf(rp_pio::SFR_SM2_EXECCTRL_WRAP_TARGET),
            SmBit::Sm3 => self.pio.rf(rp_pio::SFR_SM3_EXECCTRL_WRAP_TARGET),
        } as u16;
        self.sm_exec(instr);
        // re-enable the machine
        self.pio.wo(
            rp_pio::SFR_CTRL,
            self.pio.r(rp_pio::SFR_CTRL)
            | self.pio.ms(rp_pio::SFR_CTRL_EN, self.sm_bitmask())
        );
    }

    pub fn gpio_reset_overrides(&mut self) {
        self.pio.wo(rp_pio::SFR_IO_O_INV, 0);
        self.pio.wo(rp_pio::SFR_IO_OE_INV, 0);
        self.pio.wo(rp_pio::SFR_IO_I_INV, 0);
    }
    pub fn gpio_set_outover(&mut self, pin: usize, value: bool) {
        self.pio.wo(rp_pio::SFR_IO_O_INV,
            (if value {1} else {0}) << pin
            | (self.pio.r(rp_pio::SFR_IO_O_INV) & !(1 << pin))
        );
    }
    #[allow(dead_code)]
    pub fn gpio_set_oeover(&mut self, pin: usize, value: bool) {
        self.pio.wo(rp_pio::SFR_IO_OE_INV,
            (if value {1} else {0}) << pin
            | (self.pio.r(rp_pio::SFR_IO_OE_INV) & !(1 << pin))
        );
    }
    #[allow(dead_code)]
    pub fn gpio_set_inover(&mut self, pin: usize, value: bool) {
        self.pio.wo(rp_pio::SFR_IO_I_INV,
            (if value {1} else {0}) << pin
            | (self.pio.r(rp_pio::SFR_IO_I_INV) & !(1 << pin))
        );
    }
}

/// used to generate some test vectors
pub fn lfsr_next(state: u16) -> u16 {
    let bit = ((state >> 8) ^
               (state >>  4)) & 1;

    ((state << 1) + bit) & 0x1_FF
}