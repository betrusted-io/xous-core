use arbitrary_int::Number;
#[cfg(feature = "std")]
use bao1x_api::IoxHal;
use bao1x_api::{bio::*, bio_code};
use utra::bio_bdma;
use utralib::{
    utra::bio_bdma::{SFR_ELEVEL_FIFO_EVENT_LEVEL0, SFR_IRQMASK_0},
    *,
};

#[cfg(not(feature = "std"))]
use crate::iox::Iox;

pub struct BioSharedState {
    #[cfg(not(feature = "std"))]
    pub bio: CSR<u32>,
    #[cfg(feature = "std")]
    pub bio: AtomicCsr<u32>,
    pub imem_slice: [&'static mut [u32]; 4],
    pub core_config: [Option<CoreConfig>; 4],
    // handles are allocated in the process space of the caller, but allocation is tracked here
    pub handle_used: [bool; 4],
    pub fclk_freq_hz: u32,
    #[cfg(not(feature = "std"))]
    pub iox: Iox,
    #[cfg(feature = "std")]
    pub iox: IoxHal,
}

impl BioSharedState {
    #[cfg(not(feature = "std"))]
    pub fn new(fclk_freq: u32) -> Self {
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

        let iox = Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
        let mut bio_ss = BioSharedState {
            bio: CSR::new(utra::bio_bdma::HW_BIO_BDMA_BASE as *mut u32),
            imem_slice,
            core_config: [None; 4],
            handle_used: [false; 4],
            fclk_freq_hz: fclk_freq,
            iox,
        };
        bio_ss.init();
        bio_ss
    }

