#[allow(unused)] // time::Duration is needed in send_read but the compiler doesn't realize it
use std::time::{Duration, Instant};

use arbitrary_int::{Number, u5};
use bao1x_api::bio::*;
use bao1x_api::bio_code;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use bao1x_hal::clocks::ClockOp;
use num_traits::cast::ToPrimitive;
use utralib::generated::utra::bio_bdma;
use xous_api_susres::api::Opcode as SusresOp;

mod protocol;
use protocol::*;
pub use protocol::{
    Address, Brightness, Command, Part207LedDriver, SpecCmdData, SpecialCommand101, SpecialCommand110,
    bitflags,
};

/*
 * DALI library
 *
 * Dali stands for Digitally Addressable Lighting Interface and is a standard for controlling building
 * lighting. The naming convention is a bit confusing:
 *
 *   Application controller   A bus master, used to control lighting: that's what this library is for
 *   Control gear             A device directly controlling lights, e.g. an LED driver
 *   Control device           An input device, e.g. a light sensor or a dimmer
 *
 * Entry points into the library:
 *   set_brightness               (address, brightness)
 *   send_command                 (address, command)
 *   query_command_value          (address, command) -> u8
 *   query_command_bool           (address, command) -> bool
 *   send_special_command         (special command, data)
 *   query_special_command_value  (special command, data) -> u8
 *   query_special_command_bool   (special command, data) -> bool
 *
 * With these possible arguments:
 *   address            enum Address
 *   brightness         struct Brightness
 *   command            enums Command, Part207LedDriver
 *   special command    enums SpecialCommand101, SpecialCommand110
 *   data               enum SpecialCommandData
 *
 * Return values (u8) can be decoded with the help of offsets in mod bitflags.
 *
 * Limitations of this library:
 *  1) It's not been tested with actual devices.
 *  2) Not all device types are implemented. To add a new device type, implement an enum with trait
 *     DataByte, equivalent to Extension207LedDriver.
 *  3) DALI-2 adds support for control devices such as sensors and dimmers, which are not supported.
 *  4) Only synchronous functions at this point.
 *
 * More information:
 *  https://en.wikipedia.org/wiki/Digital_Addressable_Lighting_Interface
 *  https://jared.geek.nz/2025/06/dali-lighting-protocol/
 *  https://github.com/sde1000/python-dali
 *  https://github.com/qqqlab/DALI-Lighting-Interface/tree/main/Documentation
 */

#[derive(Clone, Copy, Debug)]
pub enum DaliOpError {
    TxError,
    WrongDataByteForCommand,
}

#[derive(Clone, Copy, Debug)]
pub enum DaliSetupError {
    TxRxPinsIdentical,
    CantChangeFclk,
    InvalidCore,
    OutOfMemory,
    NoFreeMachines,
    ResourceInUse,
    InternalError,
    ResourceError,
}

impl From<BioError> for DaliSetupError {
    fn from(error: BioError) -> Self {
        match error {
            // possible returns from init_core
            BioError::InvalidCore => DaliSetupError::InvalidCore,
            BioError::Oom => DaliSetupError::OutOfMemory,
            BioError::NoFreeMachines => DaliSetupError::NoFreeMachines,
            BioError::ResourceInUse => DaliSetupError::ResourceInUse,
            // catch-all
            _ => DaliSetupError::InternalError,
        }
    }
}

impl From<ResourceError> for DaliSetupError {
    fn from(error: ResourceError) -> Self {
        match error {
            // adapted from ResourceError => BioError conversion
            // ResourceError::None should not get returned to any caller here
            ResourceError::InternalError => DaliSetupError::InternalError,
            _ => DaliSetupError::ResourceInUse,
        }
    }
}

#[derive(PartialEq)]
pub enum InvertTx {
    // A level shifter for interfacing with the bus may invert the signal (e.g. a transistor pulling the
    // line low on a high pin ) or not (e.g. an opto-isolator).
    True,
    False,
}

