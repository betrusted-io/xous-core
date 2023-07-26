#![allow(dead_code)]

use utralib::generated::*;
use xous::MemoryRange;
use susres::{RegManager, RegOrField, SuspendResume};
use llio::I2cStatus;
use crate::api::*;
use num_traits::*;
use core::sync::atomic::{AtomicBool, Ordering::SeqCst};

static INTERRUPT_HOOKED: AtomicBool = AtomicBool::new(false);

pub const TLV320AIC3100_I2C_ADR: u8 = 0b0011_000;
const I2C_TIMEOUT: u32 = 50;

pub struct Codec {
    csr: utralib::CSR<u32>,
    fifo: MemoryRange,
    susres_manager: RegManager<{utra::audio::AUDIO_NUMREGS}>,
    llio: llio::Llio,
    i2c: llio::I2c,
    ticktimer: ticktimer_server::Ticktimer,
    play_buffer: FrameRing,
    play_frames_dropped: u32,
    tx_stat_errors: u32,
    rec_buffer: FrameRing,
    rec_frames_dropped: u32,
    rx_stat_errors: u32,
    powered_on: bool,
    initialized: bool,
    live: bool,
    conn: xous::CID,
    drain: bool,
    // to recall values through suspend/resume
    speaker_gain: f32,
    headphone_left_gain: f32,
    headphone_right_gain: f32,
}

static SILENCE: [u32; FIFO_DEPTH] = [ZERO_PCM as u32 | (ZERO_PCM as u32) << 16; FIFO_DEPTH];

/// gain is specified in dB, and has a useful range from 0 to -80dB
fn analog_volume_db_to_code(g: f32) -> u8 {
    if g >= 0.0 {
        0
    } else if g >= -17.5 {
        (-g * 2.0) as u8
    } else if g >= -34.6 {
        (-g * 2.0 - 0.4) as u8
    } else if g >= -49.3 {
        (100.0 - (50.0 + g) * 0.7202) as u8
    } else if g >= -72.2 {
        127
    } else {
        127
    }
}

fn audio_handler(_irq_no: usize, arg: *mut usize) {
    let codec = unsafe { &mut *(arg as *mut Codec) };
    let volatile_audio = codec.fifo.as_mut_ptr() as *mut u32;

    // load the play buffer
    if let Some(frame) = codec.play_buffer.dq_frame() {
        if codec.csr.rf(utra::audio::TX_STAT_FREE) != 1 {
            codec.tx_stat_errors += 1;
        }
        for &stereo_sample in frame.iter() {
            if true {
                //// TODO
                // there is some bug which is causing the right channel to be frame shifted left by one, but not the left....could be a hardware bug.
                unsafe { volatile_audio.write_volatile((stereo_sample & 0xFFFF_0000) | ((stereo_sample & 0xFFFF) >> 1)) };
            } else {
                unsafe { volatile_audio.write_volatile(stereo_sample) };
            }
        }
    } else {
        codec.play_frames_dropped += 1;
        for &stereo_sample in SILENCE.iter() {
            unsafe { volatile_audio.write_volatile(stereo_sample) };
        }
    }
    // copy the record buffer
    if codec.csr.rf(utra::audio::RX_STAT_DATAREADY) != 1 {
        codec.rx_stat_errors += 1;
    }
    let rx_rdcount = codec.csr.rf(utra::audio::RX_STAT_RDCOUNT) as usize;
    let rx_wrcount = codec.csr.rf(utra::audio::RX_STAT_WRCOUNT) as usize;

    let mut rec_buf: [u32; FIFO_DEPTH] = [ZERO_PCM as u32 | (ZERO_PCM as u32) << 16; FIFO_DEPTH];
    for stereo_sample in rec_buf.iter_mut() {
        unsafe{ *stereo_sample = volatile_audio.read_volatile(); }
    }
    match codec.rec_buffer.nq_frame(rec_buf) {
        Ok(()) => {},
        Err(_buff) => {
            codec.rec_frames_dropped += 1
        },
    }

    // if the buffer is low, let the audio handler know we used up another frame!
    if codec.play_buffer.readable_count() < 6 && !codec.drain {
        xous::try_send_message(codec.conn,
            xous::Message::new_scalar(Opcode::AnotherFrame.to_usize().unwrap(), rx_rdcount, rx_wrcount, 0, 0)).unwrap();
    }

    codec.csr.wfo(utra::audio::EV_PENDING_RX_READY, 1);
}

