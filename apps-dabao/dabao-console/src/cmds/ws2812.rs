use bao1x_api::bio::{BioApi, CoreConfig, CoreRunSetting, IoConfig};
use bao1x_api::{IoSetup, bio_code, iox};
use utralib::utra::bio_bdma;
use utralib::*;

use crate::cmds::ShellCmdApi;

pub struct Ws2812 {}
impl Ws2812 {
    pub fn new() -> Self { Ws2812 {} }
}

// WS2812B timings - base cycle time is 210ns
// 0: Hi 2 cycles, low 4 cycles
// 1: Hi 4 cycles, low 2 cycles
// WS2812C timings - base cycle time is 312.5ns -0/+50ns
// 0: Hi 1 cycle, low 3 cycle
// 1: Hi 2 cycle, low 2 cycle

impl<'a> ShellCmdApi<'a> for Ws2812 {
    cmd_api!(ws2812);

    fn process(&mut self, args: String, _env: &mut super::CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "ws2812 [test_b]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "test_b" => {
                    let iox = iox::IoxHal::new();
                    iox.setup_pin(
                        iox::IoxPort::PB,
                        5,
                        Some(iox::IoxDir::Output),
                        None,
                        None,
                        Some(iox::IoxEnable::Enable),
                        Some(iox::IoxEnable::Enable),
                        Some(iox::IoxDriveStrength::Drive2mA),
                    );

                    let mut bio = bao1x_hal::bio::Bio::new();
                    // setup pins
                    let mut io_config = IoConfig::default();
                    io_config.mapped = 1 << 5; // BIO5
                    // snap the outputs to the quantum of Core0
                    io_config.snap_outputs = Some(bao1x_api::bio::BioCore::Core1);
                    bio.setup_io_config(io_config).unwrap();

                    // 4_761_904Hz = 1/(210 ns)
                    // 6_666_667Hz = 1/(150 ns)
                    let config =
                        CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqInt(6_666_667) };
                    // load the code
                    bio.init_core(bao1x_api::bio::BioCore::Core1, &ws2812b_kernel(), 0, config).unwrap();

                    // start the core
                    bio.set_core_state([
                        CoreRunSetting::Unchanged,
                        CoreRunSetting::Start,
                        CoreRunSetting::Stop,
                        CoreRunSetting::Stop,
                    ])
                    .unwrap();

                    // grab a handle
                    let core_handle =
                        unsafe { bio.get_core_handle(bao1x_api::bio::Fifo::Fifo1).unwrap().unwrap() };
                    let core_handle1 =
                        unsafe { bio.get_core_handle(bao1x_api::bio::Fifo::Fifo2).unwrap().unwrap() };
                    // now start jamming data into the handle...we should see pixels come out!

                    // safety: core_handle lives as long as the CSR handle that we generate from it
                    let mut handle: CSR<u32> = CSR::new(unsafe { core_handle.handle() as *mut u32 });
                    let handle1: CSR<u32> = CSR::new(unsafe { core_handle1.handle() as *mut u32 });

                    // first value is the pin mask of the LED strip
                    log::info!("sending in mask {:x}", io_config.mapped);
                    handle.wo(bio_bdma::SFR_TXF1, io_config.mapped);

                    let mut hue: [f32; 6] = [0.0, 30.0, 60.0, 90.0, 120.0, 150.0];

