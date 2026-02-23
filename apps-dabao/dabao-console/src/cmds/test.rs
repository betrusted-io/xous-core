use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Test {}

impl<'a> ShellCmdApi<'a> for Test {
    cmd_api!(test);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();

        #[allow(unused_variables)]
        let helpstring = "test [proc] [freemem] [interrupts] [panic] [timer] [env]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "timer" => {
                    let start = _env.ticktimer.elapsed_ms();
                    log::info!("Starting test");
                    let mut seconds = 0;
                    loop {
                        let elapsed = _env.ticktimer.elapsed_ms() - start;
                        if elapsed > seconds * 1000 {
                            log::info!("{} s", seconds);
                            seconds += 1;
                        }
                    }
                }
                "panic" => {
                    log::info!("System will panic now");
                    panic!("test panic");
                }
                "proc" => {
                    // hard coded - debug feature - if the platform ABI changes its name or opcode map this
                    // can break, but also this routine is not meant for public
                    // consumption and coding it here avoids breaking dependencies to the Xous API crate.
                    let page_buf = xous::PageBuf::new();
                    xous::rsyscall(xous::SysCall::PlatformSpecific(2, page_buf.as_ptr(), 0, 0, 0, 0, 0))
                        .unwrap();

                    log::info!("Process listing:");
                    for line in page_buf.as_str().lines() {
                        log::info!("{}", line);
                    }
                }
                "freemem" => {
                    // hard coded - debug feature - if the platform ABI changes its name or opcode map this
                    // can break, but also this routine is not meant for public
                    // consumption and coding it here avoids breaking dependencies to the Xous API crate.
                    let page_buf = xous::PageBuf::new();
                    xous::rsyscall(xous::SysCall::PlatformSpecific(1, page_buf.as_ptr(), 0, 0, 0, 0, 0))
                        .unwrap();

                    log::info!("RAM usage:");
                    for line in page_buf.as_str().lines() {
                        log::info!("{}", line);
                    }
                }
                "interrupts" => {
                    // hard coded - debug feature - if the platform ABI changes its name or opcode map this
                    // can break, but also this routine is not meant for public
                    // consumption and coding it here avoids breaking dependencies to the Xous API crate.
                    let page_buf = xous::PageBuf::new();
                    xous::rsyscall(xous::SysCall::PlatformSpecific(3, page_buf.as_ptr(), 0, 0, 0, 0, 0))
                        .unwrap();

                    log::info!("Interrupt handlers:");
                    for line in page_buf.as_str().lines() {
                        log::info!("{}", line);
                    }
                }
                #[cfg(feature = "bio-math-test")]
                "cos" => {
                    let mut math = bio_lib::c::math_test::MathTest::new().unwrap();
                    math.test_cos();
                }
                #[cfg(feature = "bio-wheel-test")]
                "wheel" => {
                    let mut wheel =
                        bio_lib::c::colorwheel::Colorwheel::new(arbitrary_int::u5::from_u8(5), 12, 2, None)
                            .unwrap();
                    // run forever, code blocks here
                    wheel.run(None);
                }
                "temp" => {
                    let adc = bao1x_hal_service::Adc::new();
                    let raw_temp = adc.read_raw(bao1x_hal::udma::AdcSource::Temperature, Some(8));
                    log::info!(
                        "raw: {}, temp: {}",
                        raw_temp,
                        bao1x_hal::udma::Adc::raw_to_temp_celsius(raw_temp)
                    );
                }
                "adc" => {
                    let adc = bao1x_hal_service::Adc::new();
                    // safety - we have manually checked there are no conflicts with this mapping
                    unsafe { adc.enable_channel(bao1x_hal::udma::AdcExtChannel::Adc0) };
                    loop {
                        let raw = adc.read_raw(
                            bao1x_hal::udma::AdcSource::Ext(bao1x_hal::udma::AdcExtChannel::Adc0),
                            Some(8),
                        );
                        log::info!("raw: {} volts: {:.3}V", raw, bao1x_hal::udma::Adc::raw_to_voltage(raw));
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
                "env" => {
                    log::info!("{:?}", std::env::vars());
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