    #[cfg(feature = "std")]
    pub fn new(fclk_freq: u32) -> Self {
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

        let iox = bao1x_api::IoxHal::new();

        let mut bio_ss = BioSharedState {
            bio: AtomicCsr::new(csr.as_mut_ptr() as *mut u32),
            // safety: MemoryRange does not de-allocate the mapping on Drop. So the maps live the
            // lifetime of the process (or until `unmap_memory` is called: please don't do that).
            // Since the range maps to some underlying hardware and it's aligned for the data types,
            // it's as safe as any naked slice can be (i.e. you still have to think about concurrency etc.).
            imem_slice: unsafe {
                [
                    core::slice::from_raw_parts_mut(
                        imem0.as_mut_ptr() as *mut u32,
                        utralib::HW_BIO_IMEM0_MEM_LEN / size_of::<u32>(),
                    ),
                    core::slice::from_raw_parts_mut(
                        imem1.as_mut_ptr() as *mut u32,
                        utralib::HW_BIO_IMEM1_MEM_LEN / size_of::<u32>(),
                    ),
                    core::slice::from_raw_parts_mut(
                        imem2.as_mut_ptr() as *mut u32,
                        utralib::HW_BIO_IMEM2_MEM_LEN / size_of::<u32>(),
                    ),
                    core::slice::from_raw_parts_mut(
                        imem3.as_mut_ptr() as *mut u32,
                        utralib::HW_BIO_IMEM3_MEM_LEN / size_of::<u32>(),
                    ),
                ]
            },
            core_config: [None; 4],
            handle_used: [false; 4],
            fclk_freq_hz: fclk_freq,
            iox,
        };
        bio_ss.init();

        bio_ss
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

        for core in 0..4 {
            // crate::println!("ldst trial");
            self.load_code(mem_init_code(), 0, BioCore::from(core));
            self.set_core_run_states([core == 0, core == 1, core == 2, core == 3]);
            for _ in 0..16 {
                let _ = self.bio.r(utra::bio_bdma::SFR_RXF0);
                let _ = self.bio.r(utra::bio_bdma::SFR_RXF1);
                let _ = self.bio.r(utra::bio_bdma::SFR_RXF2);
                let _ = self.bio.r(utra::bio_bdma::SFR_RXF3);
            }
            // crate::println!("ldst trial end");
        }
        self.bio.wfo(utra::bio_bdma::SFR_FIFO_CLR_SFR_FIFO_CLR, 0xf);
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

    /// Given a target frequency, computes dividers that get as close to it as we can.
    /// The return tuple is:
    /// (fdiv_int, fdiv_frac, actual_freq_hz)
    pub(crate) fn compute_dividers(&self, target_freq: u32, allow_frac: bool) -> (u16, u8, u32) {
        // If target frequency is at or above fclk, return bypass mode (no division)
        if target_freq >= self.fclk_freq_hz {
            return (0, 0, self.fclk_freq_hz);
        }

        if allow_frac {
            // Calculate divisor in fixed point (8 fractional bits)
            // divisor = (fclk * 256) / target_freq
            let total_div = ((self.fclk_freq_hz as u64) * 256) / (target_freq as u64);

            // Maximum divisor: 65535 + 255/256 = 16776960 in fixed point
            let max_div = 65535u64 * 256 + 255;

            if total_div > max_div {
                // Frequency too low, clamp to maximum dividers
                let actual_freq = self.compute_freq(65535, 255);
                return (65535, 255, actual_freq);
            }

            // Try both floor and ceil to minimize error
            let div1 = total_div;
            let div2 = (total_div + 1).min(max_div);

            let (di1, df1) = ((div1 / 256) as u16, (div1 % 256) as u8);
            let (di2, df2) = ((div2 / 256) as u16, (div2 % 256) as u8);

            let freq1 = self.compute_freq(di1, df1);
            let freq2 = self.compute_freq(di2, df2);

            let error1 = freq1.abs_diff(target_freq);
            let error2 = freq2.abs_diff(target_freq);

            if error1 <= error2 { (di1, df1, freq1) } else { (di2, df2, freq2) }
        } else {
            // Integer division only
            let div_ideal = ((self.fclk_freq_hz as u64) / (target_freq as u64)).max(1);

            if div_ideal > 65535 {
                // Frequency too low, clamp to maximum divider
                let actual_freq = self.compute_freq(65535, 0);
                return (65535, 0, actual_freq);
            }

            // Try both floor and ceil to minimize error
            let div1 = div_ideal as u16;
            let div2 = (div_ideal + 1).min(65535) as u16;

            let freq1 = self.compute_freq(div1, 0);
            let freq2 = self.compute_freq(div2, 0);

            let error1 = freq1.abs_diff(target_freq);
            let error2 = freq2.abs_diff(target_freq);

            if error1 <= error2 { (div1, 0, freq1) } else { (div2, 0, freq2) }
        }
    }

    pub(crate) fn compute_freq(&self, div_int: u16, div_frac: u8) -> u32 {
        // Compute divisor: div_int + div_frac/256 = (256*div_int + div_frac)/256
        let divisor = (div_int as u32) * 256 + (div_frac as u32);

        // Handle division by zero
        if divisor == 0 {
            return self.fclk_freq_hz;
        }

        // Calculate: fclk / ((256*div_int + div_frac)/256) = (fclk * 256) / divisor
        // Use u64 to avoid overflow since fclk can be up to 700M
        let numerator = (self.fclk_freq_hz as u64) * 256;

        // Perform division with rounding to nearest
        let result = (numerator + (divisor as u64) / 2) / (divisor as u64);

        result as u32
    }

    pub(crate) fn apply_config(&mut self, config: &CoreConfig, core: BioCore) -> Option<u32> {
        let (div_int, div_frac, actual_freq) = match config.clock_mode {
            ClockMode::FixedDivider(div_int, div_frac) => {
                (div_int, div_frac, self.compute_freq(div_int, div_frac))
            }
            ClockMode::TargetFreqFrac(target) => self.compute_dividers(target, true),
            ClockMode::TargetFreqInt(target) => self.compute_dividers(target, false),
            ClockMode::ExternalPin(_) => {
                // disable divisor
                match core {
                    BioCore::Core0 => self.bio.wo(bio_bdma::SFR_QDIV0, 0),
                    BioCore::Core1 => self.bio.wo(bio_bdma::SFR_QDIV1, 0),
                    BioCore::Core2 => self.bio.wo(bio_bdma::SFR_QDIV2, 0),
                    BioCore::Core3 => self.bio.wo(bio_bdma::SFR_QDIV3, 0),
                };
                return None;
            }
        };
        let sfr_value = (div_int as u32) << 16 | (div_frac as u32) << 8;
        match core {
            BioCore::Core0 => self.bio.wo(bio_bdma::SFR_QDIV0, sfr_value),
            BioCore::Core1 => self.bio.wo(bio_bdma::SFR_QDIV1, sfr_value),
            BioCore::Core2 => self.bio.wo(bio_bdma::SFR_QDIV2, sfr_value),
            BioCore::Core3 => self.bio.wo(bio_bdma::SFR_QDIV3, sfr_value),
        };
        Some(actual_freq)
    }
}

impl<'a> BioApi<'a> for BioSharedState {
    fn init_core(
        &mut self,
        core: BioCore,
        code: &[u8],
        offset: usize,
        config: CoreConfig,
    ) -> Result<Option<u32>, BioError> {
        if self.core_config[core as usize].is_some() {
            return Err(BioError::ResourceInUse);
        }
        self.core_config[core as usize] = Some(config);
        self.load_code(code, offset, core);
        Ok(self.apply_config(&config, core))
    }