pub struct DaliConfig {
    pub rx_pin: u5,
    pub tx_pin: u5,
    pub io_mode: IoConfigMode,
    pub inverts: InvertTx,
}

#[allow(unused)] // never used, says the compiler
impl DaliConfig {
    pub fn new(rx_pin: u5, tx_pin: u5) -> Result<Self, DaliSetupError> {
        if rx_pin == tx_pin {
            return Err(DaliSetupError::TxRxPinsIdentical);
        }
        Ok(Self { rx_pin, tx_pin, io_mode: IoConfigMode::Overwrite, inverts: InvertTx::False })
    }
}

pub struct Dali {
    bio_ss: Bio,
    rx_pin: u5,
    tx_pin: u5,
    // a CoreHandle is a page alias for the underlying virtual memory, assigned to the calling process to
    // avoid syscalls on accessing that resource
    _rx_handle: CoreHandle,
    _tx_handle: CoreHandle,
    // a CoreCSR transforms the handle into a Rust object that can be shared and copied more information in
    // the Baochip Coder's guide, Ch. 2.
    rx: CoreCsr,
    tx: CoreCsr,
    // tracks the resources used by the object
    resource_grant: ResourceGrant,
}

impl Resources for Dali {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "Dali".to_string(),
            cores: vec![CoreRequirement::Any, CoreRequirement::Any],
            fifos: vec![Fifo::Fifo1, Fifo::Fifo2],
            static_pins: vec![],
            dynamic_pin_count: 2,
        }
    }
}

