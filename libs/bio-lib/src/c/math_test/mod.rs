mod math_test;
use bao1x_api::bio::*;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use math_test::*;
use utralib::utra::bio_bdma;

pub struct MathTest {
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

impl Resources for MathTest {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "math-test".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo0, Fifo::Fifo1],
            static_pins: vec![],
            dynamic_pin_count: 0,
        }
    }
}

impl Drop for MathTest {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl MathTest {
    pub fn new() -> Result<Self, BioError> {
        let mut bio_ss = Bio::new();
        // claim core resource and initialize it
        let resource_grant = bio_ss.claim_resources(&Self::resource_spec())?;
        let config = CoreConfig { clock_mode: bao1x_api::bio::ClockMode::FixedDivider(0, 0) };
        bio_ss.init_core(resource_grant.cores[0], &math_test_bio_code(), 0, config)?;
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

    pub fn test_cos(&mut self) {
        for i in (0..360).step_by(4) {
            // self.bio_ss.debug(self.resource_grant.cores[0]);
            self.tx.csr.wo(bio_bdma::SFR_TXF0, i);
            // let mut dbg_count: usize = 0;
            while self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0 {
                /*
                if dbg_count < 20 {
                    self.bio_ss.debug(self.resource_grant.cores[0]);
                }
                dbg_count = dbg_count.saturating_add(1);
                */
            }
            let result = self.rx.csr.r(bio_bdma::SFR_RXF1) as i32 + 11; // result should be -10 to 10
            let mut line = String::new();
            for _ in 0..result {
                line.push(' ');
            }
            line.push('*');
            log::info!("{:3},{:2}:{}", i, result, line);
        }
    }
}
