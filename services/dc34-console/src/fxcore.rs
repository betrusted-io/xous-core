use arbitrary_int::{Number, u5};
use bao1x_api::bio::*;
use bao1x_api::bio_code;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use utralib::utra::bio_bdma;

pub struct FxCore {
    bio_ss: Bio,
    // handles have to be kept around or else the underlying CSR is dropped
    _tx_handle: CoreHandle,
    // the CoreCsr is a convenience object that manages the CSR view of the handle
    tx: CoreCsr,
    // tracks the resources used by the object
    resource_grant: ResourceGrant,
}

impl Resources for FxCore {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "Ws2812".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo3],
            static_pins: vec![],
            dynamic_pin_count: 0,
        }
    }
}

impl Drop for FxCore {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl FxCore {
    pub fn new() -> Result<FxCore, BioError> {
        let mut bio_ss = Bio::new();
        // claim core resource and initialize it
        let resource_grant = bio_ss.claim_resources(&FxCore::resource_spec())?;
        // 20khz to simplify wait looping - 10.7khz is the lowest we can go with a 700MHz fclk
        let config = CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqInt(20_000) };
        log::debug!("grant: {:?}", resource_grant);
        bio_ss.init_core(resource_grant.cores[0], fx_core(), config)?;
        bio_ss.set_core_run_state(&resource_grant, true);

        // safety: fifo3 is stored in the FxCore object so it isn't Drop'd before the object is
        // destroyed
        let tx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo3) }?.expect("Didn't get FIFO3 handle");

        let mut tx = CoreCsr::from_handle(&tx_handle);

        Ok(Self {
            bio_ss,
            tx,
            // safety: tx and rx are wrapped in CSR objects whose lifetime matches that of the handles
            _tx_handle: tx_handle,
            resource_grant,
        })
    }
}

#[rustfmt::skip]
bio_code!(fx_core, FX_CORE_START, FX_CORE_END,
    "li x15, 0x0D0000",  // green LED, mid brightness
    "li x1, 0xF00", // LED buffer, stuck at top of memory
    "li x3, 0", // initial location of lit LED
    "li x2, 32", // length of LED "strip"
    "li x4, 0x1000000", // transmission mask
    "li x5, 2500",  // delay between transmissions

"99:", // top of main loop
    // zero out the LED buffer
    "add x14, x1, x2", // x14 is temp loop counter
"1:",
    "addi x14, x14, -4", // decrement loop counter
    "sw x0, 0(x14)", // store zero
    "bgt x14, x1, 1b", // go back if x14 hasn't hit LED buffer beginning

    // light one LED
    "add x14, x3, x1", // compute address for lit LED
    "add x10, x15, x3", // make intensity depend on location
    "sw x10, 0(x14)",  // put it in the location
    "addi x3, x3, 4",  // increment the location
    "blt x3, x2, 9f",  // skip zeroing if we haven't hit the end of the strip
    "mv x3, x0",       // zero x3 if we're equal to x2 (length of strip)

"9:",
    // transmit data to the LED renderer
    "add x14, x1, x2", // x14 is temp loop counter
"10:",
    "addi x14, x14, -4", // decrement loop counter
    "lw x13, 0(x14)",  // fetch the LED value
    "beq x14, x1, 20f",  // 20: has the code for handling the final LED
    "mv x17, x13",    // put it in FIFO1
    "nop",
    "nop",
    "nop",
    "j 10b",          // move more data
"20:",                // final LED needs the transmission flag
    "or x13, x4, x13", // OR in the transmission flag
    "mv x17, x13",    // put it in FIFO1
    // fall through to the wait loop

    "mv x14, x0",   // zero the loop counter
"30:",
    "addi x14, x14, 1",
    "mv x20, x0",   // wait for quantum @ 20khz
    "bne x14, x5, 30b", // go back until we hit the transmission delay time

    "mv x0, x18",   // empty the transmission token

    "j 99b"   // go back to the top of the loop and do it all over again
);
