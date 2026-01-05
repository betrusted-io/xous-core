use arbitrary_int::{Number, u5};
use bao1x_api::bio::*;
use bao1x_api::bio_code;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use utralib::utra::bio_bdma;

pub enum LedVariant {
    B,
    C,
}

pub struct Ws2812 {
    bio_ss: Bio,
    bio_pin: u5,
    // handles have to be kept around or else the underlying CSR is dropped
    _tx_handle: CoreHandle,
    _rx_handle: CoreHandle,
    // the CoreCsr is a convenience object that manages the CSR view of the handle
    tx: CoreCsr,
    rx: CoreCsr,
    // tracks the resources used by the object
    resource_grant: ResourceGrant,
}

impl Resources for Ws2812 {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "Ws2812".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo1, Fifo::Fifo2],
            static_pins: vec![],
            dynamic_pin_count: 1,
        }
    }
}

impl Drop for Ws2812 {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_dynamic_pin(self.bio_pin.as_u8(), &Ws2812::resource_spec().claimer).unwrap();
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl Ws2812 {
    pub fn new(variant: LedVariant, bio_pin: u5, io_mode: Option<IoConfigMode>) -> Result<Ws2812, BioError> {
        let mut bio_ss = Bio::new();
        // claim core resource and initialize it
        let resource_grant = bio_ss.claim_resources(&Ws2812::resource_spec())?;
        let config = CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqInt(6_666_667) };
        let kernel = match variant {
            LedVariant::B => ws2812b_kernel(),
            LedVariant::C => ws2812c_kernel(),
        };
        log::debug!("grant: {:?}", resource_grant);
        bio_ss.init_core(resource_grant.cores[0], &kernel, 0, config)?;
        bio_ss.set_core_run_state(&resource_grant, true);

        // claim pin resource - this only claims the resource, it does not configure it
        bio_ss.claim_dynamic_pin(bio_pin.as_u8(), &Ws2812::resource_spec().claimer)?;
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

        // safety: fifo1 and fifo2 are stored in the Ws2812 object so they aren't Drop'd before the object is
        // destroyed
        let tx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo1) }?.expect("Didn't get FIFO1 handle");
        let rx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo2) }?.expect("Didn't get FIFO2 handle");

        let mut tx = CoreCsr::from_handle(&tx_handle);
        tx.csr.wo(bio_bdma::SFR_TXF1, io_config.mapped);

        Ok(Self {
            bio_ss,
            bio_pin,
            tx,
            rx: CoreCsr::from_handle(&rx_handle),
            // safety: tx and rx are wrapped in CSR objects whose lifetime matches that of the handles
            _tx_handle: tx_handle,
            _rx_handle: rx_handle,
            resource_grant,
        })
    }

    /// Sends the data down the strip, but doesn't wait for the send to finish before returning.
    ///
    /// Useful for implementations that want to do something else during the time that the LED
    /// strip is being updated.
    pub fn send_async(&mut self, strip: &[u32]) {
        if let Some((&last, elements)) = strip.split_last() {
            for &led in elements.iter() {
                self.tx.csr.wo(bio_bdma::SFR_TXF1, led);
            }
            self.tx.csr.wo(bio_bdma::SFR_TXF1, last | 0x1_00_00_00);
        }
    }

    /// This ensures the previous send is finished. If the system does not do this before calling
    /// `send_async` again, it risks losing data because the incoming FIFO has overflowed.
    pub fn send_await(&self) {
        while self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2) == 0 {
            xous::yield_slice();
        }
        let _result = self.rx.csr.r(bio_bdma::SFR_RXF2); // empty the token
    }

    /// A routine that emulates a synchronous behavior by simply chaining async/await.
    pub fn send(&mut self, strip: &[u32]) {
        self.send_async(strip);
        self.send_await();
    }
}

/// Helper function to pack RGB values into u32s
pub fn rgb_to_u32(r: u8, g: u8, b: u8) -> u32 { ((g as u32) << 16) | ((r as u32) << 8) | (b as u32) }

