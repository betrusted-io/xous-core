mod colorwheel;
use std::time::Duration;

use arbitrary_int::{Number, u5};
use bao1x_api::bio::*;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use colorwheel::*;

pub struct Colorwheel {
    bio_ss: Bio,
    bio_pin: u5,
    _led_count: u8,
    _color_rate: u8,
    // handles have to be kept around or else the underlying CSR is dropped
    _tx_handle: CoreHandle,
    // the CoreCsr is a convenience object that manages the CSR view of the handle
    _tx: CoreCsr,
    // tracks the resources used by the object
    resource_grant: ResourceGrant,
}

impl Resources for Colorwheel {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "colorwheel".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo1],
            static_pins: vec![],
            dynamic_pin_count: 1,
        }
    }
}

impl Drop for Colorwheel {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_dynamic_pin(self.bio_pin.as_u8(), &Colorwheel::resource_spec().claimer).unwrap();
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl Colorwheel {
    pub fn new(
        bio_pin: u5,
        led_count: u8,
        color_rate: u8,
        io_mode: Option<IoConfigMode>,
    ) -> Result<Self, BioError> {
        let mut bio_ss = Bio::new();
        // claim core resource and initialize it
        let resource_grant = bio_ss.claim_resources(&Self::resource_spec())?;
        let config = CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqInt(6_666_667) };
        bio_ss.init_core(resource_grant.cores[0], colorwheel_bio_code(), config)?;
        bio_ss.set_core_run_state(&resource_grant, true);

        // claim pin resource - this only claims the resource, it does not configure it
        bio_ss.claim_dynamic_pin(bio_pin.as_u8(), &Colorwheel::resource_spec().claimer)?;
        // now configure the claimed resource
        let mut io_config = IoConfig::default();
        io_config.mapped = 1 << bio_pin.as_u32();

        // snap the outputs to the quantum of the configured core
        // don't use this - it causes ws2812 to not be compatible with other applications, e.g.
        // captouch. The main drawback is the timing is every so slightly off but it seems
        // within tolerance.
        // io_config.snap_outputs = Some(resource_grant.cores[0].into());

        io_config.mode = io_mode.unwrap_or(IoConfigMode::Overwrite);
        bio_ss.setup_io_config(io_config).unwrap();

        // safety: fifo1 is stored in this object so they aren't Drop'd before the object is
        // destroyed
        let tx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo1) }?.expect("Didn't get FIFO1 handle");

        let mut tx = CoreCsr::from_handle(&tx_handle);
        tx.csr.wo(utralib::utra::bio_bdma::SFR_TXF1, bio_pin.as_u32());
        tx.csr.wo(utralib::utra::bio_bdma::SFR_TXF1, led_count as u32);
        tx.csr.wo(utralib::utra::bio_bdma::SFR_TXF1, color_rate as u32);

        Ok(Self {
            bio_ss,
            bio_pin,
            _led_count: led_count,
            _color_rate: color_rate,
            _tx: CoreCsr::from_handle(&tx_handle),
            // safety: tx and rx are wrapped in CSR objects whose lifetime matches that of the handles
            _tx_handle: tx_handle,
            resource_grant,
        })
    }

    /// Call this immediately after setting up Colorwheel, because when the object goes out
    /// of scope, the program stops running. This basically is just a placeholder to keep
    /// the object around long enough.
    pub fn run(&mut self, duration: Option<Duration>) {
        if let Some(d) = duration {
            std::thread::sleep(d);
        } else {
            log::info!("looping forever");
            loop {
                // loop forever
            }
        }
    }
}
