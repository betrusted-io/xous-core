use utralib::generated::*;
use xous::MemoryRange;
use susres::{RegManager, RegOrField, SuspendResume, ManagedMem};

pub struct Codec {
    csr: utralib::CSR<u32>,
    fifo: MemoryRange,
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