                    loop {
                        for i in 0..6 {
                            // Generate RGB color from current hue (full saturation and value)
                            let (r, g, b) = hsv_to_rgb(hue[i], 1.0, 0.05);

                            // Pack into 24-bit RGB value
                            let rgb_value: u32 = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);

                            // Increment hue and wrap around
                            hue[i] += 1.0;
                            if hue[i] >= 360.0 {
                                hue[i] = 0.0;
                            }
                            let go = if i != 5 { 0x0 } else { 0x1_00_00_00 };
                            handle.wo(bio_bdma::SFR_TXF1, rgb_value | go);
                        }
                        // check if the data was sent
                        while handle1.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2) == 0 {
                            xous::yield_slice();
                        }
                        let _ = handle1.r(bio_bdma::SFR_RXF2);
                        _env.ticktimer.sleep_ms(10).ok();
                    }
                }
                "basic" => {
                    let mut bio = bao1x_hal::bio::Bio::new();
                    // setup pins
                    let mut io_config = IoConfig::default();
                    io_config.mapped = 0xff;
                    bio.setup_io_config(io_config).unwrap();

                    // 2.38105
                    let config =
                        CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqInt(4_761_904) };
                    // load the code
                    bio.init_core(bao1x_api::bio::BioCore::Core1, &basic_test(), 0, config).unwrap();

                    // start the core
                    bio.set_core_state([
                        CoreRunSetting::Unchanged,
                        CoreRunSetting::Start,
                        CoreRunSetting::Stop,
                        CoreRunSetting::Stop,
                    ])
                    .unwrap();

                    // grab a handle
                    let core_handle0 =
                        unsafe { bio.get_core_handle(bao1x_api::bio::Fifo::Fifo1).unwrap().unwrap() };
                    let core_handle1 =
                        unsafe { bio.get_core_handle(bao1x_api::bio::Fifo::Fifo2).unwrap().unwrap() };
                    // now start jamming data into the handle...we should see pixels come out!

                    // safety: core_handle lives as long as the CSR handle that we generate from it
                    let handle: CSR<u32> = CSR::new(unsafe { core_handle0.handle() as *mut u32 });
                    let handle1: CSR<u32> = CSR::new(unsafe { core_handle1.handle() as *mut u32 });
                    _env.ticktimer.sleep_ms(100).ok();
                    log::info!("fifo level: {}", handle.r(bio_bdma::SFR_FLEVEL));
                    for i in 0..8 {
                        unsafe {
                            handle.base().add(bio_bdma::SFR_TXF1.offset()).write_volatile(i as u32);
                        }
                        log::info!("rbk1 {:x}", handle1.r(bio_bdma::SFR_RXF2));
                    }
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> (u8, u8, u8) {
    let c = value * saturation;
    let h = hue / 60.0;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = value - c;

    let (r, g, b) = if h < 1.0 {
        (c, x, 0.0)
    } else if h < 2.0 {
        (x, c, 0.0)
    } else if h < 3.0 {
        (0.0, c, x)
    } else if h < 4.0 {
        (0.0, x, c)
    } else if h < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

#[rustfmt::skip]
bio_code!(basic_test, BASIC_TEST_START, BASIC_TEST_END,
    /*
    "mv x1, x0",
    "addi x1, x1, 1",
    "mv x16, x1",
    "addi x1, x1, 1",
    "mv x16, x1",
    "addi x1, x1, 1",
    "mv x16, x1",
    "addi x1, x1, 1",
    "mv x16, x1"
    */

    // add loopback on fifo
    "10:",
    "mv x1, x16",
    "addi x2, x1, 1",
    "mv x17, x2",
    "addi x3, x1, 16",
    "mv x18, x3",
    "addi x4, x1, 32",
    "mv x19, x4",
    "j 10b"

    // quantum toggle test
    /*
    "li x1, 0x20",
    "mv x26, x1",
    "mv x24, x1",
    "10:",
    "mv x22, x1",
    "mv x20, x0",
    "mv x23, x0",
    "mv x20, x0",
    "j 10b"
    */

);

// WS2812B kernel
//
// Data is sent in via FIFO0. The first piece of data sent in is the last in the LED chain.
// Data has the following format:
// bit 24     - 0 means more data. 1 means transmit all previously sent values.
// bits 23:16 - g[7:0]
// bits 15:8  - r[7:0]
// bits 7:0   - b[7:0]
//
// Upon transmission, the buffer is cleared and the data is built up again from the last LED in the chain.
//
// 423 (414)ns / 843 ns total 1257ns (state A)
// 835ns / 418 ns total 1256ns (state B)
#[rustfmt::skip]
bio_code!(ws2812b_kernel, WS2812B_DEV_START, WS2812B_DEV_END,
    "mv x4, x17", // read from FIFO1 - the first argument is the GPIO pin mask we're using to transmit. stash this in x4
    // "mv x18, x4", // debug by looping back -----------
    "mv x26, x4", // apply mask to all GPIO operations
    // setup the pin as an output
    "mv x24, x4",
    // zero the output
    "mv x23, x0",

    // LEDs will go onto the stack
    "li x9, 0x1000000", // bit 24 mask
    "li sp, 0x800", // start of the LED buffer
    "10:",
    // read a 24-bit color number from FIFO1
    "mv x8, x17",
    "sw x8, 0(sp)",
    "addi sp, sp, 4", // stack builds *UP*, away from the code
    "and x10, x9, x8", // AND incoming word with bit 24 mask
    "bne x10, x0, 20f", // jump to the routine at 20f to send if x10 is not 0
    "j 10b", // go back and get more data

    // -- sending loop --
    "20:",
    "li x8, 0x800", // x8 now has the starting address of the data we're going to send
    // x9 is the bit 24 mask used by the above loop
    "li x12, 0x800000", // bit 23 mask
    // loop setup done
    "30:",
    "li x11, 24", // number of bits to shift through
    "lw x10, 0(x8)", // fetch the word to send
    // "mv x18, x10", // debug by looping back -----------
    "31:",
    "and x13, x12, x10", // the bit we're contemplating is at bit 23 position, extract it into x13
    "bne x13, x0, 40f", // if 1, go to 40f, where the 1 routine is

    // zero routine
    // 2 hi
    "mv x22, x4", // sets to 1
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    // 7 lo
    "mv x23, x0", // sets to 0 because we set the GPIO mask earlier
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    // do work during the quantum - zero path
    "slli x10, x10, 1", // shift the pixel value
    "addi x11, x11, -1", // decrement the pixel bit counter
    "mv x20, x0",
    "j 50f", // jump to the loop end check
    // one routine
    "40:",
    // 7 hi
    "mv x22, x4", // sets to 1
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    // 2 lo
    "mv x23, x0", // sets to 0 because we set the GPIO mask earlier
    "mv x20, x0", // wait for quantum
    // do work during the quantum - one path
    "slli x10, x10, 1", // shift the pixel value
    "addi x11, x11, -1", // decrement the pixel bit counter
    "mv x20, x0",

    "50:", // shift and do the next pixel value
    "bne x11, x0, 31b", // go back for more in the loop

    "60:", // check if we've exhausted all the LED values
    "addi x8, x8, 4",
    "bge sp, x8, 30b", // see if we've hit the value of current sp
    "li sp, 0x800", // reset the stack pointer for a fresh fetch
    // wait to reset the chain
    "li x14, 2000", // delay wait time to reset the chain per spec
    "70:",
    "addi x14, x14, -1",
    "mv x20, x0",
    "bne x14, x0, 70b",

    "mv x18, x31", // put a token in to synchronize and indicate that the loop is done

    "j 10b" // go back and get more data
);
