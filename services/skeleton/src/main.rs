#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::FromPrimitive;

use log::info;


#[cfg(any(target_os = "none", target_os = "xous"))]
mod implementation {
    use utralib::generated::*;
    // use crate::api::*;
    use log::info;
    use susres::{RegManager, RegOrField, SuspendResume};

    pub struct Codec {
        csr: utralib::CSR<u32>,
        fifo: xous::MemoryRange,
        susres_manager: RegManager::<{utra::audio::AUDIO_NUMREGS}>,
    }

    impl Codec {
        pub fn new() -> Codec {
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
                utralib::HW_AUDIO_MEM_LEN,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Audio CSR range");

            let mut codec = Codec {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                susres_manager: RegManager::new(csr.as_mut_ptr() as *mut u32),
                fifo,
            };

            codec
        }

        pub fn suspend(&mut self) {
            self.susres_manager.suspend();
        }
        pub fn resume(&mut self) {
            self.susres_manager.resume();
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod implementation {
    use log::info;

    pub struct Codec {
    }

    impl Codec {
        pub fn new() -> Codec {
            Codec {
            }
        }
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }
    }
}


#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Codec;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let codec_sid = xns.register_name(api::SERVER_NAME_CODEC, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", codec_sid);

    let mut codec = Codec::new();

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let sr_cid = xous::connect(codec_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(None, &xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    loop {
        let msg = xous::receive_message(codec_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                codec.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                codec.resume();
            }),
            Some(Opcode::Quit) => {
                log::warn!("Quit received, goodbye world!");
                break;
            },
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(codec_sid).unwrap();
    xous::destroy_server(codec_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