impl Drop for Dali {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_dynamic_pin(self.rx_pin.as_u8(), &Dali::resource_spec().claimer).unwrap();
        self.bio_ss.release_dynamic_pin(self.tx_pin.as_u8(), &Dali::resource_spec().claimer).unwrap();
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl Dali {
    pub fn new(config: DaliConfig) -> Result<Dali, DaliSetupError> {
        let rx_pin = config.rx_pin;
        let tx_pin = config.tx_pin;
        // Lower fclk speed to allow longer quantum intervals, any freqency below 157MHz works
        let requested_frequency = 140_000_000;
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns
            .request_connection_blocking(xous_api_susres::api::SERVER_NAME_SUSRES)
            .expect("Can't connect to Susres server");
        let result = xous::send_message(
            conn,
            xous::Message::new_blocking_scalar(
                SusresOp::PlatformSpecific.to_usize().unwrap(),
                ClockOp::SetFclk.to_usize().unwrap(),
                requested_frequency as usize,
                0,
                0,
            ),
        );
        match result {
            Ok(xous::Result::Scalar2(ok, _freq)) => {
                if ok == 0 {
                    return Err(DaliSetupError::CantChangeFclk);
                }
            }
            _ => return Err(DaliSetupError::CantChangeFclk),
        };

        let mut bio_ss = Bio::new();
        // claim resources
        let resource_grant = bio_ss.claim_resources(&Dali::resource_spec())?;
        log::debug!("granted to Dali: {:?}", resource_grant);
        // configure cores
        let config_rx =
            CoreConfig { clock_mode: bao1x_api::bio::ClockMode::ExternalPin(BioPin::new(rx_pin.as_u8())) };
        let config_tx = CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqFrac(2400) };
        let rx_kernel = dali_rx_kernel();
        let tx_kernel = dali_tx_kernel();
        bio_ss.init_core(resource_grant.cores[0], rx_kernel, config_rx)?;
        bio_ss.init_core(resource_grant.cores[1], tx_kernel, config_tx)?;
        // claim pins, configure pins, io
        bio_ss.claim_dynamic_pin(rx_pin.as_u8(), &Dali::resource_spec().claimer)?;
        bio_ss.claim_dynamic_pin(tx_pin.as_u8(), &Dali::resource_spec().claimer)?;
        let mut io_config = IoConfig::default();
        if config.inverts == InvertTx::True {
            io_config.o_inv = 1 << tx_pin.as_u32();
        }
        io_config.mapped = (1 << rx_pin.as_u32()) | (1 << tx_pin.as_u32());
        io_config.mode = config.io_mode;
        bio_ss.setup_io_config(io_config).unwrap();
        // final setup steps
        bio_ss.update_bio_freq(140_000_000);
        bio_ss.set_core_run_state(&resource_grant, true);

        // get memory ranges needed for register access
        // safety: tx and rx are wrapped in CSR objects whose lifetime matches that of the handles
        let rx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo1) }?.expect("Didn't get Fifo1 handle");
        let tx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo2) }?.expect("Didn't get Fifo2 handle");
        let mut rx = CoreCsr::from_handle(&rx_handle);
        let mut tx = CoreCsr::from_handle(&tx_handle);

        // push rx and tx pin location into the respective FIFOs
        rx.csr.wo(bio_bdma::SFR_TXF1, 1 << rx_pin.as_u32());
        tx.csr.wo(bio_bdma::SFR_TXF2, 1 << tx_pin.as_u32());

        // can cause conflicts w/ other BIO applications. let's put a reminder here
        log::debug!("Dali uses event register[1:0].");

        Ok(Self {
            bio_ss,
            rx_pin,
            tx_pin,
            _rx_handle: rx_handle,
            _tx_handle: tx_handle,
            rx,
            tx,
            resource_grant,
        })
    }

    // if a device answers unexpectedly, the reading and sending FIFOs will get out of sync,
    // leading to false tx errors
    fn empty_read_queue(&mut self) {
        // wait for line idle flag
        while self.rx.csr.rf(bio_bdma::SFR_EVENT_STATUS_SFR_EVENT_STATUS) & 1 == 0 {}
        while self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) != 0 {
            _ = self.rx.csr.r(bio_bdma::SFR_RXF1);
        }
    }

    fn send_sync(
        &mut self,
        address_byte: impl AddrByte,
        data_byte: impl DataByte,
    ) -> Result<(), DaliOpError> {
        self.empty_read_queue();
        let cmd = ForwardFrame::new(address_byte, data_byte).to_bits();
        log::debug!("sending: {:08b}_{:08b}", cmd >> 8, cmd & 0xff);
        while self.tx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2) != 0 {}
        self.tx.csr.wo(bio_bdma::SFR_TXF2, cmd as u32);
        while self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0 {}
        if self.rx.csr.r(bio_bdma::SFR_RXF1) as u16 == cmd {
            return Ok(());
        } else {
            return Err(DaliOpError::TxError);
        }
    }

    fn send_read(
        &mut self,
        address_byte: impl AddrByte,
        data_byte: impl DataByte,
    ) -> Result<BackwardFrame, DaliOpError> {
        let _ = self.send_sync(address_byte, data_byte)?;
        let now = Instant::now();
        // wait time
        //   (last bit transition   -> value pushed onto FIFO)                --
        //   value pushed onto FIFO -> end of stop bits                      1250us
        //   end of stop bits       -> max. var. delay for backward frames   9166us
        while self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0
            && now.elapsed().as_micros() < 10416
        {}
        // due to min. delay for backward frames and their length, it's unlikely a transmission finishes
        // before the end of this timeout
        let read = if self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) != 0 {
            // message received -> read
            self.rx.csr.r(bio_bdma::SFR_RXF1) & 0x0000_00ff
        } else if self.rx.csr.rf(bio_bdma::SFR_EVENT_STATUS_SFR_EVENT_STATUS) & 0b11 == 0 {
            // line busy, external source -> wait for message
            while self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0 {}
            self.rx.csr.r(bio_bdma::SFR_RXF1) & 0x0000_00ff
        } else {
            // line idle -> timeout
            // line busy, tx core is sending -> timeout
            0
        };
        if read == 0 {
            log::debug!("received: --timeout--");
        } else {
            log::debug!(
                "received: {:08b}_{:08b}_{:08b}_{:08b}",
                (read >> 24),
                (read >> 16) & 0xff,
                (read >> 8) & 0xff,
                read & 0xff
            );
        }
        return Ok(BackwardFrame(read as u8));
    }

    pub fn set_brightness(&mut self, address: Address, brightness: Brightness) -> Result<(), DaliOpError> {
        _ = self.send_sync(address, brightness)?;
        Ok(())
    }

    pub fn send_command(
        &mut self,
        address: Address,
        command: impl StdCommand + DataByte,
    ) -> Result<(), DaliOpError> {
        _ = self.send_sync(address, command)?;
        Ok(())
    }

    pub fn query_command_value(
        &mut self,
        address: Address,
        command: impl StdCommand + DataByte,
    ) -> Result<u8, DaliOpError> {
        let response = self.send_read(address, command)?;
        Ok(response.0)
    }

    pub fn query_command_bool(
        &mut self,
        address: Address,
        command: impl StdCommand + DataByte,
    ) -> Result<bool, DaliOpError> {
        let response = self.send_read(address, command)?;
        if response.0 != 0 { Ok(true) } else { Ok(false) }
    }

    pub fn send_special_command<Trait: SpecialCommand + AddrByte>(
        &mut self,
        command: Trait,
        data: SpecCmdData,
    ) -> Result<(), DaliOpError> {
        let data_byte = command.match_data_byte(data)?;
        _ = self.send_sync(command, data_byte)?;
        Ok(())
    }

    pub fn query_special_command_value<Trait: SpecialCommand + AddrByte>(
        &mut self,
        command: Trait,
        data: SpecCmdData,
    ) -> Result<u8, DaliOpError> {
        let data_byte = command.match_data_byte(data)?;
        let response = self.send_read(command, data_byte)?;
        Ok(response.0)
    }

    pub fn query_special_command_bool<Trait: SpecialCommand + AddrByte>(
        &mut self,
        command: Trait,
        data: SpecCmdData,
    ) -> Result<bool, DaliOpError> {
        let data_byte = command.match_data_byte(data)?;
        let response = self.send_read(command, data_byte)?;
        if response.0 != 0 { Ok(true) } else { Ok(false) }
    }
}

