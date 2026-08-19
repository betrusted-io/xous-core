use arbitrary_int::{Number, u5};
use bao1x_api::bio::*;
use bao1x_api::bio_code;
use bao1x_api::bio_resources::*;
use bao1x_api::{IoSetup, IoxDir, IoxEnable};
use bao1x_hal::bio::{Bio, CoreCsr};
use utralib::utra::bio_bdma;


// Send a reset on the bus
static CMD_RESET: u32 = 0;
// Write a byte on the bus
static CMD_WRITE: u32 = 1;
// Read n bytes from the bus (passed via parameter)
static CMD_READ: u32 = 2;
// Write a single bit on the bus
static CMD_WRITE_BIT: u32 = 3;
// Wait n µs (Implemented on the BIO just for fun)
static CMD_WAIT: u32 = 4;
// For debugging: Pull bus to LO
#[allow(dead_code)]
static CMD_LO: u32 = 5;
// For debugging: Set bus to input
#[allow(dead_code)]
static CMD_RELEASE: u32 = 6;

// Indicate the following host command shall be interpreted
// by all bus devices
static YW_SKIPROM: u32 = 0xCC;
// Indicate the next eight bytes will specify
// which device should consider itself addressed
// (followed by a command such as READ_SCRATCHPAD)
static YW_ROMADDR: u32 = 0x55;
// Store the last measured temperature into the scratchpad
// memory area
static YW_CONVERT_TEMPERATURE: u32 = 0x44;
// Read out the scratchpad memory and send it over
// the bus to the host
static YW_READ_SCRATCHPAD: u32 = 0xBE;
// Search ROM: Find addresses of all connected devices,
// one by one
static YW_SEARCH_ROM: u32 = 0xF0;


pub struct YiWire {
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

impl Resources for YiWire {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "YiWire".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo2, Fifo::Fifo3],
            static_pins: vec![],
            dynamic_pin_count: 1,
        }
    }
}