// WS2812B kernel
//
// FIFO1 - data input to send
// FIFO2 - transmit done token
//
// Data is sent in via FIFO1.
// They *very first* data transmitted on initialization is the mask that represents which I/O
// to drive the signal onto.
//
// Thereafter, the first piece of data sent in is the value for the last LED in the chain.
// Data has the following format:
// bit 24     - 0 means more data. 1 means transmit all previously sent values.
// bits 23:16 - g[7:0]
// bits 15:8  - r[7:0]
// bits 7:0   - b[7:0]
//
// Upon transmission, the buffer is cleared and the data is built up again from the last LED in the chain.
// Expects the quantum clock to be set to a 150ns period (6.66666..7 MHz)
//
// Example of transmission on a 3-long LED strip, wired as follows:
//   Baochip-1x BIO[5] -> LED2 -> LED1 -> LED0
//
//   0x20 -> FIFO1          ; sets pinmask for BIO[5]
//   0x0_00_00_ff -> FIFO1  ; last LED is blue
//   0x0_00_ff_00 -> FIFO1  ; middle LED is red
//   0x1_ff_00_00 -> FIFO1  ; first LED is green + commit bit set
//   [data transmission happens, LEDs now get the colors]
//   FIFO2 -> token         ; CPU waits for FIFO2 to get more than 0 elements, and drains the transmit token
//
//   next iteration would start with the last LED color; the pinmask should *not* be transmitted again.
//
// The transmit token is merely the current x31 register value (clock elapsed + core ID register)
#[rustfmt::skip]
bio_code!(ws2812b_kernel, WS2812B_START, WS2812B_END,
    "mv x4, x17", // read from FIFO1 - the first argument is the GPIO pin mask we're using to transmit. stash this in x4
    // "mv x18, x4", // debug by looping back -----------
    "mv x26, x4", // apply mask to all GPIO operations
    // setup the pin as an output
    "mv x24, x4",
    // zero the output
    "mv x23, x0",

    // LEDs will go onto the stack
    "li x9, 0x1000000", // bit 24 mask
    "li sp, 0x800", // start of the LED buffer
    "10:",
    // read a 24-bit color number from FIFO1
    "mv x8, x17",
    "sw x8, 0(sp)",
    "addi sp, sp, 4", // stack builds *UP*, away from the code
    "and x10, x9, x8", // AND incoming word with bit 24 mask
    "bne x10, x0, 20f", // jump to the routine at 20f to send if x10 is not 0
    "j 10b", // go back and get more data

    // -- sending loop --
    "20:",
    "li x8, 0x800", // x8 now has the starting address of the data we're going to send
    // x9 is the bit 24 mask used by the above loop
    "li x12, 0x800000", // bit 23 mask
    // loop setup done
    "30:",
    "li x11, 24", // number of bits to shift through
    "lw x10, 0(x8)", // fetch the word to send
    // "mv x18, x10", // debug by looping back -----------
    "31:",
    "and x13, x12, x10", // the bit we're contemplating is at bit 23 position, extract it into x13
    "bne x13, x0, 40f", // if 1, go to 40f, where the 1 routine is

    // zero routine
    // 2 hi
    "mv x22, x4", // sets to 1
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    // 7 lo
    "mv x23, x0", // sets to 0 because we set the GPIO mask earlier
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    // do work during the quantum - zero path
    "slli x10, x10, 1", // shift the pixel value
    "addi x11, x11, -1", // decrement the pixel bit counter
    "mv x20, x0",
    "j 50f", // jump to the loop end check

    // one routine
    "40:",
    // 7 hi
    "mv x22, x4", // sets to 1
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    // 2 lo
    "mv x23, x0", // sets to 0 because we set the GPIO mask earlier
    "mv x20, x0", // wait for quantum
    // do work during the quantum - one path
    "slli x10, x10, 1", // shift the pixel value
    "addi x11, x11, -1", // decrement the pixel bit counter
    "mv x20, x0",

    "50:", // shift and do the next pixel value
    "bne x11, x0, 31b", // go back for more in the loop

    "60:", // check if we've exhausted all the LED values
    "addi x8, x8, 4",
    "bge sp, x8, 30b", // see if we've hit the value of current sp
    "li sp, 0x800", // reset the stack pointer for a fresh fetch
    // wait to reset the chain
    "li x14, 2000", // delay wait time to reset the chain per spec
    "70:",
    "addi x14, x14, -1",
    "mv x20, x0",
    "bne x14, x0, 70b",

    "mv x18, x31", // put a token in to synchronize and indicate that the loop is done

    "j 10b" // go back and get more data
);

