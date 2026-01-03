use bao1x_api::bio::*;
use bao1x_api::bio_code;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use utralib::utra::bio_bdma;

// This demo is not compatible with baosec because it uses FIFO0

pub struct MacDemo {
    bio_ss: Bio,
    // handles have to be kept around or else the underlying CSR is dropped
    _tx_handle: CoreHandle,
    _rx_handle: CoreHandle,
    // the CoreCsr is a convenience object that manages the CSR view of the handle
    tx: CoreCsr,
    rx: CoreCsr,
    // tracks the resources used by the object
    resource_grant: ResourceGrant,
}

impl Resources for MacDemo {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "MAC-demo".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo0, Fifo::Fifo1],
            static_pins: vec![],
            dynamic_pin_count: 0,
        }
    }
}

impl Drop for MacDemo {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl MacDemo {
    pub fn new() -> Result<Self, BioError> {
        let mut bio_ss = Bio::new();
        // claim core resource and initialize it
        let resource_grant = bio_ss.claim_resources(&Self::resource_spec())?;
        let config = CoreConfig { clock_mode: bao1x_api::bio::ClockMode::FixedDivider(0, 0) };
        bio_ss.init_core(resource_grant.cores[0], &mac_code(), 0, config)?;
        bio_ss.set_core_run_state(&resource_grant, true);

        // safety: fifo1 and fifo2 are stored in this object so they aren't Drop'd before the object is
        // destroyed
        let tx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo0) }?.expect("Didn't get FIFO0 handle");
        let rx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo1) }?.expect("Didn't get FIFO1 handle");

        Ok(Self {
            bio_ss,
            tx: CoreCsr::from_handle(&tx_handle),
            rx: CoreCsr::from_handle(&rx_handle),
            // safety: tx and rx are wrapped in CSR objects whose lifetime matches that of the handles
            _tx_handle: tx_handle,
            _rx_handle: rx_handle,
            resource_grant,
        })
    }

    pub fn mac_hw(&mut self, a: &[i32], b: i32) -> i32 {
        self.tx.csr.wo(bio_bdma::SFR_TXF0, a.len() as u32);
        self.tx.csr.wo(bio_bdma::SFR_TXF0, b as u32);
        for v in a.iter() {
            self.tx.csr.wo(bio_bdma::SFR_TXF0, *v as u32);
        }
        // wait for computation to finish
        while self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0 {}
        self.rx.csr.r(bio_bdma::SFR_RXF1) as i32
    }

    /// this is the same function as above but run on the main CPU
    pub fn mac_sw(&self, a: &[i32], b: i32) -> i32 {
        let mut r: i32 = 0;
        for &v in a.iter() {
            r += v * b;
        }
        r
    }
}

#[rustfmt::skip]
bio_code!(mac_code, MAC_START, MAC_END,
  // first arg into x16 is the number of elements to MAC
  // second arg is the coefficient
  // remaining args are the vector
  // compute mac = a0 * b + a1 * b + a2 * b ...
  // return value is in x17
  "20:",
    "mv   a0, x16", // number of elements
    "mv   a1, x16", // coefficient
    "mv   a2, x0",  // initialize return value
    "jal  ra, 30f",
    "mv   x17, a2", // return the value
    "j    20b", // go back for more
  "30:",
    "bne  x0, a0, 31f", // test if end
    "ret",
  "31:",
    "addi a0, a0, -1",  // decrement arg counter
    "mv t1, x16",       // fetch vector value: note, we can't multiply directly from a FIFO because while the multi-cycle multiply runs, the FIFO keeps draining
    // "mul  t0, a1, t1",  // multiply
    // "mul  x5, x11, x6", // Translation of above to x-style registers.
    // The mul instruction is converted into a `.word`` because Rust refuses to emit `mul` instructions for global_asm!
    ".word 0x026582b3",
    "add  a2, t0, a2",  // accumulate
    "j    30b"          // loop
);