impl Drop for YiWire {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_dynamic_pin(self.bio_pin.as_u8(), &YiWire::resource_spec().claimer).unwrap();
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl YiWire {
    pub fn new(bio_pin: u5, io_mode: Option<IoConfigMode>) -> Result<YiWire, BioError> {
        let iox = bao1x_api::iox::IoxHal::new();
        let mut bio_ss = Bio::new();
        // claim core resource and initialize it
        let resource_grant = bio_ss.claim_resources(&YiWire::resource_spec())?;
        let config = CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqInt(1_000_000) };
        bio_ss.init_core(resource_grant.cores[0], yiwire_kernel(), config)?;
        bio_ss.set_core_run_state(&resource_grant, true);

        // claim pin resource - this only claims the resource, it does not configure it
        bio_ss.claim_dynamic_pin(bio_pin.as_u8(), &YiWire::resource_spec().claimer)?;

        let port_config = bao1x_api::bio::bio_bit_to_port_and_pin(bio_pin);
        iox.setup_pin(
            port_config.0,
            port_config.1,
            Some(IoxDir::Input), // Port is input by default, we dynamically alternate between in/out later
            Some(bao1x_api::IoxFunction::Gpio),
            Some(IoxEnable::Enable),
            Some(IoxEnable::Enable),
            None,
            None,
        );

        // now configure the claimed resource
        let mut io_config = IoConfig::default();
        io_config.mapped = 1 << bio_pin.as_u32();
        io_config.mode = io_mode.unwrap_or(IoConfigMode::Overwrite);
        bio_ss.setup_io_config(io_config).unwrap();

        let tx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo2) }?.expect("Didn't get FIFO2 handle");
        let rx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo3) }?.expect("Didn't get FIFO3 handle");

        let mut tx = CoreCsr::from_handle(&tx_handle);
        let rx =  CoreCsr::from_handle(&rx_handle);
        tx.csr.wo(bio_bdma::SFR_TXF2, io_config.mapped);

        Ok(Self {
            bio_ss,
            bio_pin,
            tx,
            rx,
            // safety: tx and rx are wrapped in CSR objects whose lifetime matches that of the handles
            _tx_handle: tx_handle,
            _rx_handle: rx_handle,
            resource_grant,
        })
    }

    pub fn enumerate(&mut self) -> Result<Vec<[u8; 8]>, BioError> {
        let mut discrepancy_at = 0;
        let mut last_bitmask = [false; 64];
        let mut addrs = Vec::new();
        loop {
            let cmd_reset = self.cmd(CMD_RESET, 0x0);
            if cmd_reset == 0 {
                let mut bitmask = [false; 64];
                let mut addr = [0u8; 8];
                self.cmd(CMD_WRITE, YW_SEARCH_ROM);
                for bit in 0 .. 64 {
                    let res = self.cmd(CMD_READ, 0x02);
                    let id_bit = (res & 0x01) != 0;
                    let cmp_bit = (res & 0x02) != 0;
                    if id_bit && cmp_bit {
                        // error
                        // TODO: return error
                        return Ok(Vec::new());
                    }
                    if id_bit != cmp_bit {
                        bitmask[bit] = id_bit;
                        addr[bit >> 3] |= (if id_bit {1} else {0}) << (bit & 0x07);
                        self.cmd(CMD_WRITE_BIT, if id_bit { 1 } else { 0 });
                    } else {
                        let id_bit = if bit < discrepancy_at {
                            last_bitmask[bit]
                        } else {
                            bit == discrepancy_at
                        };
                        bitmask[bit] = id_bit;
                        addr[bit >> 3] |= (if id_bit {1} else {0}) << (bit & 0x07);
                        self.cmd(CMD_WRITE_BIT, if id_bit { 1 } else { 0 });
                        if !id_bit {
                            discrepancy_at = bit;
                        }
                    }
                }
                if last_bitmask == bitmask{
                    // Found all devices
                    break;
                }
                let computed_crc = crc8(&addr[..7]);
                let transmitted_crc = addr[7];
                if computed_crc == transmitted_crc {
                    addrs.push(addr);
                } else {
                    log::warn!("Received device address with incorrect CRC got {computed_crc}, expected {transmitted_crc}");
                }
                last_bitmask = bitmask;
            }
        }
        Ok(addrs)
    }

    pub fn measure_temperature(&mut self) -> Result<bool, BioError> {
        let cmd_reset = self.cmd(CMD_RESET, 0x0);
        if cmd_reset == 0 {
            // Send command to measure the temperature and
            // write it to the sensor's memory
            self.cmd(CMD_WRITE, YW_SKIPROM);
            self.cmd(CMD_WRITE, YW_CONVERT_TEMPERATURE);

            // Wait 750µs to allow the memory to be written
            // Use the BIO for this for fun
            self.cmd(CMD_WAIT, 750);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn get_temperature(&mut self, addr: &[u8]) -> Result<Option<f32>, BioError> {
        let cmd_reset = self.cmd(CMD_RESET, 0x0);
        if cmd_reset == 0 {
            self.cmd(CMD_WRITE, YW_ROMADDR);
            for addr_byte in addr {
                self.cmd(CMD_WRITE, (*addr_byte) as u32);
            }
            self.cmd(CMD_WRITE, YW_READ_SCRATCHPAD);

            let b1 = self.cmd(CMD_READ, 0x20);
            let b2 = self.cmd(CMD_READ, 0x20);
            let transmitted_crc = self.cmd(CMD_READ, 0x08) as u8;
            // This array is built so a CRC can be computed
            let arry = [  (b1 & 0xFF) as u8,
                             ((b1 >> 8) & 0xFF) as u8,
                             ((b1 >> 16) & 0xFF) as u8,
                             ((b1 >> 24) & 0xFF) as u8,
                              (b2 & 0xFF) as u8,
                             ((b2 >> 8) & 0xFF) as u8,
                             ((b2 >> 16) & 0xFF) as u8,
                             ((b2 >> 24) & 0xFF) as u8 ];
            let computed_crc = crc8(&arry);

            if transmitted_crc == computed_crc {
                let temp_raw = (b1 & 0xFFFF) as i16;
                let temp: f32 = (temp_raw as f32) / 16.0;
                Ok(Some(temp))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    fn cmd(&mut self, command: u32, param: u32) -> u32 {
        let whole: u32 = command + (param << 3);
        self.tx.csr.wo(bio_bdma::SFR_TXF2, whole);
        while self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3) == 0 {}
        self.rx.csr.r(bio_bdma::SFR_RXF3)
    }
}


// helpers
fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0x00;
    let reflected_poly: u8 = 0x8C;

    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            if (crc & 0x01) != 0 {
                crc = (crc >> 1) ^ reflected_poly;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}



// yiwire -- ws2812, adapted
//
// FIFO1 - data input to send
// FIFO2 - transmit done token
//
// Data is sent in via FIFO1.
// They *very first* data transmitted on initialization is the mask that represents which I/O
// to drive the signal onto.
//

// Our registers:
// x0: always 0
// x1: The BITMASK for our BIO PIN
// x2: Combined command+parameters
// x3: Command (3 bit)
// x4: various local usage
// x5: waiting time (number of quantums)
// x6: return register
// x7: bit counter
// x8: Parameters for command/return value
// x18: FiFo from the host
// x19: FiFo to the host
#[rustfmt::skip]
bio_code!(yiwire_kernel, YIWIRE_START, YIWIRE_END,
    "mv x1, x18",           // read from FIFO2 - the first argument is the GPIO pin mask we're using to transmit. stash this in x4
    "mv x26, x1",           // apply mask to all GPIO operations
    "li x0, 0",             // x0 should always be 0
    "mv x25, x1",           // Set our BIO pin to input

"10:",
    "mv x2, x18",           // read command from FIFO2
    "andi x3, x2, 0x07",    // The command is in the lower 3 bits
    "srli x8, x2, 3",       // The parameters are in the NFBs (next few bits)

    "li x4, 0",             // 0: RESET
    "beq x3, x4, 20f",

    "li x4, 1",             // 1: WRITE BYTE
    "beq x3, x4, 30f",

    "li x4, 2",             // 2: READ BYTE
    "beq x3, x4, 40f",

    "li x4, 3",             // 1: WRITE BIT
    "beq x3, x4, 50f",

    "li x4, 4",             // 4: WAIT
    "beq x3, x4, 60f",

    "li x4, 5",             // 5: LO
    "beq x3, x4, 70f",

    "li x4, 6",             // 6: RELEASE
    "beq x3, x4, 80f",

    "li x19, 0xFE",         // Return ERROR
    "j 10b",                // Loop to read next command

"20:",
    // Signal a RESET by pulling the PIN to LO for 480µs
    "mv x23, x0",           // Set our BIO pin to LO
    "mv x24, x1",           // Set our BIO pin to output
    "li x5, 480",           // Wait for 480µs
    "jal x6, 1000f",        // Call waiting routine
    "mv x25, x1",           // Release the pull to LO
    "li x5, 70",            // Wait for 70µs
    "jal x6, 1000f",
    "mv x4, x21",           // Read PIN to see presence
    "and x4, x4, x1",
    "mv x19, x4",           // Write the presence to the FIFO
    "li x5, 410",           // Complete the 2nd 480µs cycle
    "jal x6, 1000f",

    "j 10b",                // Loop to read next command

"30:",
    "li x7, 8",             // 8 bits to send

"35:",
    "mv x23, x0",           // Set our BIO pin to LO
    "mv x24, x1",           // Set our BIO pin to output

    "andi x4, x8, 1",       // Get lowermost bit
    "srli x8, x8, 1",       // Shift our input byte one to the right to get the next bit later
    "beq x4, x0, 36f",      // If lowermost bit is zero, send zero

    // By this point we send a one
    "li x5, 6",            // Delay 10µs
    "jal x6, 1000f",
    "mv x25, x1",           // Release the pull to LO
    "li x5, 64",            // Delay 55µs
    "jal x6, 1000f",
    "j 37f",

"36:",
    // By this point we send a zero
    "li x5, 60",            // Delay 65µs
    "jal x6, 1000f",
    "mv x25, x1",           // Release the pull to LO
    "li x5, 10",             // Delay 5µs
    "jal x6, 1000f",

"37:",
    "addi x7, x7, -1",      // Count down bit counter
    "bne x7, x0, 35b",      // All bits sent? acknowledge
    "li x19, 1",            // Send an acknowledgement 1 to FIFO
    "mv x20, x0",           // Write to x20 to delay for one quantum
    "j 10b",                // Receive next command

"40:",
    "li x7, 0",             // Number of bits read
    "mv x9, x8",            // Save number of bits to read
    "li x8, 0",             // Resulting byte

"41:",
    "mv x23, x0",           // Set our BIO pin to LO
    "mv x24, x1",           // Set our BIO pin to output
    "li x5, 6",             // Delay 3µs
    "jal x6, 1000f",
    "mv x25, x1",           // Release the pull to LO
    "li x5, 8",             // Delay 10µs
    "jal x6, 1000f",
    "mv x4, x21",           // Read PIN
    "li x5, 55",            // Delay 50µs to complete the cycle
    "jal x6, 1000f",
    "and x4, x4, x1",       // Pick our PIN with our bitmask
    "snez x4, x4",          // Make it zero or one
    "sll x4, x4, x7",       // Shifty shifting for bit fiddling
    "or x8, x8, x4",        // Add bit to byte
    "addi x7, x7, 1",       // Count to next bit
    "bne x7, x9, 41b",      // Bits left? Jump up

    "mv x19, x8",           // All bits received, send resulting byte to FIFO
    "j 10b",                // Await next command

"50:",
    "mv x23, x0",           // Set our BIO pin to LO
    "mv x24, x1",           // Set our BIO pin to output

    "beq x8, x0, 51f",       // Check if we should write a zero bit

    // Write a one bit
    "li x5, 6",            // Delay 10µs
    "jal x6, 1000f",
    "mv x25, x1",           // Release the pull to LO
    "li x5, 64",            // Delay 55µs
    "jal x6, 1000f",
    "li x19, 0x11",            // Send an acknowledgement 1 to FIFO
    "j 10b",                // Receive next command

"51:",
    // Write a zero bit
    "li x5, 60",            // Delay 65µs
    "jal x6, 1000f",
    "mv x25, x1",           // Release the pull to LO
    "li x5, 10",            // Delay 5µs
    "jal x6, 1000f",
    "li x19, 0x10",         // Send an acknowledgement 1 to FIFO
    "j 10b",                // Receive next command

"60:",
    "mv x5, x8",            // Caller says wait for how long
    "jal x6, 1000f",
    "li x19, 0x16",         // Signal success to the caller
    "j 10b",                // Await next command

"70:",
    "mv x23, x0",           // Set our BIO pin to LO
    "mv x24, x1",           // Set our BIO pin to output
    "li x19, 0x70",         // Signal success to the caller
    "j 10b",                // Await next command

"80:",
    "mv x25, x1",           // Set our BIO pin to input
    "li x19, 0x80",         // Signal success to the caller
    "j 10b",                // Await next command

"1000:",
    "mv x20, x0",           // Write to x20 to delay for one quantum
    "addi x5, x5, -1",      // dec x5
    "bne x5, x0, 1000b",    // loop if x5>0
    "jalr x0, x6, 0"        // return

);

