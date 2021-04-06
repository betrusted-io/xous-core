#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::{error, info};

pub enum WhichMessible {
    One,
    Two,
}

pub const TRNG_BUFF_LEN: usize = 512*1024;

#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use xous::MemoryRange;
    use crate::WhichMessible;

    pub struct Trng {
        server_csr: utralib::CSR<u32>,
        xadc_csr: utralib::CSR<u32>,
        messible_csr: utralib::CSR<u32>,
        messible2_csr: utralib::CSR<u32>,
        buffer_a: MemoryRange,
        buffer_b: MemoryRange,
    }

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let trng = unsafe { &mut *(arg as *mut Trng) };
        // just clear the pending request, as this is used as a "wait" until request function
        trng.server_csr.wo(utra::trng_server::EV_PENDING, trng.server_csr.r(utra::trng_server::EV_PENDING));
    }

    impl Trng {
        pub fn new() -> Trng {
            let server_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::trng_server::HW_TRNG_SERVER_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map TRNG Server CSR range");

            let xadc_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::trng::HW_TRNG_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map TRNG xadc CSR range");

            let buff_a = xous::syscall::map_memory(
                xous::MemoryAddress::new(0x4020_0000), // fix this at a known physical address
                None,
                crate::TRNG_BUFF_LEN,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map TRNG comms buffer A");

            let buff_b = xous::syscall::map_memory(
                xous::MemoryAddress::new(0x4030_0000), // fix this at a known physical address
                None,
                crate::TRNG_BUFF_LEN,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map TRNG comms buffer B");

            let messible = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::messible::HW_MESSIBLE_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map messible");

            let messible2 = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::messible2::HW_MESSIBLE2_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map messible");

            let mut trng = Trng {
                server_csr: CSR::new(server_csr.as_mut_ptr() as *mut u32),
                xadc_csr: CSR::new(xadc_csr.as_mut_ptr() as *mut u32),
                buffer_a: buff_a,
                buffer_b: buff_b,
                messible_csr: CSR::new(messible.as_mut_ptr() as *mut u32),
                messible2_csr: CSR::new(messible2.as_mut_ptr() as *mut u32),
            };

            xous::claim_interrupt(
                utra::trng_server::TRNG_SERVER_IRQ,
                handle_irq,
                (&mut trng) as *mut Trng as *mut usize,
            )
            .expect("couldn't claim irq");

            trng
        }

        pub fn init(&mut self) {
            ///// configure power settings and which generator to use
            self.server_csr.wo(utra::trng_server::CONTROL,
                self.server_csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                | self.server_csr.ms(utra::trng_server::CONTROL_POWERSAVE, 0)
               // | self.server_csr.ms(utra::trng_server::CONTROL_AV_DIS, 1)  // disable the AV generator to characterize the RO
               // | self.server_csr.ms(utra::trng_server::CONTROL_RO_DIS, 1)  // disable the RO to characterize only the AV
            );

            ///// configure avalanche
            // delay in microseconds for avalanche poweron after powersave
            // self.server_csr.rmwf(utra::trng_server::AV_CONFIG_POWERDELAY, 50_000);
            self.server_csr.wo(utra::trng_server::AV_CONFIG,
                self.server_csr.ms(utra::trng_server::AV_CONFIG_POWERDELAY, 50_000)
                | self.server_csr.ms(utra::trng_server::AV_CONFIG_SAMPLES, 32)
            );

            ///// configure ring oscillator
            self.server_csr.wo(utra::trng_server::RO_CONFIG,
                self.server_csr.ms(utra::trng_server::RO_CONFIG_DELAY, 4)
                | self.server_csr.ms(utra::trng_server::RO_CONFIG_DWELL, 100)
                | self.server_csr.ms(utra::trng_server::RO_CONFIG_GANG, 1)
                | self.server_csr.ms(utra::trng_server::RO_CONFIG_FUZZ, 1)
                | self.server_csr.ms(utra::trng_server::RO_CONFIG_OVERSAMPLING, 3)
            )

            /* historical note -- for modular noise variants -- do not remove
            self.xadc_csr.rmwf(utra::trng::MODNOISE_CTL_PERIOD, 495); // to set just the period

            self.xadc_csr.wo(utra::trng::MODNOISE_CTL,  // to set also the deadtime
                self.xadc_csr.ms(utra::trng::MODNOISE_CTL_ENA, 1)
                | self.xadc_csr.ms(utra::trng::MODNOISE_CTL_PERIOD, 64)
                | self.xadc_csr.ms(utra::trng::MODNOISE_CTL_DEADTIME, 5)
            );*/
        }

        pub fn messible_send(&mut self, which: WhichMessible, value: u8) {
            match which {
                WhichMessible::One => self.messible_csr.wfo(utra::messible::IN_IN, value as u32),
                WhichMessible::Two => self.messible2_csr.wfo(utra::messible2::IN_IN, value as u32),
            }
        }
        pub fn messible_get(&mut self, which: WhichMessible) -> u8 {
            match which {
                WhichMessible::One => self.messible_csr.rf(utra::messible::OUT_OUT) as u8,
                WhichMessible::Two => self.messible2_csr.rf(utra::messible2::OUT_OUT) as u8,
            }
        }
        pub fn messible_wait_get(&mut self, which: WhichMessible) -> u8 {
            match which {
                WhichMessible::One => {
                    while self.messible_csr.rf(utra::messible::STATUS_HAVE) == 0 {
                        xous::yield_slice();
                    }
                    self.messible_csr.rf(utra::messible::OUT_OUT) as u8
                },
                WhichMessible::Two => {
                    while self.messible2_csr.rf(utra::messible2::STATUS_HAVE) == 0 {
                        xous::yield_slice();
                    }
                    self.messible2_csr.rf(utra::messible2::OUT_OUT) as u8
                },
            }
        }

        pub fn get_buff_a(&self) -> *mut u32 {
            self.buffer_a.as_mut_ptr() as *mut u32
        }
        pub fn get_buff_b(&self) -> *mut u32 {
            self.buffer_b.as_mut_ptr() as *mut u32
        }

        pub fn get_data_eager(&self) -> u32 {
            while self.server_csr.rf(utra::trng_server::STATUS_AVAIL) == 0 {
                xous::yield_slice();
            }
            self.server_csr.rf(utra::trng_server::DATA_DATA)
        }
        pub fn wait_full(&self) {
            while self.server_csr.rf(utra::trng_server::STATUS_FULL) == 0 {
                xous::yield_slice();
            }
        }
        pub fn read_temperature(&self) -> f32 {
            (self.xadc_csr.rf(utra::trng::XADC_TEMPERATURE_XADC_TEMPERATURE) as f32 * 503.975) / 4096.0 - 273.15
        }
        pub fn read_vccint(&self) -> u32 {
            (self.xadc_csr.rf(utra::trng::XADC_VCCINT_XADC_VCCINT) * 3000) / 4096
        }
        pub fn read_vccaux(&self) -> u32 {
            (self.xadc_csr.rf(utra::trng::XADC_VCCAUX_XADC_VCCAUX) * 3000) / 4096
        }
        pub fn read_vccbram(&self) -> u32 {
            (self.xadc_csr.rf(utra::trng::XADC_VCCBRAM_XADC_VCCBRAM) * 3000) / 4096
        }
        pub fn read_vbus(&self) -> u32 {
            (self.xadc_csr.rf(utra::trng::XADC_VBUS_XADC_VBUS) * 5033) / 1000
        }
        pub fn read_usb_p(&self) -> u32 {
            (self.xadc_csr.rf(utra::trng::XADC_USB_P_XADC_USB_P) * 1000) / 4096
        }
        pub fn read_usb_n(&self) -> u32 {
            (self.xadc_csr.rf(utra::trng::XADC_USB_N_XADC_USB_N) * 1000) / 4096
        }

    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    pub struct Trng {
    }

    impl Trng {
        pub fn new() -> Trng {
            Trng {
            }
        }

        pub fn messible_send(&mut self, which: WhichMessible, value: u32) {
        }

        pub fn init(&mut self) {
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Trng;

    log_server::init_wait().unwrap();

    let log_server_id = xous::SID::from_bytes(b"xous-log-server ").unwrap();
    let log_conn = xous::connect(log_server_id).unwrap();

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    // Create a new com object
    let mut trng = Trng::new();
    trng.init();
    // just create buffers out of pointers. Definitely. Unsafe.
    let mut buff_a = trng.get_buff_a() as *mut u32;
    let mut buff_b = trng.get_buff_b() as *mut u32;

    for i in 0..TRNG_BUFF_LEN / 4 {
        // buff_a[i] = trng.get_data_eager();
        unsafe { buff_a.add(i).write_volatile(trng.get_data_eager()) };
    }
    for i in 0..TRNG_BUFF_LEN / 4 {
        // buff_b[i] = trng.get_data_eager();
        unsafe { buff_b.add(i).write_volatile(trng.get_data_eager()) };
    }
    info!("TRNG: starting service");
    let mut phase = 1;
    trng.messible_send(WhichMessible::One, phase); // indicate buffer A is ready to go

    loop {
        if false {
            // to test the powerdown feature
            trng.wait_full();
            ticktimer.sleep_ms(5000).expect("couldn't sleep");
        }

        if false {
            // select this to print XADC data to info!()
            ticktimer.sleep_ms(20).expect("couldn't sleep"); // sleep to allow xadc sampling in case we're in a very tight TRNG request loop
            info!("temperature: {}C", trng.read_temperature());
            info!("vccint: {}mV", trng.read_vccint());
            info!("vccaux: {}mV", trng.read_vccaux());
            info!("vccbram: {}mV", trng.read_vccbram());
            info!("vbus: {}mV", trng.read_vbus());
            info!("usb_p: {}mV", trng.read_usb_p());
            info!("usb_n: {}mV", trng.read_usb_n());
        }

        // to test the full loop
        trng.messible_wait_get(WhichMessible::Two);
        phase += 1;
        if phase % 2 == 1 {
            info!("TRNG: filling A");
            for i in 0..TRNG_BUFF_LEN / 4 {
                //buff_a[i] = trng.get_data_eager();
                unsafe { buff_a.add(i).write_volatile(trng.get_data_eager()) };
            }
            trng.messible_send(WhichMessible::One, phase);
        } else {
            info!("TRNG: filling B");
            for i in 0..TRNG_BUFF_LEN / 4 {
                //buff_b[i] = trng.get_data_eager();
                unsafe { buff_b.add(i).write_volatile(trng.get_data_eager()) };
            }
            trng.messible_send(WhichMessible::One, phase);
        }
    }
}