impl Codec {
    pub fn new(conn: xous::CID, xns: &xous_names::XousNames) -> Codec {
        let csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::audio::HW_AUDIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map Audio CSR range");
        let fifo = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_AUDIO_MEM),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map Audio CSR range");

        let llio = llio::Llio::new(xns);
        let i2c = llio::I2c::new(xns);

        Codec {
            csr: CSR::new(csr.as_mut_ptr() as *mut u32),
            susres_manager: RegManager::new(csr.as_mut_ptr() as *mut u32),
            fifo,
            llio,
            i2c,
            ticktimer: ticktimer_server::Ticktimer::new().expect("can't connect to the ticktimer"),
            play_buffer: FrameRing::new(),
            play_frames_dropped: 0,
            rec_buffer: FrameRing::new(),
            rec_frames_dropped: 0,
            powered_on: false,
            initialized: false,
            live: false,
            conn,
            tx_stat_errors: 0,
            rx_stat_errors: 0,
            drain: false,
            speaker_gain: -6.0,
            headphone_left_gain: -15.0,
            headphone_right_gain: -15.0,
        }
    }

    fn trace(&mut self) {
        if self.tx_stat_errors > 0 || self.rx_stat_errors > 0 {
            log::trace!("drop p:{} r:{} | staterr tx:{} rx:{}", self.play_frames_dropped, self.rec_frames_dropped, self.tx_stat_errors, self.rx_stat_errors);
            self.tx_stat_errors = 0;
            self.rx_stat_errors = 0;
        }
    }
    fn trace_rx(&self) {
        log::trace!("T rd {} wr {}", self.csr.rf(utra::audio::RX_STAT_RDCOUNT), self.csr.rf(utra::audio::RX_STAT_WRCOUNT));
    }

    pub fn suspend(&mut self) {
        self.susres_manager.suspend();
        if self.powered_on {
            self.llio.audio_on(false).unwrap(); // force the codec into an off state for resume
        }
    }
    pub fn resume(&mut self) {
        self.ticktimer.sleep_ms(470).unwrap(); // audio code resume has the lowest priority, it should only resume after most other activities stabilized
        if self.powered_on {
            self.llio.audio_on(true).unwrap(); // this is a blocking scalar
            self.ticktimer.sleep_ms(2).unwrap(); // give the codec a moment to power up before writing to it
            // spec is 1ms, but set 2 because of OS timing jitter
            if self.initialized {
                self.init();
            }
        }
        self.susres_manager.resume();
        self.set_speaker_gain_db(self.speaker_gain);
        self.set_headphone_gain_db(self.headphone_left_gain, self.headphone_right_gain);
    }

    pub fn init(&mut self) {
        // this should only be called once per reboot
        if !INTERRUPT_HOOKED.swap(true, SeqCst) {
            xous::claim_interrupt(
                utra::audio::AUDIO_IRQ,
                audio_handler,
                self as *mut Codec as *mut usize,
            )
            .expect("couldn't claim audio irq");
            self.csr.wfo(utra::audio::EV_PENDING_RX_READY, 1);

            self.susres_manager.push(RegOrField::Reg(utra::audio::RX_CTL), None);
            self.susres_manager.push(RegOrField::Reg(utra::audio::TX_CTL), None);
            self.susres_manager.push_fixed_value(RegOrField::Reg(utra::audio::EV_PENDING), 0xFFFF_FFFF);
            self.susres_manager.push(RegOrField::Reg(utra::audio::EV_ENABLE), None);
        }

        // this may be called repeatedly, e.g if the code was put through suspend/resume
        log::trace!("audio_clocks");
        self.audio_clocks();
        log::trace!("audio_ports");
        self.audio_ports();
        log::trace!("audio_mixer");
        self.audio_mixer();
        // this restores the user state of gain, which is overriden during the audio_mixer() reset sequence
        self.set_speaker_gain_db(self.speaker_gain);
        self.set_headphone_gain_db(self.headphone_left_gain, self.headphone_right_gain);
        log::trace!("audio initialized!");
        self.initialized = true;
    }

    pub fn nq_play_frame(&mut self, frame: [u32; FIFO_DEPTH]) -> Result<(), [u32; FIFO_DEPTH]> {
        self.play_buffer.nq_frame(frame)
    }
    pub fn dq_rec_frame(&mut self) -> Option<[u32; FIFO_DEPTH]> {
        self.rec_buffer.dq_frame()
    }
    pub fn free_play_frames(&self) -> usize {
        self.play_buffer.writeable_count()
    }
    pub fn can_play(&self) -> bool {
        !self.play_buffer.is_empty()
    }
    pub fn drain(&mut self) {
        self.drain = true;
    }
    pub fn available_rec_frames(&self) -> usize {
        self.rec_buffer.readable_count()
    }

    pub fn power(&mut self, state: bool) {
        self.llio.audio_on(state).expect("couldn't set audio power state");
        self.powered_on = state;
        if state == false {
            self.initialized = false;
        }
    }

    pub fn is_on(&self) -> bool {
        self.powered_on
    }
    pub fn is_init(&self) -> bool {
        self.initialized
    }
    pub fn is_live(&self) -> bool {
        self.live
    }

    pub fn set_speaker_gain_db(&mut self, gain_db: f32) {
        self.i2c.i2c_mutex_acquire();
        self.speaker_gain = gain_db;
        if gain_db <= -79.0 {
            // mute
            self.w(0, &[1]); // select page 1
            self.w(32, &[0b0_0_00011_0]); // class D amp powered off
        } else {
            let code = analog_volume_db_to_code(gain_db);
            self.w(0, &[1]); // select page 1
            self.w(32, &[0b1_0_00011_0]); // class D amp powered on
            self.w(38, &[
                0b1_000_0000 | code,
                ]);
        }
        self.i2c.i2c_mutex_release();
    }

    pub fn set_headphone_gain_db(&mut self, gain_db_left: f32, gain_db_right: f32) {
        self.headphone_left_gain = gain_db_left;
        self.headphone_right_gain = gain_db_right;
        self.i2c.i2c_mutex_acquire();
        if gain_db_left <= -79.0 && gain_db_right <= -79.0 {
            // mute
            self.w(0, &[1]); // select page 1
            self.w(31, &[0b0_0_0_10_1_0_0]); // headphones powered down
        } else {
            let code_left = analog_volume_db_to_code(gain_db_left);
            let code_right = analog_volume_db_to_code(gain_db_right);
            self.w(0, &[1]); // select page 1
            self.w(31, &[0b1_1_00011_0]); // headphones powered up
            self.w(36, &[
                0b1_000_0000 | code_left, // HPL
                0b1_000_0000 | code_right, // HPR
                ]);
        }
        self.i2c.i2c_mutex_release();
    }
    /// Convenience wrapper for I2C transactions. Multiple I2C ops that have to be execute atomically must be manually guarded with a i2c_mutex_[acquire/release]
    fn w(&mut self, adr: u8, data: &[u8]) -> bool {
        // log::info!("writing to 0x{:x}, {:x?}", adr, data);
        match self.i2c.i2c_write(TLV320AIC3100_I2C_ADR, adr, data) {
            Ok(status) => {
                //log::trace!("write returned with status {:?}", status);
                match status {
                    I2cStatus::ResponseWriteOk => true,
                    I2cStatus::ResponseBusy => false,
                    _ => {log::error!("try_send_i2c unhandled response: {:?}", status); false},
                }
            }
            _ => {log::error!("try_send_i2c unhandled error"); false}
        }
    }
    /// Convenience wrapper for I2C transactions. Multiple I2C ops that have to be execute atomically must be manually guarded with a i2c_mutex_[acquire/release]
    fn r(&mut self, adr: u8, data: &mut[u8]) -> bool {
        match self.i2c.i2c_read(TLV320AIC3100_I2C_ADR, adr, data) {
            Ok(status) => {
                match status {
                    I2cStatus::ResponseReadOk => true,
                    I2cStatus::ResponseBusy => false,
                    _ => {log::error!("try_send_i2c unhandled response: {:?}", status); false},
                }
            }
            _ => {log::error!("try_send_i2c unhandled error"); false}
        }
    }

    pub fn get_headset_code(&mut self) -> u8 {
        self.i2c.i2c_mutex_acquire();
        self.w(0, &[0]);
        let mut code: [u8; 1] = [0; 1];
        if !self.r(67, &mut code) {
            log::warn!("headset code read unsuccessful");
        };
        self.i2c.i2c_mutex_release();
        code[0]
    }

    pub fn get_dacflag_code(&mut self) -> u8 {
        self.i2c.i2c_mutex_acquire();
        self.w(0, &[0]);
        let mut code: [u8; 1] = [0; 1];
        self.r(37, &mut code);
        self.i2c.i2c_mutex_release();
        code[0]
    }

    pub fn get_hp_status(&mut self) -> u8 {
        self.i2c.i2c_mutex_acquire();
        self.w(0, &[1]);
        let mut code: [u8; 1] = [0; 1];
        self.r(31, &mut code);
        self.i2c.i2c_mutex_release();
        code[0]
    }

    pub fn get_i2s_config(&mut self) -> [u8; 4] {
        self.i2c.i2c_mutex_acquire();
        self.w(0, &[0]);
        let mut code: [u8; 4] = [0; 4];
        self.r(27, &mut code);
        self.i2c.i2c_mutex_release();
        code
    }

    /// audio_clocks() sets up the default clocks for 8kHz sampling rate, assuming a 12MHz MCLK input
    ///
    /// fIN = 12 MHz
    /// M = 2.5
    /// N = 32   (PLL freq = 153.6MHz)
    /// N_MOD = 0
    /// P = 12.5
    /// fOUT = 12_288_000 Hz
    ///
    /// sample rate = 8_000
    /// oversampling rate (OSR) = 128
    /// local divider = 12
    /// 8_000 * 128 * 12 = 12_288_000 Hz
    ///
    fn audio_clocks(&mut self) {
        self.i2c.i2c_mutex_acquire();
        self.w(0, &[0]);  // select page 0
        self.w(1, &[1]);  // software reset
        self.ticktimer.sleep_ms(2).unwrap(); // reset happens in 1 ms; +1 ms due to timing jitter uncertainty

        self.w(0, &[0]);  // select page 0

        // select PLL_CLKIN = MCLK; CODEC_CLKIN = PLL_CLK
        self.w(4, &[0b0000_0011]);

        // fs = 8kHz
        // PLL_CLKIN = 12MHz
        // PLLP = 1, PLLR = 1, PLLJ = 7, PLLD = 1680, NDAC = *12*, MDAC = 7, DOSR = 128, MADC = 2 , NADC = *42*
        // ^^ from page 68 of datasheet, fs=48kHz/12MHz clkin line, with *bold* items multiplied by 6 to get to 8kHz
        self.w(5, &[
            0b1001_0001,  // P, R = 1, 1 and pll powered up
            7,            // PLLJ = 7
            ((1680 >> 8) & 0xFF) as u8, // D MSB of 1680
            (1680 & 0xFF) as u8,        // D LSB of 1680
            ]);

        self.w(11, &[
            0x80 | 12,  // NADC = 12 (set to 2 for 48kHz)
            0x80 | 7,   // MDAC = 7
            0,   // DOSR = MSB of 128
            128, // DOSR = LSB of 128
        ]);

        self.w(18, &[
            0x80 | 42,  // NADC = 42 (set to 7 for 48kHz)
            0x80 | 2,   // MADC = 2
            128, // AOSR = 128
        ]);
        self.i2c.i2c_mutex_release();
    }

    /// audio_ports() sets up the digital port bitwidths, modes, and syncs
    ///
    /// From the hardware i2s block as implemented on betrusted-soc:
    /// 16 bits per sample, 32 bit word width, stero, master mode, left-justified, MSB first
    fn audio_ports(&mut self) {
        self.i2c.i2c_mutex_acquire();
        self.w(0, &[0]); // select page 0

        // 32 bits/word * 2 channels * 8000 samples/s = 512_000 = BCLK
        // pick off of DAC_MOD_CLK = 1.024MHz
        self.w(27, &[
            0b00_00_1_1_0_1, // I2S standard, 16 bits per sample, BCLK output, WCLK output, DOUT is Hi-Z when unused
            0b0,           // no offset on left justification
            0b0000_0_1_01, // BDIV_CLKIN = DAC_MOD_CLK, BCLK active even when powered down
            0b1000_0010    // BCLK_N_VAL = 2, N divider is powered up
            ]);

        // "word width" (WCLK) timing is implied based on the DAC fs computed
        // at the end of the clock tree, and WCLK simply toggles every other sample, so there is no
        // explicit WCLK divider

        // turn on headset detection
        self.w(0, &[0]); // select page 0
        // detection enabled, 64ms glitch reject, 8ms button glitch reject
        self.w(67, &[0b1_00_010_01] );

        // use auto volume control -- DO WE WANT THIS???
        //self.w(116, &[0b1_1_01_0_001] );
        self.i2c.i2c_mutex_release();
    }

    pub fn audio_loopback(&mut self, do_loop:bool) {
        self.i2c.i2c_mutex_acquire();
        self.w(0, &[1]); // select page 1

        // DAC routing -- route DAC to mixer channel, don't loopback MIC
        if do_loop {
            self.w(35, &[0b01_0_0_01_0_0]);
        } else {
            self.w(35, &[0b01_0_1_01_1_0]);
        }
        self.i2c.i2c_mutex_release();
    }

    /// set up the audio mixer to sane defaults
    fn audio_mixer(&mut self) {
        self.i2c.i2c_mutex_acquire();
        ////////// SETUP DAC -- this is on page 0
        self.w(0, &[0]); // select page 0
        // DAC setup - both channels on, soft-stepping enabled, left-to-left, right-to-right
        self.w(63, &[0b1_1_01_01_00]);
        // DAC volume - neither DACs muted, independent volume controls
        self.w(64, &[0b0000_0_0_00]);
        // DAC left volume control
        self.w(65, &[0b1111_0110]); // -5dB
        // DAC right volume control
        self.w(66, &[0b1111_0110]); // -5dB

        ///////// VOLUME, PGA CONTROLS -- PAGE 1
        self.w(0, &[1]); // select page 1

        // DAC routing -- route DAC to mixer channel, don't loopback MIC
        self.w(35, &[0b01_0_0_01_0_0]);
        //self.w(35, &[0b01_0_1_01_1_0]);

        // internal volume control
        self.w(36, &[
            0b1_001_1110, // HPL channel control on, -15dB
            0b1_001_1110, // HPR channel control on, -15dB
            0b1_000_1100, // SPK control on, -6dB
            ]);

        // driver PGA control
        self.w(40, &[
            0b0_0011_111, // HPL driver PGA = 3dB, not muted, all gains applied
            0b0_0011_111, // HPR driver PGA = 3dB, not muted, all gains applied
            0b000_01_1_0_1, // SPK gain = 12 dB, driver not muted, all gains applied
            ]);

            // HP driver control -- 16us short circuit debounce, best DAC performance, HPL/HPR as headphone drivers
        self.w(44, &[0b010_11_0_0_0]);

        // MICBIAS control -- micbias always on, set to 2.5V
        self.w(46, &[0b0_000_1_0_10]);

        // MIC PGA
        self.w(47, &[60]); // target 30dB, code is (target * 2)dB

        // fine-gain input selection for P_terminal -- only MIC1RP selected, with RIN=10kohm
        self.w(48, &[0b00_01_00_00]);
        // M_terminal select -- CM selected with RIN = 10k
        self.w(49, &[0b01_00_00_00]);
        // CM settincgs - MIC1LP/MIC1LM connected to CM; MIC1RP is floating
        self.w(50, &[0b1_0_1_00000]);

        // don't change power control bits on SC
        self.w(30, &[0b1_1]);

        // class D amp is powered on
        self.w(32, &[0b1_0_00011_0]);

        // HPL on, HPR on, OCM = 1.65V, limit on short circuit
        self.w(31, &[0b1_1_0_10_1_0_0]);

        ////////// SETUP ADC & AGC -- this is on page 0
        self.w(0, &[0]); // select page 0
        // ADC setup -- ADC powered on, digital MIC not used
        self.w(81, &[0b1_0_00_0_0_00]);
        // ADC digital volume conrol -- not muted, 0dB gain
        self.w(82, &[0b0_000_0000]);
        // ADC digital volume control coarse adjust
        self.w(83, &[0b0]); // +0.0 dB

        self.w(86, &[
            0b1_011_0000, // AGC enabled, target level = -12dB
            0b00_10101_0, // hysteresis 1dB, noise threshold = -((value-1)*2 + 30): 21 => -70dB
            100, // max gain = code/2 dB
            0b_00010_000, // attack time = 0b_acode_mul = (acode*32*mul)/Fs
            0b_01101_000, // decay time  = 0b_dcode_mul = (dcode*32*mul)/Fs
            0x01, // noise debounce time = code*4 / fs
            0x01, // signal debounce time = code*4 / fs
            ]);
        self.i2c.i2c_mutex_release();
    }

    /// set up the betrusted-side signals
    pub fn audio_i2s_start(&mut self) {
        /*
        self.csr.wfo(utra::audio::RX_CTL_RESET, 1);
        self.csr.wfo(utra::audio::TX_CTL_RESET, 1);
        */
        let volatile_audio = self.fifo.as_mut_ptr() as *mut u32;
        for _ in 0..FIFO_DEPTH*2 {
            unsafe { (volatile_audio).write(ZERO_PCM as u32 | (ZERO_PCM as u32) << 16); }  // prefill TX fifo with zero's
        }

        // enable interrupts on the RX_READY
        self.csr.wfo(utra::audio::EV_PENDING_RX_READY, 1); // clear any pending interrupt
        self.csr.wfo(utra::audio::EV_ENABLE_RX_READY, 1);

        // this sets everything running
        self.csr.wfo(utra::audio::RX_CTL_ENABLE, 1);
        self.csr.wfo(utra::audio::TX_CTL_ENABLE, 1);
        self.drain = false;
        self.live = true;
    }

    pub fn audio_i2s_stop(&mut self) {
        self.csr.wfo(utra::audio::EV_ENABLE_RX_READY, 0);
        self.csr.wfo(utra::audio::EV_PENDING_RX_READY, 1);

        self.csr.wfo(utra::audio::RX_CTL_ENABLE, 0);
        self.csr.wfo(utra::audio::TX_CTL_ENABLE, 0);

        log::info!("playback stopped. frames dropped: p{} r{} / errors: tx{} rx{}",
            self.play_frames_dropped, self.rec_frames_dropped, self.tx_stat_errors, self.rx_stat_errors);
        self.play_frames_dropped = 0;
        self.rec_frames_dropped = 0;
        self.tx_stat_errors = 0;
        self.rx_stat_errors = 0;

        self.csr.wfo(utra::audio::RX_CTL_RESET, 1);
        self.csr.wfo(utra::audio::TX_CTL_RESET, 1);
        self.live = false;
        self.drain = true;
        self.play_buffer.clear();
        self.rec_buffer.clear();
    }

    /// this is a testing-only function which does a double-buffered audio loopback
    pub fn audio_loopback_poll(&mut self, buf_a: &mut [u32; FIFO_DEPTH], buf_b: &mut [u32; FIFO_DEPTH], toggle: bool) -> bool {
        let volatile_audio = self.fifo.as_mut_ptr() as *mut u32;

        if (self.csr.rf(utra::audio::TX_STAT_FREE) == 1) && (self.csr.rf(utra::audio::RX_STAT_DATAREADY) == 1) {
            for i in 0..FIFO_DEPTH {
                if toggle {
                    unsafe{ buf_a[i] = *volatile_audio; }
                    unsafe { *volatile_audio = buf_b[i]; }
                } else {
                    unsafe{ buf_b[i] = *volatile_audio; }
                    unsafe { *volatile_audio = buf_a[i]; }
                }
            }
            // wait for the done flags to clear; with an interrupt-driven system, this isn't necessary
            while (self.csr.rf(utra::audio::TX_STAT_FREE) == 1) & (self.csr.rf(utra::audio::RX_STAT_DATAREADY) == 1) {}
            // indicate we had an audio event
            true
        } else {
            false
        }
    }
}
