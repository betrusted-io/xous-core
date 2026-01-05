use std::time::Duration;

use bao1x_api::bio::*;
use bao1x_api::bio_resources::*;
use bao1x_api::{IoSetup, IoxDir, IoxDriveStrength, IoxEnable, IoxFunction, IoxHal, IoxPort, bio_code};
use bao1x_hal::bio::{Bio, CoreCsr};
use utralib::utra::bio_bdma;

const BITS_PER_SAMPLE: usize = 4;
const POWER_ON_DELAY_MILLIS: u64 = 50;

pub struct AvTrng {
    bio_ss: Bio,
    // handles have to be kept around or else the underlying CSR is dropped
    _txrx_handle: CoreHandle,
    // the CoreCsr is a convenience object that manages the CSR view of the handle
    txrx: CoreCsr,
    // tracks the resources used by the object
    resource_grant: ResourceGrant,
    power: (IoxPort, u8),
    iox: IoxHal,
    powered_on: bool,
}

impl Resources for AvTrng {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "Avalanche TRNG".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo0],
            static_pins: vec![],
            dynamic_pin_count: 1,
        }
    }
}

impl Drop for AvTrng {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
        // power down the TRNG
        self.iox.set_gpio_pin_value(self.power.0, self.power.1, bao1x_api::IoxValue::Low);
    }
}

impl AvTrng {
    /// If IoConfigMode is None, the system defaults to Overwrite mode.
    pub fn new(
        data: (IoxPort, u8),
        power: (IoxPort, u8),
        io_mode: Option<IoConfigMode>,
    ) -> Result<Self, BioError> {
        let mut bio_ss = Bio::new();
        let iox = IoxHal::new();

        // claim core resource and initialize it
        let resource_grant = bio_ss.claim_resources(&Self::resource_spec())?;

        iox.setup_pin(
            power.0,
            power.1,
            Some(IoxDir::Output),
            Some(IoxFunction::Gpio),
            None,
            Some(IoxEnable::Disable),
            None,
            Some(IoxDriveStrength::Drive2mA),
        );
        iox.setup_pin(
            data.0,
            data.1,
            Some(IoxDir::Input),
            Some(IoxFunction::Gpio),
            Some(IoxEnable::Enable), // enable the schmitt trigger on this pad
            Some(IoxEnable::Disable),
            None,
            Some(IoxDriveStrength::Drive2mA),
        );
        let claimed_pin =
            iox.set_bio_bit_from_port_and_pin(data.0, data.1).expect("Couldn't allocate TRNG input pin");
        // claim pin resource - this only claims the resource, it does not configure it
        bio_ss.claim_dynamic_pin(claimed_pin, &AvTrng::resource_spec().claimer)?;
        let config =
            CoreConfig { clock_mode: bao1x_api::bio::ClockMode::ExternalPin(BioPin::new(claimed_pin)) };
        bio_ss.init_core(resource_grant.cores[0], &avtrng_bio_code(), 0, config)?;

        // power on
        iox.set_gpio_pin_value(power.0, power.1, bao1x_api::IoxValue::High);
        // wait for power-on
        std::thread::sleep(Duration::from_millis(POWER_ON_DELAY_MILLIS));

        // now configure the claimed resource
        let mut io_config = IoConfig::default();
        io_config.mapped = 1 << claimed_pin as u32;
        io_config.mode = io_mode.unwrap_or(IoConfigMode::Overwrite);
        bio_ss.setup_io_config(io_config).unwrap();

        bio_ss.set_core_run_state(&resource_grant, true);
        let txrx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo0) }?.expect("Didn't get FIFO0 handle");
        let mut txrx = CoreCsr::from_handle(&txrx_handle);
        // send the pin configuration to the TRNG process to kick it off
        txrx.csr.wo(bio_bdma::SFR_TXF0, claimed_pin as u32);

        Ok(Self { bio_ss, txrx, _txrx_handle: txrx_handle, resource_grant, power, iox, powered_on: true })
    }

    pub fn power_down(&mut self) {
        self.iox.set_gpio_pin_value(self.power.0, self.power.1, bao1x_api::IoxValue::Low);
        self.powered_on = false;
    }

    pub fn power_up(&mut self) {
        self.iox.set_gpio_pin_value(self.power.0, self.power.1, bao1x_api::IoxValue::High);
        std::thread::sleep(Duration::from_millis(POWER_ON_DELAY_MILLIS));
        self.powered_on = true;
    }

    pub fn get_u32(&mut self) -> u32 {
        if !self.powered_on {
            self.power_up();
        }

        let mut raw32 = 0;
        for _ in 0..(size_of::<u32>() * 8) / BITS_PER_SAMPLE {
            // wait for the next interval to arrive
            while self.txrx.csr.rf(utralib::utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL0) == 0 {}
            let raw = self.txrx.csr.r(utralib::utra::bio_bdma::SFR_RXF0);
            raw32 <<= BITS_PER_SAMPLE;
            // shift right by one because bit 0 always samples as 0, due to instruction timing
            raw32 |= (raw >> 1) & ((1 << BITS_PER_SAMPLE) - 1)
        }
        raw32
    }
}

#[rustfmt::skip]
bio_code!(avtrng_bio_code, BM_AVTRNG_BIO_START, BM_AVTRNG_BIO_END,
    "mv x1, x16", // get pin for trng input
    "li x2, 1",
    "sll x1, x2, x1", // shift the pin into a bitmask
    "mv x25, x1",  // make it an input
"10:",
    "mv x20, x0", // wait for quantum: this time, the toggle from the TRNG
    "mv x1, x31", // remember aclk time
    "mv x16, x1", // save result to FIFO
    "j 10b" // and do it again
);