    fn de_init_core(&mut self, core: BioCore) -> Result<(), BioError> {
        let ctrl = self.bio.r(bio_bdma::SFR_CTRL);
        let core_code = 1u32 << core as u32;
        let core_mask = core_code | core_code << 4 | core_code << 8;
        // shut off just the core that is computed by the mask
        self.bio.wo(bio_bdma::SFR_CTRL, ctrl & !core_mask);
        self.core_config[core as usize] = None;
        Ok(())
    }

    fn get_bio_freq(&self) -> u32 { self.fclk_freq_hz }

    fn get_core_freq(&self, core: BioCore) -> Option<u32> {
        if self.core_config[core as usize].is_some() {
            if self.bio.rf(bio_bdma::SFR_EXTCLOCK_USE_EXTCLK) & (1u32 << core as u32) != 0 {
                return None;
            }
            let qdiv = match core {
                BioCore::Core0 => self.bio.r(bio_bdma::SFR_QDIV0),
                BioCore::Core1 => self.bio.r(bio_bdma::SFR_QDIV1),
                BioCore::Core2 => self.bio.r(bio_bdma::SFR_QDIV2),
                BioCore::Core3 => self.bio.r(bio_bdma::SFR_QDIV3),
            };
            Some(self.compute_freq((qdiv >> 16) as u16, ((qdiv >> 8) & 0xFF) as u8))
        } else {
            None
        }
    }

    fn get_version(&self) -> u32 { self.bio.r(bio_bdma::SFR_CFGINFO) }

    fn update_bio_freq(&mut self, freq: u32) -> u32 {
        let prev_freq = self.fclk_freq_hz;
        self.fclk_freq_hz = freq;
        for (i, config) in self.core_config.clone().iter().enumerate() {
            if let Some(config) = config {
                self.apply_config(config, i.into());
            }
        }
        prev_freq
    }

    unsafe fn get_core_handle(&self, _fifo: Fifo) -> Result<Option<CoreHandle>, BioError> {
        unimplemented!("This is managed by the main loop server, not the hardware interface");
    }

    fn set_core_state(&mut self, which: [CoreRunSetting; 4]) -> Result<(), BioError> {
        let mut start_mask = 0;
        let mut stop_mask = 0;
        for (core_index, setting) in which.iter().enumerate() {
            match setting {
                CoreRunSetting::Start => start_mask |= 1u32 << core_index as u32,
                CoreRunSetting::Stop => stop_mask |= 1u32 << core_index as u32,
                CoreRunSetting::Unchanged => (),
            }
        }
        let core_code = start_mask;
        start_mask = start_mask | start_mask << 4 | start_mask << 8;
        stop_mask = stop_mask | stop_mask << 4 | stop_mask << 8;

        let ctrl = self.bio.r(bio_bdma::SFR_CTRL);
        self.bio.wo(bio_bdma::SFR_CTRL, (ctrl & !stop_mask) | start_mask);

        let mut timeout = 0;
        loop {
            let ctrl = self.bio.r(utra::bio_bdma::SFR_CTRL) & 0xFF0;
            if ctrl == 0 {
                break;
            }
            timeout += 1;
            if timeout > 100000 {
                crate::println!("Timeout on set_core_run_states: req {:x} != rbk {:x}", core_code, ctrl);
                break;
            }
        }
        let check = self.bio.r(utra::bio_bdma::SFR_CTRL);
        if check != core_code {
            crate::println!("run-state check failed: {:x}", check);
        }

        Ok(())
    }

    fn setup_io_config(&mut self, config: IoConfig) -> Result<(), BioError> {
        self.bio.wo(bio_bdma::SFR_IO_I_INV, config.i_inv);
        self.bio.wo(bio_bdma::SFR_IO_O_INV, config.o_inv);
        self.bio.wo(bio_bdma::SFR_IO_OE_INV, config.oe_inv);
        self.bio.wo(bio_bdma::SFR_SYNC_BYPASS, config.sync_bypass);

        let mut sfr = self.bio.r(bio_bdma::SFR_CONFIG);
        if let Some(core) = config.snap_inputs {
            let mask = (bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_QUANTUM.mask()
                << bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_QUANTUM.offset())
                | (bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_WHICH.mask()
                    << bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_WHICH.offset());

            sfr = (sfr & !mask as u32)
                | self.bio.ms(bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_QUANTUM, 1)
                | self.bio.ms(bio_bdma::SFR_CONFIG_SNAP_INPUT_TO_WHICH, core as u32);
        }
        if let Some(core) = config.snap_outputs {
            let mask = (bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM.mask()
                << bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM.offset())
                | (bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_WHICH.mask()
                    << bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_WHICH.offset());

            sfr = (sfr & !mask as u32)
                | self.bio.ms(bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_QUANTUM, 1)
                | self.bio.ms(bio_bdma::SFR_CONFIG_SNAP_OUTPUT_TO_WHICH, core as u32);
        }
        self.bio.wo(bio_bdma::SFR_CONFIG, sfr);

        self.iox.set_ports_from_bio_bitmask(config.mapped);
        Ok(())
    }

