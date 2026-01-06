use std::time::{Duration, Instant};
use std::u64;

use arbitrary_int::{Number, u5};
use bao1x_api::bio::*;
use bao1x_api::bio_code;
use bao1x_api::bio_resources::*;
use bao1x_api::{IoSetup, IoxDir, IoxEnable};
use bao1x_hal::bio::{Bio, CoreCsr};
use utralib::utra::bio_bdma;

const DEFAULT_THRESHOLD: u32 = 5;
const CALIBRATION_TIMEOUT_MS: u64 = 100;

#[derive(Debug)]
pub enum CaptouchError {
    Timeout,
    NotCalibrated,
}

pub struct Captouch {
    bio_ss: Bio,
    // handles have to be kept around or else the underlying CSR is dropped
    _txrx_handle: CoreHandle,
    // the CoreCsr is a convenience object that manages the CSR view of the handle
    txrx: CoreCsr,
    // tracks the resources used by the object
    resource_grant: ResourceGrant,
    threshold: u32,
    baseline: u32,
}

impl Resources for Captouch {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "Captouch".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo3],
            static_pins: vec![],
            dynamic_pin_count: 1,
        }
    }
}

impl Drop for Captouch {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl Captouch {
    /// If IoConfigMode is None, the system defaults to Overwrite mode.
    pub fn new(pin: u5, io_mode: Option<IoConfigMode>) -> Result<Self, BioError> {
        let iox = bao1x_api::iox::IoxHal::new();
        let port_config = bao1x_api::bio::bio_bit_to_port_and_pin(pin);
        log::debug!("port config: {:?}", port_config);
        iox.setup_pin(
            port_config.0,
            port_config.1,
            Some(IoxDir::Input),
            Some(bao1x_api::IoxFunction::Gpio),
            Some(IoxEnable::Enable),
            Some(IoxEnable::Enable),
            None,
            None,
        );
        let mut bio_ss = Bio::new();
        // claim core resource and initialize it
        let resource_grant = bio_ss.claim_resources(&Self::resource_spec())?;
        let config =
            CoreConfig { clock_mode: bao1x_api::bio::ClockMode::ExternalPin(BioPin::new(pin.as_u8())) };
        bio_ss.init_core(resource_grant.cores[0], &captouch_sense_code(), 0, config)?;

        // claim pin resource - this only claims the resource, it does not configure it
        bio_ss.claim_dynamic_pin(pin.as_u8(), &Captouch::resource_spec().claimer)?;
        // now configure the claimed resource
        let mut io_config = IoConfig::default();
        io_config.mapped = 1 << pin.as_u32();
        io_config.mode = io_mode.unwrap_or(IoConfigMode::Overwrite);
        bio_ss.setup_io_config(io_config).unwrap();

        bio_ss.set_core_run_state(&resource_grant, true);
        // safety: fifo1 and fifo2 are stored in this object so they aren't Drop'd before the object is
        // destroyed
        let txrx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo3) }?.expect("Didn't get FIFO3 handle");
        let mut txrx = CoreCsr::from_handle(&txrx_handle);
        txrx.csr.wo(bio_bdma::SFR_TXF3, 1 << pin.as_u32());