#[rustfmt::skip]
bio_code!(dali_rx_kernel, DALI_RX_START, DALI_RX_END,
/* DALI receiver
 *
 * The rx core serves two functions: It receives messages and it gates transmissions, enforcing radio silence
 * on an active line and maintaining appropriate delay between frames.
 *
 * The decoding algorithm:
 *  The core blocks on an idle line. The first bit always consists of a low pulse (a logical one), which
 *  triggers extclk and sets the decoder running. After an edge has occured, it waits for half a cycle,
 *  skipping over the transition between bits, which means all detected edges are meaningful and an absence
 *  of an edge signals the end of transmission.
 *  If an edge is detected, the appropritate bit is appended to the received-bit container. If no edge occurs
 *  for a full cycle, the transmission has stopped, the received value gets passed up and the receiver enfores
 *  a transmission delay. Backward frames can send earlier than forward frames, so line activity during that
 *  time restarts the receiving process without allowing transmission.
 *  The "1" in x15 is the mandatory starting bit of any transmission and shows how many bits were received
 *  (for example, if a transmission consists of only zeroes).
 *  Alternative decoder algorithms considered and not chosen:
 *    The standard allows 10% variation in timing, which makes sampling at a fixed frequency problematic,
 *    especially for longer frames.
 *    Measuring the first low-going pulse and sampling at that frequency would decode all legal transmissions
 *    with only extreme edge cases failing (e.g. an unstable clock or low pulses much longer/shorter than high
 *    pulses).
 *  In practice a simpler algorithm would probably be okay.
 *
 * Timing info:
 *  Shortest legal cycle: 833.33 - 10% = 750us
 *  Longest legal cycle: 833,33 + 10% = 916,7us
 *  Longest half cycle: ~460us
 *
 * Explanation of various delays:
 *  Startup delay
 *      Wait until the tx core has pulled the line high, otherwise that edge triggers extclk and an
 *      unnecessary value in FIFO
 *  50 us
 *      Sampling delay, not critical
 *  460us
 *      Half-cycle, skips over the transitions between bits, see explanation above
 *  917us
 *      Full-cycle, checks for end of transmission
 *  11,25ms
 *      Mandatory silence at the end of transmission:
 *      1/2 cycle (last edge --> end of last bit) + 2 stop bits + delay (9,167ms)
 *
 * Register usage
 *  x 1 delay counter
 *  x 2
 *  x 3 rx pin
 *  x 4
 *  x 5 old rx pin state
 *  x 6 new rx pin state
 *  x 7 bit value at LSB
 *  x 8
 *  x 9 mask for Core ID
 *  x10 timestamp of last edge
 *  x11 current timestamp
 *  x12 elapsed cycle count
 *  x13 920us as cycle count
 *  x14 11,25ms as cycle count
 *  x15 received bit word
 *
 * Flag usage
 *  Bit 0 line idle flag (1 -> idle)
 *  Bit 1 transmittiting flag (1 -> transmitting)
 */

    "mv    x3, x17",        // read pin # from FIFO1
    "mv    x26, x3",        // set GPIO mask
    "mv    x25, x3",        // set pin as input

    "li    x13, 0x1f57c",   // CPU cycles in 917us at 140MHz
    "li    x14, 0x17f4d0",  // CPU cycles in 11,25ms at 140MHz

    "li    x1, 0x189c",
"0:", // startup delay
    "addi  x1, x1, -1",
    "bnez   x1, 0b",

"10:", // wait for line activity
    "li    x28, 1",         // set line idle flag
"11:",
    "li    x15, 1",         // reset old received value register
    "mv    x20, x0",        // wait for line activity
"12:", // line active
    "li    x29, 1",         // clear line idle flag

"20:", // wait for half a cycle
    "mv    x10, x31",       // timestamp edge
    "li    x1, 0x189c",     // set counter for half-cycle delay (~470us)
"21:",
    "addi  x1, x1, -1",     // count down
    "bnez  x1, 21b",        // keep counting down until x1 == 0

"30:", // start sampling
    "and   x5, x21, x3",    // read rx pin state into x5
    "li    x1, 630",        // set counter for sampling delay (~50us)
"31:",
    "addi  x1, x1, -1",     // count down
    "bnez  x1, 31b",        // keep counting down until x1 == 0

"35:", // check for elapsed time > 1 cycle
    "mv    x11, x31",       // take current timestamp
    "li    x9, 0x3FFFFFFF", // mask out Core ID
    "and   x10, x10, x9",
    "and   x11, x11, x9",
    "bgtu  x10, x11, 36f",  // check if timestamp has rolled over

    "sub   x12, x11, x10",  // x12 contains elapsed cycles
    "j 37f",
"36:",
    "sub   x12, x10, x9",   // cycles before roll-over
    "add   x12, x12, x11",  // add cycles after roll-over
"37:",
    "bgeu  x12, x13, 50f",  // if elapsed time > 1 cycle, go to end-of-transmission

"39:", // take new sample and compare
    "and   x6, x21, x3",    // read new pin value
    "beq   x5, x6, 30b",    // if nothing changed, sample again

"40:", // found an edge!
    "snez  x7, x6",         // shifts bit into LSB position regardless of pin #
                            // logical 1 if pin is high, 0 if pin is low
    "slli  x15, x15, 1",    // left-shift bit container
    "or    x15, x15, x7",   // add new bit at the end of bit container
    "j 20b",                // wait for next bit

"50:", // end of transmission
    "and   x5, x21, x3",    // read pin value: at this point, the line idles high
                            // needed to check for line activity during tx delay
    "mv    x17, x15",       // push received value onto FIFO

"60:", // transmission delay
    "mv    x11, x31",       // take current timestamp
    "li    x9, 0x3FFFFFFF", // mask out Core ID
    "and   x10, x10, x9",
    "and   x11, x11, x9",
    "bgtu  x10, x11, 66f",  // check if timestamp has rolled over

    "sub   x12, x11, x10",  // x12 contains elapsed cycles
    "j 67f",
"66:",
    "sub   x12, x10, x9",   // cycles before roll-over
    "add   x12, x12, x11",  // add cycles after roll-over
"67:",
    "and   x6, x21, x3",    // read new pin value
    "bne   x5, x6, 11b",    // line activity -> receive, don't set line idle flag
    "bgeu  x12, x14, 10b",  // elapsed time > delay: go idle
    "j 60b"                 // else keep delaying

);