// Identical to the ws2812b kernel except the inner loop timings are tweaked for the ws2812c variant.
// The same clock configuration is used for this kernel.
#[rustfmt::skip]
bio_code!(ws2812c_kernel, WS2812C_START, WS2812C_END,
    "mv x4, x17", // read from FIFO1 - the first argument is the GPIO pin mask we're using to transmit. stash this in x4
    // "mv x18, x4", // debug by looping back -----------
    "mv x26, x4", // apply mask to all GPIO operations
    // setup the pin as an output
    "mv x24, x4",
    // zero the output
    "mv x23, x0",

    // LEDs will go onto the stack
    "li x9, 0x1000000", // bit 24 mask
    "li sp, 0x800", // start of the LED buffer
    "10:",
    // read a 24-bit color number from FIFO1
    "mv x8, x17",
    "sw x8, 0(sp)",
    "addi sp, sp, 4", // stack builds *UP*, away from the code
    "and x10, x9, x8", // AND incoming word with bit 24 mask
    "bne x10, x0, 20f", // jump to the routine at 20f to send if x10 is not 0
    "j 10b", // go back and get more data

    // -- sending loop --
    "20:",
    "li x8, 0x800", // x8 now has the starting address of the data we're going to send
    // x9 is the bit 24 mask used by the above loop
    "li x12, 0x800000", // bit 23 mask
    // loop setup done
    "30:",
    "li x11, 24", // number of bits to shift through
    "lw x10, 0(x8)", // fetch the word to send
    // "mv x18, x10", // debug by looping back -----------
    "31:",
    "and x13, x12, x10", // the bit we're contemplating is at bit 23 position, extract it into x13
    "bne x13, x0, 40f", // if 1, go to 40f, where the 1 routine is

    // zero routine
    // 2 hi
    "mv x22, x4", // sets to 1
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    // 5 lo
    "mv x23, x0", // sets to 0 because we set the GPIO mask earlier
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    // do work during the quantum - zero path
    "slli x10, x10, 1", // shift the pixel value
    "addi x11, x11, -1", // decrement the pixel bit counter
    "mv x20, x0",
    "j 50f", // jump to the loop end check

    // one routine
    "40:",
    // 5 hi
    "mv x22, x4", // sets to 1
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    // 5 lo
    "mv x23, x0", // sets to 0 because we set the GPIO mask earlier
    "mv x20, x0", // wait for quantum
    "mv x20, x0",
    "mv x20, x0",
    "mv x20, x0",
    // do work during the quantum - one path
    "slli x10, x10, 1", // shift the pixel value
    "addi x11, x11, -1", // decrement the pixel bit counter
    "mv x20, x0",

    "50:", // shift and do the next pixel value
    "bne x11, x0, 31b", // go back for more in the loop

    "60:", // check if we've exhausted all the LED values
    "addi x8, x8, 4",
    "bge sp, x8, 30b", // see if we've hit the value of current sp
    "li sp, 0x800", // reset the stack pointer for a fresh fetch
    // wait to reset the chain
    "li x14, 2000", // delay wait time to reset the chain per spec
    "70:",
    "addi x14, x14, -1",
    "mv x20, x0",
    "bne x14, x0, 70b",

    "mv x18, x31", // put a token in to synchronize and indicate that the loop is done

    "j 10b" // go back and get more data
);