    fn setup_dma_windows(&mut self, windows: DmaFilterWindows) -> Result<(), BioError> {
        for (i, maybe_window) in windows.windows.iter().enumerate() {
            if let Some(window) = maybe_window {
                // safety: this is safe because the .offset() function is used to fetch the offset
                // from the base of the BIO region, and the indices are aligned in pairs starting
                // from the 0-offset window
                unsafe {
                    self.bio
                        .base()
                        .add(bio_bdma::SFR_FILTER_BASE_0.offset() + i * 2)
                        .write_volatile(window.base as u32);
                    self.bio
                        .base()
                        .add(bio_bdma::SFR_FILTER_BOUNDS_0.offset() + i * 2)
                        .write_volatile(window.bounds.get());
                }
            }
        }
        Ok(())
    }

    fn setup_fifo_event_triggers(&mut self, config: FifoEventConfig) -> Result<(), BioError> {
        let event_offset = config.which.to_usize_checked() * 2 + config.trigger_slot.raw_value() as usize;
        assert!(event_offset <= 7, "Computed event offset is invalid");
        // safety: this is safe because which is bounds checked with to_usize_checked() and the
        // implementation of arbitrary_int that encodes trigger_slot is also bounds checked. The assert
        // above also helps confirm a lack of logic bugs.
        unsafe {
            self.bio
                .base()
                .add(SFR_ELEVEL_FIFO_EVENT_LEVEL0.offset() + event_offset)
                .write_volatile(config.level.level().as_u32());
        }
        let mask = 1u32 << event_offset as u32;
        let mut lt_gt_eq_set = 0u32;
        let mut lt_gt_eq_clear = 0xFFFF_FFFFu32;
        if config.trigger_less_than {
            lt_gt_eq_set |= mask;
        } else {
            lt_gt_eq_clear &= !mask;
        }
        if config.trigger_equal_to {
            lt_gt_eq_set |= mask << 8;
        } else {
            lt_gt_eq_clear &= !(mask << 8);
        }
        if config.trigger_greater_than {
            lt_gt_eq_set |= mask << 16;
        } else {
            lt_gt_eq_clear &= !(mask << 16);
        }
        let mut lt_gt_eq = self.bio.r(bio_bdma::SFR_ETYPE);
        lt_gt_eq &= lt_gt_eq_clear;
        lt_gt_eq |= lt_gt_eq_set;
        self.bio.wo(bio_bdma::SFR_ETYPE, lt_gt_eq);
        Ok(())
    }

    fn setup_irq_config(&mut self, config: IrqConfig) -> Result<(), BioError> {
        let irq_offset = config.which.to_usize_checked();
        assert!(irq_offset <= 3, "Computed IRQ offset is not valid!");
        // safety: this is safe because irq_offset is bounds checked above, and the irqmasks
        // are lined up in the hardware in a fashion that corresponds to the coding of the Irq specifier
        unsafe {
            self.bio.base().add(SFR_IRQMASK_0.offset() + irq_offset).write_volatile(config.mask.raw_value());
        }
        let edge = self.bio.r(bio_bdma::SFR_IRQ_EDGE);
        if config.edge_triggered {
            self.bio.wo(bio_bdma::SFR_IRQ_EDGE, (1 << irq_offset as u32) | edge);
        } else {
            self.bio.wo(bio_bdma::SFR_IRQ_EDGE, !(1 << irq_offset as u32) & edge);
        }
        Ok(())
    }
}

#[rustfmt::skip]
bio_code!(mem_init_code, HAL_MEM_INIT_START, HAL_MEM_INIT_END,
    "sw x0, 0x20(x0)",
    "lw t0, 0x20(x0)",
    "li sp, 0x61200000",
    "addi sp, sp, -4",
    "sw x0, 0(sp)",
    "lw t0, 0(sp)",
  "10:",
    "j 10b"
);