#[rustfmt::skip]
bio_code!(dali_tx_kernel, DALI_TX_START, DALI_TX_END,
/* DALI transmitter
 *
 * Dali uses a 1200 baud manchester encoding, with low -> high as logical 1. Data comes in through FIFO1 to be
 * transmitted MSB -> LSB.
 *
 * The line idles high. The first bit is always a 1, followed by 16 data bits, followed by two stop bits. The
 * stop bits are not manchester encoded but simply idle the line high. The rx core enforces tx silence with an
 * active line as well as a delay between messages as per specs. Since the stop bits are indistinguishable
 * from idling, they're enforced by the rx core, too.
 *
 * Register usage
 *   x1 data
 *   x2 tx bit value
 *   x3
 *   x4
 *   x5 tx pin
 *   x6 data mask
 *   x7 shift counter
 *
 * Flag usage
 *   Bit 0 line idle flag (1 -> idle)
 *   Bit 1 transmittiting flag (1 -> transmitting)
 */

    "mv    x5, x18",        // read first argument from FIFO2, which is the tx pin number
    "mv    x26, x5",        // set GPIO mask
    "mv    x24, x5",        // set tx pin as output
    "li    x27, 0x1",       // set event sensitivity mask: this bit is the line-idle flag

    // how many bits do we need to shift over?
    "li    x6, 0x80000000", // initialize indicator bit
    "li    x7, 0x0",        // initialize counter
"10:",
    "beq   x6, x5, 11f",    // if indicator and pin match, we found our number
    "srli  x6, x6, 1",      // shift indicator one over
    "addi  x7, x7, 1",      // increase counter
    "j 10b",
"11:",
    "li    x6, 0x80000000", // initialize mask

"20:", // transmit loop
    "li    x29, 2",         // clear transmission flag
    "mv    x22, x5",        // set tx pin to high, because the line idles high
    "mv    x1, x18",        // read data from FIFO2, block on empty
    "slli  x1, x1, 16",     // shift data all the way to the left
    "mv    x0, x30",        // wait for line-idle flag
    "li    x28, 2",         // set tranmission flag
    "mv    x20, x0",        // snap to quantum
    // bit 1, always a logical 1
    "mv    x23, x0",        // pin low
    "mv    x20, x0",        // snap to quantum
    "mv    x22, x5",        // pin high
    "mv    x20, x0",        // snap to quantum
    // bit 2
    "and   x2, x6, x1",     // mask off bit for tx
    "srl   x2, x2, x7",     // shift data to position of tx pin
    "not   x23, x2",        // pin low for first half if data == 1
    "not   x22, x2",        // pin high for first half if data == 0
    "mv    x20, x0",        // snap to quantum
    "mv    x22, x2",        // pin high for second half if data == 1
    "mv    x23, x2",        // pin low for second half if data == 0
    "mv    x20, x0",        // snap to quantum
    // bit 3
    "slli  x1, x1, 1",      // shift data to the left for the next bit
    "and   x2, x6, x1",     // repeat for remaining bits
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 4
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 5
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 6
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 7
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 8
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 9
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 10
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 11
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 12
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 13
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 14
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 15
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 16
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // bit 17
    "slli  x1, x1, 1",
    "and   x2, x6, x1",
    "srl   x2, x2, x7",
    "xor   x23, x5, x2",
    "xor   x22, x5, x2",
    "mv    x20, x0",
    "and   x22, x5, x2",
    "and   x23, x5, x2",
    "mv    x20, x0",
    // two stop bits
    // not manchester-encoded, see explanation at the beginning

"j 20b"     // repeat

);
