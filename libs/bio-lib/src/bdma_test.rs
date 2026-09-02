use core::time;
use std::num::NonZero;

use bao1x_api::bio::*;
use bao1x_api::bio_code;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use utralib::utra::bio_bdma;

pub struct BdmaTest {
    bio_ss: Bio,
    // handles have to be kept around or else the underlying CSR is dropped
    _tx_handle: CoreHandle,
    _rx_handle: CoreHandle,
    // the CoreCsr is a convenience object that manages the CSR view of the handle
    tx: CoreCsr,
    rx: CoreCsr,
    // tracks the resources used by the object
    resource_grant: ResourceGrant,
    phys_mem: xous::MemoryRange,
}

impl Resources for BdmaTest {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "BDMA test".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo1, Fifo::Fifo2],
            static_pins: vec![],
            dynamic_pin_count: 0,
        }
    }
}

impl Drop for BdmaTest {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
        xous::unmap_memory(self.phys_mem).unwrap();
    }
}

impl BdmaTest {
    pub fn new() -> Result<Self, BioError> {
        let mut bio_ss = Bio::new();
        // claim core resource and initialize it
        let resource_grant = bio_ss.claim_resources(&Self::resource_spec())?;
        let config = CoreConfig { clock_mode: bao1x_api::bio::ClockMode::FixedDivider(0, 0) };
        bio_ss.init_core(resource_grant.cores[0], bdma_test_code(), config)?;
        bio_ss.set_core_run_state(&resource_grant, true);
        bio_ss
            .setup_dma_windows(DmaFilterWindows {
                windows: [
                    Some(DmaWindow {
                        base: 0x6100_0000 >> 12,
                        bounds: NonZero::<u32>::new((2 * 1024 * 1024) >> 12).unwrap(),
                    }),
                    None,
                    None,
                    None,
                ],
            })
            .unwrap();

        // safety: fifo1 and fifo2 are stored in this object so they aren't Drop'd before the object is
        // destroyed
        let tx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo1) }?.expect("Didn't get FIFO1 handle");
        let rx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo2) }?.expect("Didn't get FIFO2 handle");

        let mut phys_mem = xous::syscall::map_memory(
            None,
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::RESERVE | xous::MemoryFlags::DEV,
        )
        .unwrap();
        unsafe {
            phys_mem.as_slice_mut().fill(0);
        }

        Ok(Self {
            bio_ss,
            tx: CoreCsr::from_handle(&tx_handle),
            rx: CoreCsr::from_handle(&rx_handle),
            // safety: tx and rx are wrapped in CSR objects whose lifetime matches that of the handles
            _tx_handle: tx_handle,
            _rx_handle: rx_handle,
            resource_grant,
            phys_mem,
        })
    }

    pub fn test(&mut self) {
        log::info!("freq: {}", self.bio_ss.get_bio_freq());
        let virt_addr = self.phys_mem.as_ptr();
        let phys_addr = xous::syscall::virt_to_phys(virt_addr as usize).expect("can't convert v2p");
        const OFFSET: usize = 0x7c;

        log::info!("phys addr: {:x}", phys_addr);
        self.tx.csr.wo(bio_bdma::SFR_TXF1, phys_addr as u32 + OFFSET as u32);
        std::thread::sleep(time::Duration::from_millis(1000));
        bao1x_hal::cache_flush();
        let mut error = false;

        let rx_slice: &[u8] = unsafe { &self.phys_mem.as_slice()[OFFSET..OFFSET + 64] };

        for chunk in rx_slice.chunks(16) {
            log::info!("{:x?}", chunk);
        }
        for (i, &data) in rx_slice.iter().enumerate() {
            if data != (phys_addr + i + OFFSET) as u8 {
                error = true;
            }
        }
        if error {
            log::info!("FAIL");
        } else {
            log::info!("PASS");
        }
    }
}

#[rustfmt::skip]
bio_code!(bdma_test_code, BDMA_TEST_START, BDMA_TEST_END,
  "20:",
    "mv a1, x17",
    "addi a0, a1, 64",
   "1:",
    // "lbu x0, 0(a1)",
    "sb a1, 0(a1)",
    "addi a1, a1, 1",
    "bne a0, a1, 1b",
    "j _start"
);