        Ok(Self {
            bio_ss,
            txrx,
            // safety: tx and rx are wrapped in CSR objects whose lifetime matches that of the handles
            _txrx_handle: txrx_handle,
            resource_grant,
            threshold: DEFAULT_THRESHOLD,
            baseline: 0,
        })
    }

    pub fn raw_status(&self) -> u32 { self.txrx.csr.r(bio_bdma::SFR_RXF3) }

    /// Sets the noise threshold for response of the sensor
    pub fn set_threshold(&mut self, threshold: u32) { self.threshold = threshold; }

    pub fn get_baseline(&self) -> u32 { self.baseline }

    /// Waits for any change on the sensor. Generally, if you're going from not-touched to
    /// touched state, this will trigger a result at the not-touched to touched transition.
    /// However, if the sensor is already touched, there is sufficient noise in the reading
    /// that it will return a change value.
    pub fn wait_change(&self, timeout: Option<Duration>) -> bool {
        let start = Instant::now();
        let initial_reading = self.raw_status();
        let mut got_hit = false;
        while !got_hit
            && (Instant::now().duration_since(start) < timeout.unwrap_or(Duration::from_secs(u64::MAX)))
        {
            if self.raw_status() - self.threshold > initial_reading {
                got_hit = true;
            }
        }
        got_hit
    }

    pub fn calibrate(&mut self, timeout: Option<Duration>) -> Result<u32, CaptouchError> {
        let cal_timeout = timeout.unwrap_or(Duration::from_millis(CALIBRATION_TIMEOUT_MS));
        let start = Instant::now();
        let mut baseline = 0;
        let mut within_threshold = 0;
        loop {
            let reading = self.raw_status();

            // we have truly hit calibration - the average has converged
            if reading - baseline == 0 {
                self.baseline = baseline;
                return Ok(baseline);
            }

            // see if we're within the threshold
            if reading - baseline < self.threshold {
                within_threshold += 1;
            } else {
                within_threshold = 0;
            }

            // if we've been within threshold for more than threshold counts, also conclude
            // we've found a baseline. This helps handle the case of an extremely noisy environment,
            // as might be found on a large-area sensor with a lot of noise pick-up.
            if within_threshold > self.threshold {
                self.baseline = baseline;
                return Ok(baseline);
            }

            baseline = (baseline + reading) / 2; // average in the readings
            if Instant::now().duration_since(start) > cal_timeout {
                return Err(CaptouchError::Timeout);
            }
        }
    }

    pub fn is_touched(&self) -> bool {
        assert!(self.baseline != 0, "is_touched() requires sensor to be calibrated first");
        // return `true` if there's more capacitance than the baseline
        self.raw_status() > self.baseline + self.threshold
    }

    pub fn wait_touch(&self, timeout: Option<Duration>) -> Result<(), CaptouchError> {
        if self.baseline == 0 {
            return Err(CaptouchError::NotCalibrated);
        }
        let start = Instant::now();
        let timeout = timeout.unwrap_or(Duration::from_secs(u64::MAX));
        loop {
            if Instant::now().duration_since(start) > timeout {
                return Err(CaptouchError::Timeout);
            }
            if self.is_touched() {
                return Ok(());
            }
        }
    }

    pub fn wait_release(&self, timeout: Option<Duration>) -> Result<(), CaptouchError> {
        if self.baseline == 0 {
            return Err(CaptouchError::NotCalibrated);
        }
        let start = Instant::now();
        let timeout = timeout.unwrap_or(Duration::from_secs(u64::MAX));
        loop {
            if !self.is_touched() {
                return Ok(());
            }
            if Instant::now().duration_since(start) > timeout {
                return Err(CaptouchError::Timeout);
            }
            self.release_polling_interval();
        }
    }

    /// This is necessary for "release" detection because too rapid sensing can cause
    /// the object touching the sensor to "charge down" and you end up measuring an AC
    /// capacitance of the object.
    fn release_polling_interval(&self) { std::thread::sleep(Duration::from_millis(50)); }

    pub fn wait_touch_and_release(&self, timeout: Option<Duration>) -> Result<(), CaptouchError> {
        if self.baseline == 0 {
            return Err(CaptouchError::NotCalibrated);
        }
        let start = Instant::now();
        let timeout = timeout.unwrap_or(Duration::from_secs(u64::MAX));
        enum State {
            WaitInitialRelease,
            WaitTouch,
            WaitFinalRelease,
        }
        let mut state = State::WaitInitialRelease;
        loop {
            match state {
                State::WaitInitialRelease => {
                    if !self.is_touched() {
                        state = State::WaitTouch;
                    }
                }
                State::WaitTouch => {
                    if self.is_touched() {
                        state = State::WaitFinalRelease;
                    }
                }
                State::WaitFinalRelease => {
                    self.release_polling_interval();
                    if !self.is_touched() {
                        return Ok(());
                    }
                }
            }
            if Instant::now().duration_since(start) > timeout {
                return Err(CaptouchError::Timeout);
            }
        }
    }
}

#[rustfmt::skip]
bio_code!(
    captouch_sense_code,
    CAPTOUCH_SENSE_START,
    CAPTOUCH_SENSE_END,

    "mv    x5, x19",         // Load pin mask into register x5.
    "mv    x26, x5",         // Set GPIO mask to our pin.
"10:", // start of sensor loop
    "mv    x24, x5",         // Configure pin as an OUTPUT.
    "mv    x23, x0",         // Drive pin LOW.

    "li    x6, 0x100",
"20:", // wait loop for "low" settling - this can be made much shorter if needed
    // as it is, the sampling rate is about 236kHz, which is already quite high
    "addi  x6, x6, -1",
    "bne   x6, x0, 20b",

    "mv    x7,  x31",        // remember aclk time
    "mv    x25, x5",         // make it an input
    // now, the pull-up on the pin will slowly charge the capacitance on the pin. We wait
    // for the rising edge to be detected and that is our captouch interval
    "mv    x20, x0",         // wait for quantum: based on EXTCLK rising edge on the configured pin

    "mv    x8,  x31",        // remember aclk time

    // mask out core ID
    "li    x10, 0x3FFFFFFF",  // x10 is re-used in roll-over computation below
    "and   x7, x7, x10",
    "and   x8, x8, x10",

    // handle roll-over case: x7 is greater than x8 in the case of roll-over
    "bgtu  x7, x8, 30f",
    "sub   x9, x8, x7",      // total cycles is x8 - x7
    "j 40f",
"30:", // roll-over path
    "sub   x10, x10, x7",      // x10 now contains cycles from max versus start
    "add   x9, x10, x8",      // total cycles is x10 + x8 (masked)

"40:",
    "mv    x19, x9",         // report the delta-t
    // the line below keeps us from blocking on the FIFO being full - we steal our own entry
    // and cause the host to read the stale value in the FIFO.
    "mv    x0, x19",         // drain the FIFO entry - host will read the "stale" empty entry
    "j     10b"
);
