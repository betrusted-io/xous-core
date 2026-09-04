#[allow(unused)]
use std::time::{Duration, Instant}; // for read function timeout

use arbitrary_int::{Number, u5};
use bao1x_api::bio::*;
use bao1x_api::bio_code;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use utralib::generated::utra::bio_bdma;

/* Can Bus 2.0 Introduction
 *
 * This is an overview, meant to get you started, so some details are not entirely accurate.
 *
 * Can Bus communicates using frames. These are packages of data with a fixed structure, a variable data
 * payload, and a checksum. Each frame consists of
 *  - an arbitration field. If two frames start sending simultaneously, this field decides which one gets to
 *    send and which one has to stop.
 *  - a configuration field.
 *  - a checksum (called CRC) that allows a receiver to see if a message has been received correctly.
 *
 * The arbitration field contains
 *  - the message ID. This is not an address but a label of contents: it says something about the type of
 *    message, not the intended recipient.
 *  - the IDE bit, which distinguishes standard and extended frames. These have different lengths message IDs.
 * The configuration field contains
 *  - the DLC (data length code), which is the number of data bytes that the frame holds (can be 0-8).
 *
 * There are several mechanisms to ensure that either all nodes receive a correct frame, or no node does.
 *
 * Can bus operates on long wires (relative to its operating frequency) so transmission delays become
 * relevant. There needs to be a mechanism to ensure that all nodes sample the same value regardless of where
 * they are on the bus. This is done by synchronizing a digital PLL to every transmission and by allowing
 * enough time for a signal to propagate to every node before sampling.
 *
 * More information:
 * https://en.wikipedia.org/wiki/CAN_bus#CAN_2.0_(Classical_CAN)
 * https://www.can-cia.org/can-knowledge/can-cc
 *
 *
 * ===
 *
 *
 * About this implementation:
 *
 * The tx core takes a preformed message, calculates a CRC, adds SOF and EOF bits and sends it while bit
 * stuffing and checking for transmission errors.
 *
 * The clk core synchronizes to the line activity and provides timing pulses for the tx and rx cores.
 *
 * The rx core monitors the line for activity, de-stuffs and checks any messages it receives, and passes
 * them to the host if they're correct.
 *
 * The dbg core is reserved for supervisor activities (error accounting, re-send on failure, queue
 * management) but those are not implemented. Right now it provides a way to visualize flag status on the
 * scope.
 *
 * There's no way to send error or overload frames and no timing rules are implemented. If a core detects
 * an error, it raises the corresponding flag, but they're reset at the start of the next message.
 *
 * To use, push the pre-formed message without SOF or EOF bits onto FIFO2 (left-aligned).
 *
 * The rx core returns any messages it reads through FIFO1, without SOF, CRC, or EOF bits, but only if
 * they're correct. Faulty messages are discarded. First it returns ID and config fields, right-aligned
 * (two registers in case of an extended frame), followed by the data field.
 *
 * For examples, see the sending functions.
 *
 * The rx core blocks until it detects a SOF bit. For this to work, route a duplicate of the signal at the
 * rx pin to the clk pin.
 *
 * Bit timing is configured via hardcoded variables for now.
 *
 * All cores are set up to work with one sample per bit. There's a clock core implementation that emits
 * three sample flags per bit and an example code block in the tx core that shows how to convert the code
 * to sampling three times per bit.
 *
 * The clk core is the limiting factor in the system and it works up to 16 TQ per bit at 1 MHz line speed.
 * Sampling three times per bit is considerably slower.
 *
 */

pub struct CanBusConfig {
    pub rx_pin: u5,
    pub clk_pin: u5,
    pub tx_pin: u5,
    pub io_mode: IoConfigMode,
    pub frequency: u32,
}

#[allow(unused)] // never used, says the compiler
impl CanBusConfig {
    pub fn new(rx_pin: u5, clk_pin: u5, tx_pin: u5, frequency: u32) -> Result<Self, String> {
        if (rx_pin == tx_pin) || (rx_pin == clk_pin) || (clk_pin == tx_pin) {
            return Err("No duplicate pins allowed".to_string());
        }
        Ok(Self { rx_pin, clk_pin, tx_pin, io_mode: IoConfigMode::Overwrite, frequency })
    }
}

pub struct CanBus {
    bio_ss: Bio,
    rx_pin: u5,
    clk_pin: u5,
    tx_pin: u5,
    dbg_pin: u5,
    flag_pin: u5,
    // a CoreHandle is a page alias for the underlying virtual memory, assigned to the calling process to
    // avoid syscalls on accessing that resource
    _dbg_handle: CoreHandle,
    _rx_handle: CoreHandle,
    _tx_handle: CoreHandle,
    _clk_handle: CoreHandle,
    // a CoreCSR transforms the handle into a Rust object that can be shared and copied (more information
    // in the Baochip Coder's guide, Ch. 2)
    _dbg: CoreCsr,
    rx: CoreCsr,
    tx: CoreCsr,
    _clk: CoreCsr,
    // tracks the resources used by the object
    resource_grant: ResourceGrant,
}

impl Resources for CanBus {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "CanBus".to_string(),
            cores: vec![
                // assgning specific cores isn't strictly necessary but this way more important cores have
                // higher priority
                CoreRequirement::Specific(BioCore::Core0),
                CoreRequirement::Specific(BioCore::Core1),
                CoreRequirement::Specific(BioCore::Core2),
                CoreRequirement::Specific(BioCore::Core3),
            ],
            fifos: vec![Fifo::Fifo0, Fifo::Fifo1, Fifo::Fifo2, Fifo::Fifo3],
            static_pins: vec![],
            dynamic_pin_count: 5,
        }
    }
}

impl Drop for CanBus {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.bio_ss.release_dynamic_pin(self.rx_pin.as_u8(), &CanBus::resource_spec().claimer).unwrap();
        self.bio_ss.release_dynamic_pin(self.clk_pin.as_u8(), &CanBus::resource_spec().claimer).unwrap();
        self.bio_ss.release_dynamic_pin(self.tx_pin.as_u8(), &CanBus::resource_spec().claimer).unwrap();
        self.bio_ss.release_dynamic_pin(self.dbg_pin.as_u8(), &CanBus::resource_spec().claimer).unwrap();
        self.bio_ss.release_dynamic_pin(self.flag_pin.as_u8(), &CanBus::resource_spec().claimer).unwrap();
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

impl CanBus {
    pub fn new(config: CanBusConfig) -> Result<CanBus, BioError> {
        let rx_pin = config.rx_pin;
        let clk_pin = config.clk_pin; // route a duplicate rx signal to this pin
        let tx_pin = config.tx_pin;

        let dbg_pin = arbitrary_int::u5::new(1);
        let flag_pin = arbitrary_int::u5::new(24); // visualize flag status

        /* Bit timing configuration
         *
         * Determines the sampling point (0%: bit start, 100%: bit end) and the synchronization jump width.
         *
         * An earlier sampling point allows larger oscillator tolerances for every node and slower edge
         * transitions but requires shorter bus lengths for a given bus rate. The original specs put the
         * sampling point close to 2/3 whereas CANopen specifies close to 90%.
         * Can Bus was developed primarily for hardware, so setting the sample point works like this: For a
         * given bus rate, set a number of quanta per bit, then choose how many quanta pass before sampling.
         *
         *  Example 1: 10 quanta, sample after 6 means a sampling point of 60%.
         *  Example 2: 16 quanta, sample after 14 means a sampling point of 87,5%.
         *  Example 3: 20 quanta, sample after 12 means a sampling point of 60%.
         *
         * Example 1 and example 3 are functionally the same, the difference doesn't matter in a software
         * implementation.
         *
         * Synchronization jump width (SJW) limits the maximum adjustment per bit.
         *
         * Set SJW to min(4, phase_seg_1, phase_seg_2) with
         *   phase_seg_1: quanta before sampling point - propagation delay
         *   phase_seg_2: quanta after sampling point
         *
         * (Detail note: SJW needs to be large enough to compensate oscillator difference but as small as
         * possible to precent over-correction due to random phase shifts or noise. It's practically fixed
         * once the sampling point is set.
         * In a lot of references, SJW is given as min(4, phase_seg_1) but that's because the original specs
         * specify specify phase_seg_1 = phase_seg_2.)
         *
         * Explanation with diagrams:
         * www.can-cia.org/fileadmin/cia/documents/publications/cnlm/december_2018/..
         * ..18-4_p28_optimizing_can_bit_configuration_for_robustness_kent_lennartsson_kvaser.pdf
         *
         * "Computation of CAN Bit Timing Parameters Simplified:"
         * https://stage.can-cia.org/fileadmin/cia/documents/proceedings/2012_taralkar.pdf
         */
        let quanta_per_bit = 16;
        let sampling_point = 12;
        let synchronization_jump_width = 3;

        let mut bio_ss = Bio::new();
        // claim resources
        let resource_grant = bio_ss.claim_resources(&CanBus::resource_spec())?;
        log::debug!("granted to CanBus: {:?}", resource_grant);
        // configure cores
        let config_clk =
            CoreConfig { clock_mode: ClockMode::TargetFreqFrac(config.frequency * quanta_per_bit) };
        let config_rx = CoreConfig { clock_mode: ClockMode::ExternalPin(BioPin::new(clk_pin.as_u8())) };
        let config_tx = CoreConfig { clock_mode: ClockMode::TargetFreqFrac(config.frequency) };
        let dbg_kernel = can_bus_dbg_kernel();
        let clk_kernel = can_bus_clk_kernel();
        let rx_kernel = can_bus_rx_kernel();
        let tx_kernel = can_bus_tx_kernel();
        // The plan was to give lower IDs to more important cores but putting the tx core in core 3 breaks
        // halt-to-quantum for the rx core. To demonstrate, switch core 0 and core 3 and compare the output
        // on the dbg pin.
        // Solution: Leave it like this for now, isolate the bug later.
        bio_ss.init_core(resource_grant.cores[3], dbg_kernel, config_clk)?;
        bio_ss.init_core(resource_grant.cores[1], clk_kernel, config_clk)?;
        bio_ss.init_core(resource_grant.cores[2], rx_kernel, config_rx)?;
        bio_ss.init_core(resource_grant.cores[0], tx_kernel, config_tx)?;
        // claim pins, configure pins and IO
        bio_ss.claim_dynamic_pin(24, &CanBus::resource_spec().claimer)?;
        bio_ss.claim_dynamic_pin(rx_pin.as_u8(), &CanBus::resource_spec().claimer)?;
        bio_ss.claim_dynamic_pin(clk_pin.as_u8(), &CanBus::resource_spec().claimer)?;
        bio_ss.claim_dynamic_pin(tx_pin.as_u8(), &CanBus::resource_spec().claimer)?;
        bio_ss.claim_dynamic_pin(dbg_pin.as_u8(), &CanBus::resource_spec().claimer)?;
        let mut io_config = IoConfig::default();
        // transfer pin control to BIO
        io_config.mapped = (1 << rx_pin.as_u32())
            | (1 << clk_pin.as_u32())
            | (1 << tx_pin.as_u32())
            | (1 << flag_pin.as_u32())
            | (1 << dbg_pin.as_u32());
        io_config.i_inv = 1 << clk_pin.as_u32(); // invert clk pin signal
        io_config.mode = config.io_mode;
        bio_ss.setup_io_config(io_config).unwrap();
        bio_ss.set_core_run_state(&resource_grant, true);

        // get memory ranges needed for register access
        // safety: tx and rx are wrapped in CSR objects whose lifetime matches that of the handles
        let dbg_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo0) }?.expect("Didn't get Fifo0 handle");
        let rx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo1) }?.expect("Didn't get Fifo1 handle");
        let tx_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo2) }?.expect("Didn't get Fifo2 handle");
        let clk_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo3) }?.expect("Didn't get Fifo3 handle");
        let mut dbg = CoreCsr::from_handle(&dbg_handle);
        let mut rx = CoreCsr::from_handle(&rx_handle);
        let mut tx = CoreCsr::from_handle(&tx_handle);
        let mut clk = CoreCsr::from_handle(&clk_handle);

        // information for dbg core (via FIFO0)
        dbg.csr.wo(bio_bdma::SFR_TXF0, 1 << flag_pin.as_u32()); // flag pin location

        // information for the rx core (via FIFO1)
        rx.csr.wo(bio_bdma::SFR_TXF1, 1 << tx_pin.as_u32()); // tx pin location
        rx.csr.wo(bio_bdma::SFR_TXF1, 1 << rx_pin.as_u32()); // rx pin location
        rx.csr.wo(bio_bdma::SFR_TXF1, 1 << dbg_pin.as_u32()); // dbg pin location

        // information for the tx core (via FIFO2)
        tx.csr.wo(bio_bdma::SFR_TXF2, 1 << tx_pin.as_u32()); // tx pin location
        tx.csr.wo(bio_bdma::SFR_TXF2, 1 << rx_pin.as_u32()); // rx pin location
        //
        // information for the clk core (via FIFO3)
        clk.csr.wo(bio_bdma::SFR_TXF3, 1 << rx_pin.as_u32()); // rx pin location
        clk.csr.wo(bio_bdma::SFR_TXF3, quanta_per_bit);
        clk.csr.wo(bio_bdma::SFR_TXF3, sampling_point);
        clk.csr.wo(bio_bdma::SFR_TXF3, synchronization_jump_width);

        Ok(Self {
            bio_ss,
            rx_pin,
            clk_pin,
            tx_pin,
            dbg_pin,
            flag_pin,
            _dbg_handle: dbg_handle,
            _rx_handle: rx_handle,
            _tx_handle: tx_handle,
            _clk_handle: clk_handle,
            _dbg: dbg,
            rx,
            tx,
            _clk: clk,
            resource_grant,
        })
    }

    pub fn read(&mut self, timeout_ms: u32) {
        loop {
            let now = Instant::now();
            while self.rx.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL1) == 0
                && now.elapsed().as_millis() < timeout_ms as u128
            {}
            if now.elapsed().as_millis() >= timeout_ms as u128 {
                return;
            }
            let read = self.rx.csr.r(bio_bdma::SFR_RXF1);
            log::debug!(
                " {:8X}    {:08b}_{:08b}_{:08b}_{:08b}   ",
                read,
                (read >> 24),
                (read >> 16) & 0xff,
                (read >> 8) & 0xff,
                read & 0xff
            );
        }
    }

    pub fn send_standard_frame(&mut self) {
        // std frame                             ID      RTR IDE r0  DLC  data field
        self.tx.csr.wo(bio_bdma::SFR_TXF2, 0b00000010100__0___0___0__0010_00000001 << 6);
        // self.tx.csr.wo(bio_bdma::SFR_TXF2, 0b00000000111__0___0___0__0000_11101111 << 6);
        //                                more data bytes
        self.tx.csr.wo(bio_bdma::SFR_TXF2, 0x18181818);

        // expected output:
        //                       |--  ID   --|config|
        // A02    00000000_00000000_00001010_00000010
        //                          |--    data   --|
        // 100    00000000_00000000_00000001_00000000
    }

    pub fn send_extended_frame(&mut self) {
        // ext frame                               ID-11   SRR IDE      ID-18         RTR
        self.tx.csr.wo(bio_bdma::SFR_TXF2, 0b__01001010001__1___1__001010010100011100__0);
        // ext frame                           r1  r0  DLC   data field
        self.tx.csr.wo(bio_bdma::SFR_TXF2, 0b__0___0___0010__11101111_00011000_00011000 << 2);

        // expected output:
        //             |--            ID              --|RTR
        // 4A394A38    01001010_00111001_01001010_00111000
        //                                          |cfg |
        //        2    00000000_00000000_00000000_00000010
        //                               |--    data   --|
        //     EF18    00000000_00000000_11101111_00011000
    }
}

/* -------------------------------------------------------------------------------
 *
 *                               BIO CORE SECTION
 *
 * -------------------------------------------------------------------------------
 *
 * General architecture:
 *   core 0    ---    reserved for error accounting, transmission timing, or anything else
 *   core 1    CLK    synchronizes to line activity and provides timing flags
 *   core 2    RX     receives messages and prevents tx on a busy line
 *   core 3    TX     prepares and sends messages
 *
 * FIFO usage
 *   FIFO 0: debug readback
 *   FIFO 1: data pipe from rx core
 *   FIFO 2: data pipe into tx core
 *   FIFO 3: data pipe into clk core (for setup)
 *
 * Flag usage
 *
 *   0000 0000 0000 0000 0000 0000
 *                E -BSF CSAB ARTL
 *
 *   Core status [2:0]
 *   Tx errors [5:3]
 *   Rx errors [8:6]
 *   Synchronisation [12:9]
 *
 *   Bit  0: Line idle
 *   Bit  1: Transmitter active
 *   Bit  2: Receiver active
 *   Bit  3: Arbitration failure
 *   Bit  4: Bit error
 *   Bit  5: ACK error
 *   Bit  6: Bit stuffing error
 *   Bit  7: CRC error
 *   Bit  8: Form error
 *   Bit  9: Sample time
 *   Bit 10: Bit boundary
 *   Bit 11: ("li x28, 0x800" and "li x29, 0x800" don't work)
 *   Bit 12: EOF (request to signal bit boundaries)
 *   Bit 13:
 *   Bit 14:
 *   Bit 15:
 *   Bit 16:
 *   Bit 17:
 *   Bit 18:
 *   Bit 19:
 *   Bit 20:
 *   Bit 21:
 *   Bit 22:
 *   Bit 23:
 *
 *   "1" means the condition is present.
 *
 */

#[rustfmt::skip]
bio_code!(can_bus_dbg_kernel, CAN_BUS_DBG_START, CAN_BUS_DBG_END,
/* -------------------------------------------------------------------------------
 *
 *                                  RESERVED
 *
 * -------------------------------------------------------------------------------
 *
 * Set up to show flag status on the scope.
 *
 */

    "mv    x1, x16",         // get dbg pin location
    "mv    x26, x1",         // pin mask
    "mv    x24, x1",         // set pin to output

    // flag-to-be-tested

    // "li    x7, 0b1",         // line idle
    // "li    x7, 0b10",        // tx active
    // "li    x7, 0b100",       // rx active
    // "li    x7, 0x18",        // tx errors: arbitration + bit error
    // "li    x7, 0x20",        // tx errors: ACK
    // "li    x7, 0x1c0",       // all rx errors
    // "li    x7, 0x200",       // sample
    // "li    x7, 0x400",       // bit boundary
    // "li    x7, 0x600",       // sample and bit boundary
    // "li    x7, 0x800",       // dbg
    "li    x7, 0x1000",      // eof
    // "li    x7, 0x1001",      // eof and line idle
    // "li    x7, 0x1200",      // eof and sample
    // "li    x7, 0x1400",      // eof and bit boundary

"1:",
    "mv    x27, x7",         // set event sensitivity mask
    "not   x23, x1",         // pin low
    "mv    x0, x30",         // wait for flag
    // flag up
    "li    x27, 0",          // disable event sensitivity mask to enable non-blocking reads
    "mv    x22, x1",         // pin high
"2:",
    "mv    x20, x0",
    "and   x10, x30, x7",    // check if flag's still up
    "bnez  x10, 2b",         // still up => wait
    "j 1b"
);

#[rustfmt::skip]
bio_code!(can_bus_clk_kernel, CAN_BUS_CLK_START, CAN_BUS_CLK_END,
/* -------------------------------------------------------------------------------
 *
 *                                    CLOCK
 *
 * -------------------------------------------------------------------------------
 *
 * This core needs two clock sources: One signal to start sampling any time a SOF is detected, and a clock at
 * a multiple of the nominal line frequency. BIO can only appoint one clock source to each core, so the clock
 * core starts synchronizing when either of the other cores start running: the RX core because it
 * detects a SOF or the TX core because it receives a message through FIFO2.
 *
 * The core runs in a loop, sampling the bus once in each quantum to determine if an edge occurred. If an
 * edge occurred, the core synchronizes by adjusting the current cycle counter. Otherwise it checks if a
 * sample signal is scheduled for the current quantum or if it reached the end of a full bit, raises the
 * correct flags if necessary, and waits for the next quantum.
 *
 * The parameter Synchronization Jump Width sets a limit to the maximum allowable adjustment per cycle instead
 * of fully resetting the cycle counter on each edge.
 *
 * Some notes:
 *  - The Rx core needs to know the start and end of the ACK bit to send it. At this point, phase error
 *    handling is unimportant, so those signals get their own code block to keep functions disentangled:
 *    It's much easier to adjust an edge or to signal a bit boundary, rather than trying to do both at the
 *    same time.
 *  - The check for EOF flag happens at the sampling point to guarantee consistent and predictable
 *    change-over.
 * Timing considerations:
 *  - Error adjustments are spread over two quanta. Large errors take a longer path than short ones.
 *  - Correct edge is the same path as late edge, small error. Checking if an edge lands where
 *    expected speeds up that path but lengthens error handling.
 *  - There's an explicit instruction incrementing the loop counter while error handling. This could be
 *    folded into the adjustments to the loop counter, at (further) reduced code clarity. Example:
 *      "mv x8, x0"         =>   "li x8, 1"
 *      "addi x8, x8, 1"
 *  - Timing estimatations (CPI from PicoRV's Github page):
 *      sampling quantum
 *         2 ALU reg+reg, 4 ALU reg+imm, 2 branches not taken, 1 branch taken, 1 jump
 *         2*4 + 4*3 + 2*4 + 6 + 3 = 37
 *      end-of-bit quantum
 *        2 ALU reg+reg, 2 ALU reg+imm, 1 branch not taken, 2 branches taken, 1 jump
 *        2*4 + 2*3 + 4 + 2*6 + 3 = 33
 *
 *      early edge, large error
 *         cmp, sub, cmp+jmp, add, addi, jmp      5*4 +   6 = 26
 *      late edge, large error
 *         cmp+jmp, cmp+jmp, add, addi, jmp       3*4 + 2*6 = 30
 *  - 16 TQ at 1 MHz allows up to 43 cycles / TQ so this should run well.
 *
 * Register usage
 *   x 1 mask for eof and line idle flags
 *   x 2
 *   x 3
 *   x 4 phase error
 *   x 5
 *   x 6
 *   x 7 temp variable for flag checks
 *   x 8 loop counter (tracks current quantum)
 *   x 9 halfway point (1/2 of total quanta)
 *   x10 quanta before sampling point
 *   x11 quanta per bit
 *   x12 SJW
 *   x13 bus state
 *   x14 bus state
 *   x15 rx pin location
 *
 */

    // setup

    "mv    x15, x19",        // store rx pin location
    "mv    x11, x19",        // store quanta per bit
    "mv    x10, x19",        // store (quanta before) sampling point
    "mv    x12, x19",        // store SWJ

    "li    x1, 0x1001",      // mask for eof and line idle flags
    "li    x8, 0",           // initialize counter
    "srli  x9, x11, 1",      // halfway point = # of quanta / 2

 // line idle

"10:",
    "li    x27, 6",          // set event sensitivity mask to receiver or transmitter active flags
    "mv    x0, x30",         // start synchronizing when either core is active
    "li    x27, 0",          // disable event sensitivity mask to enable non-blocking reads

// quantum loop

"20:",
    "mv    x20, x0",         // wait for quantum
    "mv    x13, x14",        // save old sample
    "and   x14, x15, x21",   // sample bus

    "beq   x13, x14, 30f",   // edge detected?             no => skip phase error adjustments

    // phase error handling

    "mv    x20, x0",         // wait for quantum           spread error handling across two quanta
    "bgeu  x9, x8, 22f",     // early or late edge?        halfway point > current quantum? yes => late edge

    // early edge
    "sub   x4, x11, x8",     // calculate phase error      error = total quanta - current quantum
    "bltu  x12, x4, 21f",    // phase error withing SJW?   error > SJW => 21f
    // phase error ≤ SJW
    "mv    x8, x0",          // synchronize                reset loop counter
    "addi  x8, x8, 1",       //                            increase quantum counter
    "j 30f",                 //                            continue
    // phase error > SJW
"21:",
    "add   x8, x8, x12",     // synchronize                advance loop by SJW (but not by full phase error)
    "addi  x8, x8, 1",       //                            increase quantum counter
    "j 30f",                 //                            continue

    // late edge
"22:",
                             // calculate phase error      same as current loop counter
    "bltu  x12, x8, 23f",    // phase error withing SJW?   error > SJW => 23f
    // phase error ≤ SJW
    "mv    x8, x0",          // synchronize                reset loop counter
    "addi  x8, x8, 1",       //                            increase quantum counter
    "j 30f",                 //                            continue
"23:",
    // phase error > SJW
    "sub   x8, x8, x12",     // synchronize                retard loop by SJW (but not by full phase error)
    "addi  x8, x8, 1",       //                            increase quantum counter
    "j 30f",                 //                            continue

    // signal sampling time

"30:",
    "addi  x8, x8, 1",       // increase quantum counter
    "bne   x8, x10, 31f",    // check if it's sampling time (no => skip)
    "li    x28, 0x200",      // set sample flag
    "li    x29, 0x200",      // clear sample flag
    "and   x7, x30, x1",     // check eof and line idle flags
    "bnez  x7, 40f",         // if set => 40f
    "j 20b",                 // next loop

    // end of bit bookkeeping

"31:",
    "bltu  x8, x11, 20b",    // check if full bit time is up
    "mv    x8, x0",          // bit time is up, reset counter
    "j 20b",                 // next loop

    // stop synchronizing, signal bit boundaries

"40:",
    "li    x29, 0x1000",     // clear eof flag
    "mv    x20, x0",         // wait for quantum
    "andi  x7, x30, 1",      // check line idle flag
    "bnez  x7, 10b",         // line idle => wait for next SOF
    "addi  x8, x8, 1",       // increase quantum counter
    "bne   x8, x10, 41f",    // check if it's sampling time (no => continue)
    "li    x28, 0x200",      // set sample flag
    "li    x29, 0x200",      // clear sample flag
    "j 40b",                 // next loop
"41:",
    "bltu  x8, x11, 40b",    // check if full bit time is up
    "li    x28, 0x400",      // set bit boundary flag
    "li    x29, 0x400",      // clear bit boundary flag
    "mv    x8, x0",          // bit time is up, reset counter
    "j 40b"                  // next loop
);

#[rustfmt::skip]
bio_code!(can_bus_rx_kernel, CAN_BUS_RX_START, CAN_BUS_RX_END,
/* -------------------------------------------------------------------------------
 *
 *                                   RECEIVER
 *
 * -------------------------------------------------------------------------------
 *
 * The receiver processes a message in four blocks:
 *  - A common path until the IDE bit, which distinguishes standard and extended frames. Both have different
 *   lengths but the CRC bytes need to line up at the end of the data field, so they get diverging branches.
 *  - Separate paths until the DLC field, which encodes the length of the data field.
 *  - A common path again for the data field, which is the last part of the message covered by the CRC.
 *  - The last block is checking the CRC, acknowledging a correct transmission, and passing the correct
 *    message to the host.
 *  - Timing: line speed of 1 MHz gives us 700 cycles or ~150 instructions per bit. Longest bit takes ~50
 *    instructions.
 *
 * The core is clocked to an inverted copy of the rx_pin so a SOF (which is a low-going pulse) starts the
 * processing. A spike can start the core, too, so the core waits for one sample flag, checks if the bus is
 * still low, and either waits for the next bit or blocks again.
 *
 * For details regarding the CRC table calculation, offsets, and bit stuffing mechanism, see the tx core.
 *
 */

    // initialize
    "li    sp, 0x800",       // sp
    "li    x5, 0x4000",      // crc (this value starts the computation, nothing else)
    "li    x9, 0x4000",      // mask for bit 14
    "li    x10, 0x7fff",     // mask for bits [14:0]
    "li    x11, 0x4599",     // generator polynom
    "li    x12, 0x800",      // store table base
    "li    x14, 1",          // outer loop counter
    "li    x15, 0xff",       // limit for outer loop

    // outer loop: powers of 2
    "sh    x0, 0(sp)",       // set table[0] to 0
"10:",
    "and   x6, x5, x9",      // extract bit 14
    "slli  x5, x5, 1",       // shift crc << 1
    "beqz  x6, 11f",         // if bit 14 was not set, skip XOR
    "xor   x5, x5, x11",     // XOR previous crc with generator poly
"11:",
    "and   x5, x5, x10",     // discard highest bit

     // inner loop: remaining values
    "li    x13, 0",          // reset counter
"12:",
    // retrieve existing CRC
    "slli  sp, x13, 1",      // table index = 2 * inner counter (2 bytes / entry)
    "add   sp, sp, x12",     // address = base + index
    "lhu   x7, 0(sp)",       // get table[addr]
    // calculate new CRC
    "xor   x7, x7, x5",      // XOR with newly calculated CRC
    // store new CRC
    "add   sp, x13, x14",    // sum = inner counter + outer counter
    "slli  sp, sp, 1",       // index = sum * 2 (2 bytes / entry)
    "add   sp, sp, x12",     // address = base + index
    "sh    x7, 0(sp)",       // store new CRC
    // inner loop bookkeeping
    "addi  x13, x13, 1",     // increment inner loop counter
    "bltu  x13, x14, 12b",   // continue as long as inner loop counter < outer loop counter

    // outer loop bookkeeping
    "slli  x14, x14, 1",     // left-shift outer loop counter
    "bgeu  x14, x15, 13f",   // break loop after 255
    "j 10b",                 // continue with the next power of 2
"13:",

/* General core setup
 *
 * Register usage
 *   x11 temp variable to find shift counters
 *   x12
 *   x13 tx pin location
 *   x14 shift counter for rx pin
 *   x15 rx pin location
 *
 */

    "mv    x13, x17",        // read first argument from FIFO1, which is the tx pin location
    "mv    x15, x17",        // read second argument from FIFO1, which is the rx pin location
    "or    x26, x13, x15",   // set GPIO mask
    "mv    x24, x13",        // set tx pin as output
    "mv    x25, x15",        // set rx pin as input

    // dbg setup
    "mv    x1, x17",         // get dbg pin location from FIFO1
    "or    x26, x26, x1",    // add dbg pin to GPIO pin mask
    "or    x24, x13, x1",    // set dbg pin as output

    // how many bits do we need to shift over?
    "li    x11, 0x1",        // initialize indicator bit
    "li    x14, 0",          // initialize counter
"10:", // get counter for rx pin
    "beq   x11, x15, 11f",   // if indicator and pin match, we found our number
    "slli  x11, x11, 1",     // shift indicator one over
    "addi  x14, x14, 1",     // increase counter
    "j 10b",
"11:",
    "li    sp, 0xa00",       // set sp to setup information
    "sw    x13, 0(sp)",      // store tx pin location

/* Receiving loop
 *
 * For each bit, wait for the sample flag, sample the bus, then de-stuff the message stream. Interpret the
 * bit if necessary (eg. IDE bit, DLC) and update the CRC every eight bits. Lastly, compare the calculated
 * CRC with the one on the bus, acknowledge a correct receipt, and write the message into FIFO1.
 * If an error occurs at any point, stop, raise an error flag and wait for the next SOF.
 *
 * Register usage
 *   x 1 dbg pin location
 *   x 2 sp
 *   x 3 loop counter
 *   x 4 remaining bytes of data field
 *   x 5 remaining bytes in msg buffer
 *   x 6 pointer to message in memory
 *   x 7 temp variable: crc
 *   x 8 temp variable: crc
 *   x 9 crc remainder
 *   x10 temp variable: bit stuffing checks
 *   x11 de-stuffed message
 *   x12 bit stuffing buffer (as received)
 *   x13 bus state
 *   x14 shift counter for rx pin
 *   x15 rx pin location
 *
 */

    // setup
"20:",
    "li    x6, 0xa10",       // set pointer to message base
    "li    x11, 0",          // initialize message buffer
    "li    x12, 0b10",       // initialize bit stuffing buffer ("1" to initialize, plus sof)
    "not   x23, x1",         // dbg pin low
    "li    x27, 0x200",      // set event sensitivity mask to sample flag
    "li    x28, 0b1",        // set line idle flag
    "li    x29, 0b100",      // clear receiver active flag

    // wait for SOF
    "mv    x20, x0",         // wait for falling edge
    "mv    x22, x1",         // dbg pin high
    "li    x28, 0b100",      // set receiver active flag
    "li    x29, 1",          // clear line idle flag
    "mv    x0, x30",         // wait for sample flag
    "and   x13, x21, x15",   // sample bus
    "bnez  x13, 20b",        // spike => wait for sof (see notes at the beginning of the section)
    "li    x29, 0x1c0",      // clear receiver error flags
    "li    x3, 13",          // set loop counter to get first 14 bits

    // receive until ide bit
"21:",
    "mv    x0, x30",         // wait for sample flag, save state for tx active check
    "and   x13, x21, x15",   // sample bus
    "srl   x13, x13, x14",   // shift new bit into position
    "slli  x12, x12, 1",     // advance bit stuffing buffer
    "or    x12, x12, x13",   // insert new bit into buffer
    "andi  x10, x12, 0x3e",  // check bits [5:1] for all zeros
    "bnez  x10, 22f",        // if not => skip bit stuffing check
    "beqz  x13, 100f",       // next bit should be one, if not => stuffing error
    "j 21b",                 // don't insert stuffing bits into message buffer
"22:",
    "xori  x10, x10, 0x3e",  // check last [5:1] for all ones
    "bnez  x10, 23f",        // if not => skip bit stuffing check
    "bnez  x13, 100f",       // next bit should be zero, if not => stuffing error
    "j 21b",                 // don't insert stuffing bits into message buffer
"23:",
    "slli  x11, x11, 1",     // advance message buffer
    "or    x11, x11, x13",   // insert new bit into buffer
    "addi  x3, x3, -1",      // bits left in register
    "beqz  x3, 30f",         // stop after ide bit
    "j 21b",                 // get next bit

"30:", // check ide bit
    "bnez  x13, 40f",        // ide high => extended frame

    // standard frame

    // start crc
    "li    x9, 0x1800",      // set mask to bits [13:12]
    "and   x9, x9, x11",     // extract bits
    "srli  x9, x9, 10",      // two shifts in one: remove trailing zeros, multiply by 2
                             // offset = index * 2 (there's 2 bytes / entry)
    "li    sp, 0x800",       // set sp to table base
    "add   sp, sp, x9",      // address = table base + offset
    "lhu   x9, 0(sp)",       // read constant
"31:",
    "li    x8, 0x7f8",       // mask for next byte
    "and   x8, x8, x11",     // extract next byte
    "srli  x8, x8, 3",       // remove trailing zeros
    "srli  x7, x9, 7",       // prepare remainder for xor
    "xor   x8, x8, x7",      // generate table index
    "slli  x8, x8, 1",       // offset = index * 2 (there's 2 bytes / entry)
    "li    sp, 0x800",       // set sp to table base
    "add   sp, sp, x8",      // address = table base + offset
    "lhu   x8, 0(sp)",       // read constant
    "slli  x9, x9, 8",       // shift old crc
    "xor   x9, x9, x8",      // xor old crc and constant for new crc
    "li    x8, 0x7fff",      // mask for bits [14:0]
    "and   x9, x9, x8",      // discard bits [31:15]

    // receive until dlc
    "li    x3, 5",           // set loop counter to get next 5 bits
"32:",
    "mv    x0, x30",         // receive and de-stuff bits as above
    "and   x13, x21, x15",
    "srl   x13, x13, x14",
    "slli  x12, x12, 1",
    "or    x12, x12, x13",
    "andi  x10, x12, 0x3e",
    "bnez  x10, 33f",
    "beqz  x13, 100f",
    "j 32b",
"33:",
    "xori  x10, x10, 0x3e",
    "bnez  x10, 34f",
    "bnez  x13, 100f",
    "j 32b",
"34:",
    "slli  x11, x11, 1",
    "or    x11, x11, x13",
    "addi  x3, x3, -1",
    "beqz  x3, 35f",
    "j 32b",
"35:", // catch up on crc
    "li    x8, 0xff",        // mask for next byte
    "and   x8, x8, x11",     // extract next byte
    "srli  x7, x9, 7",       // calculate crc as above
    "xor   x8, x8, x7",
    "slli  x8, x8, 1",
    "li    sp, 0x800",
    "add   sp, sp, x8",
    "lhu   x8, 0(sp)",
    "slli  x9, x9, 8",
    "xor   x9, x9, x8",
    "li    x8, 0x7fff",
    "and   x9, x9, x8",

    // extract dlc
    "andi  x4, x11, 0xf",    // extract dlc
    "sw    x11, 0(x6)",      // write current message to memory
    "li    x11, 0",          // reset msg buffer
    "li    x5, 4",           // four bytes remaining in msg buffer
    "j 60f",                 // receive data bytes

    // extended frame

"40:", // start crc
    "li    x9, 0x1f80",      // set mask to bits [13:8]
    "and   x9, x9, x11",     // extract bits
    "srli  x9, x9, 6",       // two shifts in one: remove trailing zeros, multiply by 2
                             // offset = index * 2 (there's 2 bytes / entry)
    "li    sp, 0x800",       // set sp to table base
    "add   sp, sp, x9",      // address = table base + offset
    "lhu   x9, 0(sp)",       // read constant

    // receive until dlc
    "li    x3, 1",           // set loop counter: one bit remaining until next crc calculation
    "li    x4, 4",           // four bytes until dlc
    "li    x5, 19",          // set register counter: 13 bits already in register, 19 bits remaining
    "j 51f",
"50:",
    "beqz x4, 56f",          // stop when reaching dlc
    "li    x3, 8",           // set loop counter to get next byte
"51:", // entry point
    "mv    x0, x30",         // receive and de-stuff bits as above
    "and   x13, x21, x15",
    "srl   x13, x13, x14",
    "slli  x12, x12, 1",
    "or    x12, x12, x13",
    "andi  x10, x12, 0x3e",
    "bnez  x10, 52f",
    "beqz  x13, 100f",
    "j 51b",
"52:",
    "xori  x10, x10, 0x3e",
    "bnez  x10, 53f",
    "bnez  x13, 100f",
    "j 51b",
"53:",
    "slli  x11, x11, 1",
    "or    x11, x11, x13",
    "addi  x5, x5, -1",      // decrease register counter
    "bnez  x5, 54f",         // register full? yes => write to memory
    "sw    x11, 0(x6)",      // store current message block in memory
    "li    x11, 0",          // reset message buffer
    "li    x5, 32",          // 32 bits remaining in message buffer
"54:",
    "addi  x3, x3, -1",
    "beqz  x3, 55f",
    "j 51b",
"55:", // update crc after every full byte
    "li    x8, 0xff",        // calculate crc as above
    "and   x8, x8, x11",
    "srli  x7, x9, 7",
    "xor   x8, x8, x7",
    "slli  x8, x8, 1",
    "li    sp, 0x800",
    "add   sp, sp, x8",
    "lhu   x8, 0(sp)",
    "slli  x9, x9, 8",
    "xor   x9, x9, x8",
    "li    x8, 0x7fff",
    "and   x9, x9, x8",
    "addi  x4, x4, -1",      // decrease byte counter
    "j 50b",                 // continue until reaching dlc

"56:", // extract dlc
    "andi  x4, x11, 0xf",    // extract dlc
    "addi  x6, x6, 4",       // advance message pointer one word
    "sw    x11, 0(x6)",      // write current message to memory
    "li    x11, 0",          // reset msg buffer
    "li    x5, 4",           // four bytes remaining in msg buffer

    // process data field
"60:",
    "beqz  x4, 70f",         // all bytes received => crc check
    "li    x3, 8",           // set loop counter to get next byte
"61:",
    "mv    x0, x30",         // receive and de-stuff bits as above
    "and   x13, x21, x15",
    "srl   x13, x13, x14",
    "slli  x12, x12, 1",
    "or    x12, x12, x13",
    "andi  x10, x12, 0x3e",
    "bnez  x10, 62f",
    "beqz  x13, 100f",
    "j 61b",
"62:",
    "xori  x10, x10, 0x3e",
    "bnez  x10, 63f",
    "bnez  x13, 100f",
    "j 61b",
"63:",
    "slli  x11, x11, 1",
    "or    x11, x11, x13",
    "addi  x3, x3, -1",
    "beqz  x3, 64f",
    "j 61b",
"64:", // update crc after every full byte
    "li    x8, 0xff",        // calculate crc as above
    "and   x8, x8, x11",
    "srli  x7, x9, 7",
    "xor   x8, x8, x7",
    "slli  x8, x8, 1",
    "li    sp, 0x800",
    "add   sp, sp, x8",
    "lhu   x8, 0(sp)",
    "slli  x9, x9, 8",
    "xor   x9, x9, x8",
    "li    x8, 0x7fff",
    "and   x9, x9, x8",
    "addi  x4, x4, -1",      // decrease data field counter
    "addi  x5, x5, -1",      // decrease message buffer counter
    "bnez  x5, 60b",         // message buffer full? no => get next byte
    "addi  x6, x6, 4",       // advance message pointer one word
    "sw    x11, 0(x6)",      // store current message block in memory
    "li    x11, 0",          // reset msg buffer but keep last bit for crc calculations
    "li    x5, 4",           // four bytes remaining in msg buffer
    "j 60b",                 // continue for the rest of data field
"70:", // write partly filled message buffer to memory
    "addi  x5, x5, -4",      // if the message buffer has just been written out, x5 is set to 4
    "beqz  x5, 71f",         // if 0 => skip writing out
    "addi  x6, x6, 4",       // advance message pointer one word
    "sw    x11, 0(x6)",      // store current message block in memory

    // check crc
"71:",
    "li    x3, 15",          // set loop counter to get next 15 bits
    "li    x8, 0x4000",      // initialize mask to first crc bit (bit 14)
"72:",
    "mv    x0, x30",         // receive and de-stuff bits as above
    "and   x13, x21, x15",
    "srl   x13, x13, x14",
    "slli  x12, x12, 1",
    "or    x12, x12, x13",
    "andi  x10, x12, 0x3e",
    "bnez  x10, 73f",
    "beqz  x13, 100f",
    "j 72b",
"73:",
    "xori  x10, x10, 0x3e",
    "bnez  x10, 74f",
    "bnez  x13, 100f",
    "j 72b",
"74:",
    "slli  x11, x11, 1",
    "or    x11, x11, x13",
    "addi  x3, x3, -1",
    "and   x7, x8, x9",      // mask crc bit for comparison
    "srl   x7, x7, x3",      // remove trailing zeros (shift counter is the same as remaining bits counter)
    "bne   x7, x13, 101f",   // crc bit != received bit => crc error
    "srli  x8 , x8, 1",      // shift mask to next crc bit
    "beqz  x3, 75f",         // crc complete, go to crc delimiter
    "j 72b",                 // receive next crc bit

"75:", // crc delimiter bit
    "li    x28, 0x1000",     // set eof flag (request signals for bit boundaries)
    "mv    x3, x30",         // wait for sample flag and store flags
    "and   x13, x21, x15",   // sample bus
    "beqz  x13, 102f",       // "0" => form error

    // prepare for end of frame bits
    "li    x27, 0x600",      // set event sensitivity mask to sample and bit boundary flags
    "li    sp, 0xa00",       // set sp to setup information
    "lw    x12, 0(sp)",      // load tx pin location
    // ack bit
    "mv    x0, x30",         // wait for bit boundary
    "not   x23, x12",        // send ack bit: tx pin low
    "mv    x0, x30",         // wait for sample flag
    "mv    x0, x30",         // bit boundary: ack bit ends
    // ack delimiter bit
    "mv    x22, x12",        // tx pin high
    "mv    x0, x30",         // wait for sample flag
    "and   x13, x21, x15",   // sample bus
    "beqz  x13, 102f",       // "0" => form error
    "mv    x0, x30",         // bit boundary: ack delimiter ends

    // eof bits
    // bit 1
    "mv    x0, x30",         // wait for sample flag
    "and   x13, x21, x15",   // sample bus
    "beqz  x13, 102f",       // "0" => form error
    "mv    x0, x30",         // bit boundary: end of eof bit 1
    // bit 2
    "mv    x0, x30",
    "and   x13, x21, x15",
    "beqz  x13, 102f",
    "mv    x0, x30",
    // bit 3
    "mv    x0, x30",
    "and   x13, x21, x15",
    "beqz  x13, 102f",
    "mv    x0, x30",
    // bit 4
    "mv    x0, x30",
    "and   x13, x21, x15",
    "beqz  x13, 102f",
    "mv    x0, x30",
    // bit 5
    "mv    x0, x30",
    "and   x13, x21, x15",
    "beqz  x13, 102f",

    // message received correctly, pass to host
    "li    sp, 0xa10",       // set sp to message base
"77:",
    "lw    x17, 0(sp)",      // load first 4 bytes into fifo1
    "beq   x6, sp, 78f",     // same as last written address => last eof bit
    "addi  sp, sp, 4",       // else increment sp one word
    "j 77b",                 // read next word
"78:",
    "mv    x0, x30",         // bit boundary: end of bit 5

    // bit 6: treated as "don't care"
    "mv    x0, x30",         // wait for sample flag
    "mv    x0, x30",         // wait for bit boundary
    "j 110f",                // wait for next sof

    // errors

"100:", // bit stuffing error
    "li    x28, 0x40",       // set bit stuffing error flag
    "j 110f",                // wait for next sof

"101:", // crc error
    "li    x28, 0x80",       // set crc error flag
    "j 110f",                // wait for next sof

"102:", // form error
    "li    x28, 0x100",      // set form error flag
    "j 110f",                // wait for next sof

/* End of message
 *
 * Nothing happens here, the instruction only provides a common end point for all code flows. Core setup -
 * initializing registers, setting flags - happens at the start of the code block.
 *
 */

"110:", // end of message
    "j 20b",               // wait for next sof
"nop"
);

#[rustfmt::skip]
bio_code!(can_bus_tx_kernel, CAN_BUS_TX_START, CAN_BUS_TX_END,

/* -------------------------------------------------------------------------------
 *
 *                                 TRANSMITTER
 *
 * -------------------------------------------------------------------------------
 *
 * This core receives preformed messages consisting of an ID field, a data field, and some control infor-
 * mation, appends a CRC, performs bit stuffing, and sends them when the line is clear. To speed up CRC
 * calculations, it prepares a table of constants during startup.
 *
 * Memory map
 *   0x800 .. 0x9fe     CRC table
 *   0xa00              Setup information
 *   0xa10              Message after preparation, prior to sending
 *   0xa20              CRC
 */

/* CRC table
 *
 * Precomputes a table of CRC constants that allow processing messages one byte at a time instead of one
 * bit at a time. A site to check the pre-computed table:
 *   https://www.compu-tools.com/crc-lookup-table/
 *
 * Instead of calculating all values, it's enough to calculate the values for powers of 2, deriving the rest
 * by XORing existing table entries. Algorithm courtesy of Wikipedia:
 *   https://en.wikipedia.org/wiki/Computation_of_cyclic_redundancy_checks#Generating_the_lookup_table
 *
 * The algorithm in rust:
 *
 *   let mut i = 1;
 *   let mut crc: u16 = 0x4000;
 *   let mut table: [u16; 256] = [0; 256];
 *   let table[0] = 0;
 *
 *   while i < 256 {
 *
 *     // calculate new CRC for power of 2
 *     crc = if (crc & 0x4000) != 0 { (crc << 1) ^ 0x4599 } else { crc << 1 };
 *
 *     // discard highest bit
 *     crc = crc & 0x7fff;
 *
 *     // populate intermediate values
 *     for j in 0..i { table[i + j] = crc ^ table[j]; }
 *
 *     i = i << 1;
 *   }
 *
 * Register usage
 *   x 1
 *   x 2 stack pointer
 *   x 3
 *   x 4
 *   x 5 current crc
 *   x 6 bit 14 of x5
 *   x 7 table[j] (temp value to calculate intermediate CRCs)
 *   x 8
 *   x 9 mask for bit 14
 *   x10 mask for bits [14:0]
 *   x11 generator polynom
 *   x12 table base (0x800)
 *   x13 inner loop counter
 *   x14 outer loop counter
 *   x15 limit for outer loop
 *
 */

    // initialize
    "li    sp, 0x800",       // sp
    "li    x5, 0x4000",      // crc (this value starts the computation, nothing else)
    "li    x9, 0x4000",      // mask for bit 14
    "li    x10, 0x7fff",     // mask for bits [14:0]
    "li    x11, 0x4599",     // generator polynom
    "li    x12, 0x800",      // store table base
    "li    x14, 1",          // outer loop counter
    "li    x15, 0xff",       // limit for outer loop

    // outer loop: powers of 2
    "sh    x0, 0(sp)",       // set table[0] to 0
"10:",
    "and   x6, x5, x9",      // extract bit 14
    "slli  x5, x5, 1",       // shift crc << 1
    "beqz  x6, 11f",         // if bit 14 was not set, skip XOR
    "xor   x5, x5, x11",     // XOR previous crc with generator poly
"11:",
    "and   x5, x5, x10",     // discard highest bit

     // inner loop: remaining values
    "li    x13, 0",          // reset counter
"12:",
    // retrieve existing CRC
    "slli  sp, x13, 1",      // table index = 2 * inner counter (2 bytes / entry)
    "add   sp, sp, x12",     // address = base + index
    "lhu   x7, 0(sp)",       // get table[addr]
    // calculate new CRC
    "xor   x7, x7, x5",      // XOR with newly calculated CRC
    // store new CRC
    "add   sp, x13, x14",    // sum = inner counter + outer counter
    "slli  sp, sp, 1",       // index = sum * 2 (2 bytes / entry)
    "add   sp, sp, x12",     // address = base + index
    "sh    x7, 0(sp)",       // store new CRC
    // inner loop bookkeeping
    "addi  x13, x13, 1",     // increment inner loop counter
    "bltu  x13, x14, 12b",   // continue as long as inner loop counter < outer loop counter

    // outer loop bookkeeping
    "slli  x14, x14, 1",     // left-shift outer loop counter
    "bgeu  x14, x15, 13f",   // break loop after 255
    "j 10b",
"13:",

/* General core setup
 *
 * Register usage
 *   x11 temp variable to find shift counters
 *   x12 shift counter for rx pin
 *   x13 shift counter for tx pin
 *   x14 rx pin location
 *   x15 tx pin location
 *
 */

    "mv    x15, x18",        // read first argument from FIFO2, which is the tx pin location
    "mv    x14, x18",        // read second argument from FIFO2, which is the rx pin location
    "or    x26, x14, x15",   // set GPIO mask
    "mv    x24, x15",        // set tx pin as output
    "mv    x25, x14",        // set rx pin as input

    // how many bits do we need to shift over?
    "li    x11, 0x80000000", // initialize indicator bit
    "li    x12, 0",          // initialize counter
"20:", // get counter for rx pin
    "beq   x11, x14, 21f",   // if indicator and pin match, we found our number
    "srli  x11, x11, 1",     // shift indicator one over
    "addi  x12, x12, 1",     // increase counter
    "j 20b",
"21:", // get counter for tx pin
    "li    x11, 0x80000000", // initialize indicator bit
    "li    x13, 0",          // initialize counter
"22:",
    "beq   x11, x15, 23f",   // if indicator and pin match, we found our number
    "srli  x11, x11, 1",     // shift indicator one over
    "addi  x13, x13, 1",     // increase counter
    "j 22b",
"23:", // stash information in memory
    "li    sp, 0xa00",       // set sp
    "sw    x14, 0(sp)",      // store rx pin location
    "sw    x15, 4(sp)",      // store tx pin location
    "sb    x12, 8(sp)",      // store rx shift counter
    "sb    x13, 12(sp)",     // store tx shift counter
    // prepare for first message
    "li    x29, 0b111010",   // clear error flags and transmitter active flag
    "mv    x22, x15",        // line idles high

/* Message Preparation
 *
 * The core blocks on FIFO2 until a message comes in, checks the IDE bit to see if it's a standard or exten-
 * ded frame and calculates the CRC. Finally, it extracts some length information needed for transmission and
 * stores message and CRC in memory. The length information remains in registers.
 *
 * Some notes:
 * - Using the table lookup to calculate the CRC requires left-padding so the last bit ends on a byte boun-
 *   dary. This means both types of frames get processed separately.
 * - Standard frames have all relevant information in the first message block (32 bits), extended frames have
 *   the DLC in the second block.
 * - Headers have a fixed structure but the data field can be 0-8 bytes long, so it needs a loop.
 * - The CRC is 15 bits long, so shift it one left for transmission.
 * - Because the CRC is stored in its own location in memory, it needs to be moved from the lower to the
 *   higher two bytes of a word. This is done by writing a half-word into the upper bytes of a word, then
 *   reading the full word. This saves a 16 <<.
 *
 * Information:
 *   SOF: start of frame, always a 0, added on transmission
 *   ID:  11-bit or 29-bit field
 *   IDE: distinguishes standard and extended frames
 *   SRR, RTR: control bits, not important here
 *   DLC: length of data field in bytes
 *        To get the # of bits in the data field, remove trailing zeroes and multiply by 8. This can be com-
 *        bined in one shift (shift right, then left by three).
 *   EOF: six recessive bits ("1")
 *   Arbitration field: sum of SOF, ID, IDE and RTR for standard frames, plus SRR for extended frames
 *   Rest of message: sum of control bits, data, and CRC
 *
 * Because CAN uses an unsual 15-bit polynom, here are some resources in case someone needs to work on this.
 *   - A general explanation with detailed worked examples
 *     http://www.sunshine2k.de/articles/coding/crc/understanding_crc.html
 *   - An explanation with arbitrary-width polynoms
 *     https://barrgroup.com/embedded-systems/how-to/crc-calculation-c-code
 *   - A calculator for arbitratry CRCs (also shows intermediate steps)
 *     https://rndtool.info/CRC-step-by-step-calculator/
 *     Be sure to include the (implied, usually omitted) leading "1" when entering the polynom.
 *   - The exact specification for this algorithm
 *     https://reveng.sourceforge.io/crc-catalogue/1-15.htm#crc.cat-bits.15
 *     The check mentioned in the link above takes ascii "123456789" (0x31, 0x32, ...) as input. If the
 *     implementation is correct, the output is 0x059E.
 *
 * Register usage
 *   x 1 table base
 *   x 2 sp
 *   x 3 message pointer (points to end)
 *   x 4
 *   x 5 message block (from FIFO2)
 *   x 6 mask for bit stuffing
 *   x 7
 *   x 8 length of arbitration field
 *   x 9 length of message after arbitration field
 *       length of data field
 *   x10 bits of data field not yet processed
 *   x11 mask for bits [14:0]
 *   x12 temp variable
 *   x13 temp variable
 *   x14 temp variable
 *   x15 crc remainder
 */

"30:", // wait for message
    "li    x1, 0x800",       // set table base
    "li    x3, 0xa10",       // set message pointer
    "li    x11, 0x7fff",     // mask for bits [14:0]
    // this instruction seems to cause problems, replace it with a load and a shift
    // "li    x15, 0x80000",    // mask for ide bit
    "li    x15, 0x40000",    // mask for ide bit
    "slli  x15, x15, 1",
    "mv    x22, x15",        // line idles high
    "li    x29, 0b10",       // clear transmitter active flag

    "mv    x5, x18",         // block for message
    "and   x14, x15, x5",    // read out ide bit
    "bnez  x14, 32f",        // 0 => std. frame, 1 => ext. frame

    // standard frame

    "li    x8, 12",          // store length of arbitration field
    "li    x14, 0x3c000",    // mask for dlc
    "and   x9, x14, x5",     // read out dlc
    "srli  x9, x9, 11",      // # of bits in data field (explanation above)
    "mv    x10, x9",         // copy length: x9 is persistent, x10 will count down during crc calculation
    // crc: first byte
    "srli  sp, x5, 30",      // get first byte to start crc, chosen so byte-wise lookup works
    "slli  sp, sp, 1",       // offset = index * 2 (there's 2 bytes / entry)
                             // combining both shifts would leave the last bit intact
    "add   sp, sp, x1",      // address = base + offset
    "lhu   x15, 0(sp)",      // look up constant
    // crc: second byte
    "li    x13, 0x3fc00000", // mask for next message byte
    "and   x13, x13, x5",    // extract next byte
    "srli  x13, x13, 22",    // remove trailing zeros
    "srli  x14, x15, 7",     // prepare remainder for xor
    "xor   x14, x14, x13",   // generate table index
    "slli  sp, x14, 1",      // offset = index * 2
    "add   sp, sp, x1",      // address = base + offset
    "lhu   x14, 0(sp)",      // look up constant
    "slli  x15, x15, 8",     // shift old crc
    "xor   x15, x15, x14",   // xor old crc and constant for new crc
    "and   x15, x15, x11",   // only keep bits [14:0]
    // crc: third byte
    "li    x13, 0x3fc000",   // same as above
    "and   x13, x13, x5",
    "srli  x13, x13, 14",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
"31:", // crc: fourth byte
    "beqz  x10, 34f",        // stop if all data has been processed
    "addi  x10, x10, -8",    // decrease counter
    "li    x13, 0x3fc0",     // process crc as above
    "and   x13, x13, x5",
    "srli  x13, x13, 6",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    // crc: fifth / first byte (straddles fifo registers)
    "beqz  x10, 34f",        // stop if all data has been processed
    "addi  x10, x10, -8",    // decrease counter
    "li    x13, 0x3f",       // mask for last bits of current message block
    "and   x13, x13, x5",    // extract bits
    "slli  x13, x13, 2",     // shift into position
    "sw    x5, 0(x3)",       // write current block into memory
    "addi  x3, x3, 4",       // increase pointer
    "mv    x5, x18",         // load next message block
    "srli  x12, x5, 30",     // extract next two bits
    "or    x13, x13, x12",   // combine both byte fragments
    "srli  x14, x15, 7",     // process crc as above
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    // crc: second byte
    "beqz  x10, 34f",        // same as above
    "addi  x10, x10, -8",
    "li    x13, 0x3fc00000",
    "and   x13, x13, x5",
    "srli  x13, x13, 22",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    // crc: third byte
    "beqz  x10, 34f",        // same as above
    "addi  x10, x10, -8",
    "li    x13, 0x3fc000",
    "and   x13, x13, x5",
    "srli  x13, x13, 14",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    "j 31b",                 // repeat until all data has been processed

    // extended frame

"32:", // crc: first byte
    "srli  sp, x5, 26",      // get first byte to start crc, chosen so byte-wise lookup works
    "slli  sp, sp, 1",       // offset = index * 2 (there's 2 bytes / entry)
                             // combining both shifts would leave the last bit intact
    "add   sp, sp, x1",      // address = base + offset
    "lhu   x15, 0(sp)",      // look up constant
    // crc: second byte
    "li    x13, 0x3fc0000",  // mask for next message byte
    "and   x13, x13, x5",    // extract next byte
    "srli  x13, x13, 18",    // remove trailing zeros
    "srli  x14, x15, 7",     // prepare remainder for xor
    "xor   x14, x14, x13",   // generate table index
    "slli  sp, x14, 1",      // offset = index * 2
    "add   sp, sp, x1",      // address = base + offset
    "lhu   x14, 0(sp)",      // look up constant
    "slli  x15, x15, 8",     // shift old crc
    "xor   x15, x15, x14",   // xor old crc and constant for new crc
    "and   x15, x15, x11",   // only keep bits [14:0]
    // crc: third byte
    "li    x13, 0x3fc00",    // same as above
    "and   x13, x13, x5",
    "srli  x13, x13, 10",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    // crc: fourth byte
    "li    x13, 0x3fc",
    "and   x13, x13, x5",
    "srli  x13, x13, 2",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    // crc: fifth / first byte (straddles fifo registers)
    "li    x13, 0x3",        // mask for last bits of current message block
    "and   x13, x13, x5",    // extract bits
    "slli  x13, x13, 6",     // shift into position
    "sw    x5, 0(x3)",       // write current block into memory
    "addi  x3, x3, 4",       // increase pointer
    "mv    x5, x18",         // load next message block
    "srli  x12, x5, 26",     // extract next six bits
    "or    x13, x13, x12",   // combine both byte fragments
    "srli  x14, x15, 7",     // process crc as above
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    // read dlc
    "li    x8, 32",          // length of arbitration field
    "li    x14, 0x3c000000", // mask for dlc
    "and   x9, x14, x5",     // read out dlc
    "srli  x9, x9, 23",      // # of bits in data field
    "mv    x10, x9",         // copy length: x9 is persistent, x10 will count down during crc calculation
"33:", // crc: second byte
    "beqz  x10, 34f",        // stop if all data has been processed
    "addi  x10, x10, -8",    // decrease counter
    "li    x13, 0x3fc0000",  // process crc as above
    "and   x13, x13, x5",
    "srli  x13, x13, 18",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    // crc: third byte
    "beqz  x10, 34f",        // same as above
    "addi  x10, x10, -8",
    "li    x13, 0x3fc00",
    "and   x13, x13, x5",
    "srli  x13, x13, 10",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    // crc: fourth byte
    "beqz  x10, 34f",        // same as above
    "addi  x10, x10, -8",
    "li    x13, 0x3fc",
    "and   x13, x13, x5",
    "srli  x13, x13, 2",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    // crc: fifth / first byte (straddles fifo registers)
    "beqz  x10, 34f",        // same as above
    "addi  x10, x10, -8",
    "li    x13, 0x3",
    "and   x13, x13, x5",
    "slli  x13, x13, 6",
    "sw    x5, 0(x3)",
    "addi  x3, x3, 4",
    "mv    x5, x18",
    "srli  x12, x5, 26",
    "or    x13, x13, x12",
    "srli  x14, x15, 7",
    "xor   x14, x14, x13",
    "slli  sp, x14, 1",
    "add   sp, sp, x1",
    "lhu   x14, 0(sp)",
    "slli  x15, x15, 8",
    "xor   x15, x15, x14",
    "and   x15, x15, x11",
    "j 33b",                 // continue until the data field has been processed

"34:", // end of calculation
    "sw    x5, 0(x3)",       // write current message block to memory
    "li    sp, 0xa22",       // addresss for crc
    "slli  x15, x15, 1",     // shift crc
    "sh    x15, 0(sp)",      // write crc to memory (see note)
    "addi  x9, x9, 6",       // add control bits to length counter

/* Transmission loop
 *
 * Sending starts with a SOF bit (always a "0"), followed by the arbitration field, the rest of the message,
 * the CRC, the ACK bit, and an EOF field. During sending, the core checks that the transmitted bit actually
 * gets put on the bus, stops if there's an error, and raises the correct error flag.
 *
 * Bit stuffing works by keeping a log of sent bits, including stuffing bits, and checking if the five most
 * recent ones are the same polarity. If they are, an opposite bit is inserted in the transmission transparent-
 * ly. The receiver removes those bits before processing.
 * Checks work by masking the five most recent bits (the register will be zero if all bits are zero), followed
 * by XORing the mask in again (this time the register will be zero if all initial bits are one).
 * An example checking four consecutive bits:
 *
 *                all 0s     both     all 1s
 *
 *  bitstream    00001001  10011001  11111001
 *  mask         11110000  11110000  11110000
 *  AND          00000000  10010000  11110000
 *  mask            --     11110000  11110000
 *  XOR             --     01100000  00000000
 *
 * This workflow doesn't change with a reversal of bit polarity, avoids awkward bit shifting during message
 * preparation) and handles register changes without any additional work.
 *
 * Some notes:
 * - The arbitration field ends on a register boundary for extended frames, so there's a check to see if a new
 *   message block needs to be loaded when starting to send the rest of the message.
 * - The CRC is stored as a half-word into the high bytes of a word, then loaded as a full word. This left
 *   aligns the 16 bits we need to send and saves one 16 <<.
 * - The CRC is followed by one recessive ("1") delimiter bit. It doesn't count toward bit stuffing, so send
 *   it separately.
 * - The bit stuffing log is initialized to (0b01 << 30): It's being fed from the left, the first bit is the
 *   SOF, and it needs to be followed by a "1" so it doesn't trigger falsely at the start.
 * - When recording a "1" as stuffing bit, the shift followed by an OR could be replaced by initializing the
 *   head of the log to 0b1000... becaues old values are not needed anyways. This would save one instruction
 *   but reduce clarity.
 * - Timing: line speed of 1 MHz gives us 700 cycles or ~150 instructions per bit. Longest bit takes ~50
 *   instructions.
 *
 * Reminder: Setup information, message and crc in memory, length information in registers.
 *   0xa00              Setup information
 *   0xa10              Message
 *   0xa20              CRC

 * Register usage
 *   x 1 mask for bit stuffing check
 *   x 2 sp
 *   x 3 bit stuffing log
 *   x 4 result of stuffing check
 *   x 5 data (current 32 bit word)
 *   x 6 bit-to-be-transmitted
 *       also used for as a counter for sampling three times per bit
 *   x 7 current bus state
 *   x 8 remaining bits of arbitration field
 *   x 9 remaining bits of message after arbitration field
 *   x10 remaining bits of current 32-bit word
 *   x11 mask to extract bit-to-be-transmitted
 *   x12 shift counter for rx pin
 *   x13 shift counter for tx pin
 *   x14 tx pin location
 *   x15 tx pin location
 *
 */

"40:", // sof and wait for line idle
    "li    x27, 0x1",        // set event sensitivity mask to line idle flag
    "mv    x0, x30",         // wait for line line idle flag
    "mv    x20, x0",         // wait for quantum
    "li    x29, 0b111000",   // clear transmitter error flags
    "li    x28, 0b10",       // set transmitter active flag
    "mv    x23, x0",         // sof: always a "0"

    // perform setup while sending sof
    "li    x1, 0xf8000000",  // set mask for bit stuffing check
    "li    sp, 0xa00",       // set sp to setup information
    "li    x3, 0x70000000",  // reset bit stuffing log
    "li    x11, 0x80000000", // set mask for bit-to-be-transmitted
    "lb    x12, 8(sp)",      // load rx shift counter
    "lb    x13, 12(sp)",     // load tx shift counter
    "lw    x14, 0(sp)",      // load rx pin location
    "lw    x15, 4(sp)",      // load tx pin location
    "li    x27, 0x200",      // set event sensitivity mask to sample flag
    "li    sp, 0xa10",       // set sp to start of message
    "mv    x20, x0",         // wait for quantum


"50:", // arbitration
    "li    x10, 32",         // 32 bits per register
    "lw    x5, 0(sp)",       // read next 32 bits of message
    "addi  sp, sp, 4",       // inc. sp
"51:",
    "and   x6, x5, x11",     // get next bit
    "srli  x3, x3, 1",       // shift bit stuffing log
    "or    x3, x3, x6",      // add new bit to log
    "srl   x6, x6, x13",     // shift bit into position
    "mv    x22, x6",         // pin high if "1"
    "mv    x23, x6",         // pin low if "0"
    "and   x6, x5, x11",     // get bit value again
    "srl   x6, x6, x12",     // shift bit into position for checking bus state
    // one sample per bit
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "bne   x7, x6, 100f",    // arbitration failed, end transmission, set error flag
/*  // three samples per bit, example code
    "mv    x0, x30",         // wait for signal to read the bus
    "li    x6, 0",           // initialize counter
    "and   x7, x21, x14",    // read current state of bus
    "beqz  x7, smp1",        // sample low => skip to next sample, don't inc counter
    "addi  x6, x6, 1",       // inc counter
"smp1:",
    "mv    x0, x30",         // repeat for second sample
    "and   x7, x21, x14",
    "beqz  x7, smp2",
    "addi  x6, x6, 1",
"smp2:",
    "mv    x0, x30",         // repeat for third sample
    "and   x7, x21, x14",
    "beqz  x7, smp3",
    "addi  x6, x6, 1",
"smp3:",
    "and   x7, x5, x11",     // get bit value again
    "bnez  x7, high",        // decide what's a pass
    "li    x7, 1",           // set threshold low (just sent a "0")
    "bgeu  x6, x7, 100f",    // {0,1} => pass, {2,3} => arbitration error
    "j endsmp",              // continue with transmission
"high:",
    "li    x7, 2",           // set threshold high (just sent a "1")
    "bgeu  x7, x6, 100f",    // {2,3} => pass, {0,1} => arbitration error
"endsmp:", */
    "slli  x5, x5, 1",       // shift msg
    "addi  x8, x8, -1",      // count bits sent (of arbitration field)
    "addi  x10, x10, -1",    // count bits sent (of current register)
    "and   x4, x3, x1",      // check stuffing log for all 0s in [31:27]
    "bnez  x4, 57f",         // not all 0s => do nothing
    "mv    x20, x0",         // otherwise wait for quantum
    "mv    x22, x15",        // insert "1" into transmission
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "beqz  x7, 100f",        // arbitration failure during bit stuffing => set error flag
    "srli  x3, x3, 1",       // shift log
    "or    x3, x3, x11",     // append "1" to log
    "j 58f",                 // and continue
"57:",
    "xor   x4, x4, x1",      // check stuffing log for all 1s in [31:27]
    "bnez  x4, 58f",         // not all 1s => do nothing
    "mv    x20, x0",         // otherwise wait for quantum
    "mv    x23, x0",         // insert "0" into transmission
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "bnez  x7, 100f",        // arbitration failure during bit stuffing => set error flag
    "srli  x3, x3, 1",       // shift log and append "0" to log
"58:",
    "mv    x20, x0",         // wait for quantum
    "beqz  x8, 60f",         // all arbitration bits sent => send remainder of the message
    "beqz  x10, 50b",        // 32 bits sent => read next 32 bits
    "j 51b",                 // continue until all arbitration bits have been sent


"60:", // remainder of message
    "bnez  x10, 62f",        // all bits in current block sent => load new block
"61:",
    "li    x10, 32",         // 32 bits per register
    "lw    x5, 0(sp)",       // read next 32 bits of message

    "addi  sp, sp, 4",       // inc. sp
"62:",
    "and   x6, x5, x11",     // get next bit
    "srli  x3, x3, 1",       // shift bit stuffing log
    "or    x3, x3, x6",      // add new bit to log
    "srl   x6, x6, x13",     // shift bit into position
    "mv    x22, x6",         // pin high if "1"
    "mv    x23, x6",         // pin low if "0"
    "and   x6, x5, x11",     // get bit value again
    "srl   x6, x6, x12",     // shift bit into position for checking bus state
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "bne   x7, x6, 101f",    // bit check failed, end transmission
    "slli  x5, x5, 1",       // shift msg
    "addi  x9, x9, -1",      // count bits sent (of remaining message)
    "addi  x10, x10, -1",    // count bits sent (of current register)
    "and   x4, x3, x1",      // check stuffing log for all 0s in [31:27]
    "bnez  x4, 68f",         // not all 0s => do nothing
    "mv    x20, x0",         // otherwise wait for quantum
    "mv    x22, x15",        // insert "1" into transmission
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "beqz  x7, 101f",        // bit error during bit stuffing => set error flag
    "srli  x3, x3, 1",       // shift log
    "or    x3, x3, x11",     // append "1" to log
    "j 69f",                 // and continue
"68:",
    "xor   x4, x4, x1",      // check stuffing log for all 1s in [31:27]
    "bnez  x4, 69f",         // not all 1s => do nothing
    "mv    x20, x0",         // otherwise wait for quantum
    "mv    x23, x0",         // insert "0" into transmission
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "bnez  x7, 101f",        // bit error during bit stuffing => set error flag
    "srli  x3, x3, 1",       // shift log and append "0" to log
"69:",
    "mv    x20, x0",         // wait for quantum
    "beqz  x9, 70f",         // all message bits sent => send crc
    "beqz  x10, 61b",        // 32 bits sent => read next 32 bits
    "j 62b",                 // continue until all remaining bits have been sent

"70:", // crc
    "li    sp, 0xa20",       // set sp
    "lw    x5, 0(sp)",       // load crc into x5 (see note at start of section)
    "li    x8, 15",          // crc is 15 bits
"71:",
    "and   x6, x5, x11",     // get next bit
    "srli  x3, x3, 1",       // shift bit stuffing log
    "or    x3, x3, x6",      // add new bit to log
    "srl   x6, x6, x13",     // shift bit into position
    "mv    x22, x6",         // pin high if "1"
    "mv    x23, x6",         // pin low if "0"
    "and   x6, x5, x11",     // get bit value again
    "srl   x6, x6, x12",     // shift bit into position for checking bus state
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "bne   x7, x6, 101f",    // bit check failed, end transmission
    "slli  x5, x5, 1",       // shift crc
    "addi  x8, x8, -1",      // count bits sent (of crc)
    "and   x4, x3, x1",      // check stuffing log for all 0s in [31:27]
    "bnez  x4, 76f",         // not all 0s => do nothing
    "mv    x20, x0",         // otherwise wait for quantum
    "mv    x22, x15",        // insert "1" into transmission
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "beqz  x7, 101f",        // bit error during bit stuffing => set error flag
    "srli  x3, x3, 1",       // shift log
    "or    x3, x3, x11",     // append "1" to log
    "j 77f",                 // and continue
"76:",
    "xor   x4, x4, x1",      // check stuffing log for all 1s in [31:27]
    "bnez  x4, 77f",         // not all 1s => do nothing
    "mv    x20, x0",         // otherwise wait for quantum
    "mv    x23, x0",         // insert "0" into transmission
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "bnez  x7, 101f",        // bit error during bit stuffing => set error flag
    "srli  x3, x3, 1",       // shift log and append "0" to log
"77:",
    "mv    x20, x0",         // wait for quantum
    "beqz  x8, 80f",         // all crc bits sent => send crc delimiter
    "j 71b",

"80:", // crc delimiter bit
    "mv    x22, x15",        // bit is recessive ("1")
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus, should always be "1"
    "beqz  x7, 101f",        // bit check failed if "0" => end transmission
    "mv    x20, x0",         // wait for quantum

"90:", // ack bit
                             //pin already high from crc delimiter
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "bnez  x7, 102f",        // bus high => no ACK bit received => ACK error
    "mv    x20, x0",         // ack bit ends
    "mv    x22, x15",        // pin high
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "beqz  x7, 101f",        // bus low => bit error
    "mv    x20, x0",         // ack delimiter ends

    // end of frame bits
    "mv    x0, x30",         // wait for signal to read the bus
    "and   x7, x21, x14",    // read current state of bus
    "beqz  x7, 101f",        // bus low => bit error
    "mv    x20, x0",         // end of bit 1
    "mv    x0, x30",         // repeat
    "and   x7, x21, x14",
    "beqz  x7, 101f",
    "mv    x20, x0",         // end of bit 2
    "mv    x0, x30",
    "and   x7, x21, x14",
    "beqz  x7, 101f",
    "mv    x20, x0",         // end of bit 3
    "mv    x0, x30",
    "and   x7, x21, x14",
    "beqz  x7, 101f",
    "mv    x20, x0",         // end of bit 4
    "mv    x0, x30",
    "and   x7, x21, x14",
    "beqz  x7, 101f",
    "mv    x20, x0",         // end of bit 5
    "mv    x0, x30",
    "and   x7, x21, x14",
    "beqz  x7, 101f",
    "mv    x20, x0",         // end of bit 6
    "j 110f",                // end of transmission


    // errors

"100:", // arbitration error
    "li    x28, 0x8",        // set arbitration error flag
    "j 110f",                // end of transmission

"101:", // bit error
    "li    x28, 0x10",       // set bit error flag
    "j 110f",                // end of transmission

"102:", // ack error
    "li    x28, 0x20",       // set ack error flag
    "j 110f",                // end of transmission

/* End of transmission
 *
 * Nothing happens here, the instruction only provides a common end point for all code flows. Line setup -
 * pulling the line high, clearing tx active flag - happens at the start of the code block.
 *
 */

"110:", // end of transmission
    "j 30b"                  // wait for next message
);

#[rustfmt::skip]
bio_code!(can_bus_clk_kernel_three_samples, CAN_BUS_CLK_START_THREE, CAN_BUS_CLK_END_THREE,
/* -------------------------------------------------------------------------------
 *
 *                                    CLOCK
 *
 * -------------------------------------------------------------------------------
 *
 * This core is identical to the one sampling once per bit, with two differences: it samples the bus three
 * times in rapid succession before determining an edge and it emits three sampling pulses per bit time.
 *
 * It's considerably slower than the single-sample code. With the sampling delay set to ~20ns, it can probably
 * reach up to 5 MHz for each quantum or 10 TQ at a line speed of 500 kHz.
 *
 * Register usage
 *   x 1 mask for eof and line idle flags
 *   x 2 sample counter (three samples per bit)
 *   x 3 threshold (three samples per bit)
 *   x 4 phase error
 *   x 5 1st sample point (three samples per bit)
 *   x 6 2st sample point (three samples per bit)
 *   x 7 temp variable for flag checks
 *   x 8 loop counter (tracks current quantum)
 *   x 9 halfway point (1/2 of total quanta)
 *   x10 quanta before sampling point
 *   x11 quanta per bit
 *   x12 SJW
 *   x13 bus state
 *   x14 bus state
 *   x15 rx pin location
 *
 */

    // setup

    "mv    x15, x19",        // store rx pin location
    "mv    x11, x19",        // store quanta per bit
    "mv    x10, x19",        // store (quanta before) sampling point
    "mv    x12, x19",        // store SWJ

    "li    x1, 0x1001",      // mask for eof and line idle flags
    "srli  x9, x11, 1",      // halfway point = # of quanta / 2
    "li    x8, 0",           // initialize counter
    "li    x3, 2",           // set threshold (for three samples / bit)
    "addi  x6, x10, -1",     // calculate 2nd sample point (for three samples / bit)
    "addi  x5, x6, -1",      // calculate 1st sample point (for three samples / bit)

    // line idle

"10:",
    "li    x27, 6",          // set event sensitivity mask to receiver or transmitter active flags
    "mv    x0, x30",         // start synchronizing when either core is active
    "li    x27, 0",          // disable event sensitivity mask to enable non-blocking reads

    // quantum loop

"20:",
    "mv    x20, x0",         // wait for quantum
    "mv    x13, x14",        // save old sample
    "li    x2, 0",           // set threshold
    "and   x14, x15, x21",   // sample bus
    "beqz  x14, 21f",        // low => skip to next sample, do not increase counter
    "add   x2, x2, 1",       // else increase counter
"21:",
    "slli  x0, x0, 31",      // sampling delay ~20ns
    "and   x14, x15, x21",   // repeat for second sample
    "slli  x0, x0, 30",
    "beqz  x14, 22f",
    "add   x2, x2, 1",
"22:",
    "and   x14, x15, x21",   // repeat for third sample
    "slli  x0, x0, 30",
    "beqz  x14, 23f",
    "add   x2, x2, 1",
"23:",
    "bgeu  x3, x2, 24f",     // compare counter to threshold: {0,1} => low, {2,3} => high
    "li    x14, 1",          // set bus state to high
    "j 30f",
"24:",
    "li x14, 0",             // set bus state to low

    // edge detected?
"30:",
    "beq   x13, x14, 40f",   // no => continue

    // phase error handling

    "mv    x20, x0",         // wait for quantum           spread error handling across two quanta
    "bgeu  x9, x8, 32f",     // early or late edge?        halfway point > current quantum? yes => late edge

    // early edge
    "sub   x4, x11, x8",     // calculate phase error      error = total quanta - current quantum
    "bltu  x12, x4, 31f",    // phase error withing SJW?   error > SJW => 21f
    // phase error ≤ SJW
    "mv    x8, x0",          // synchronize                reset loop counter
    "addi  x8, x8, 1",       //                            increase quantum counter
    "j 40f",                 //                            continue
    // phase error > SJW
"31:",
    "add   x8, x8, x12",     // synchronize                advance loop by SJW (but not by full phase error)
    "addi  x8, x8, 1",       //                            increase quantum counter
    "j 40f",                 //                            continue

    // late edge
"32:",
                             // calculate phase error      same as current loop counter
    "bltu  x12, x8, 33f",    // phase error withing SJW?   error > SJW => 23f
    // phase error ≤ SJW
    "mv    x8, x0",          // synchronize                reset loop counter
    "addi  x8, x8, 1",       //                            increase quantum counter
    "j 40f",                 //                            continue
"33:",
    // phase error > SJW
    "sub   x8, x8, x12",     // synchronize                retard loop by SJW (but not by full phase error)
    "addi  x8, x8, 1",       //                            increase quantum counter
    "j 40f",                 //                            continue

    // signal sampling time

"40:",
    "addi  x8, x8, 1",       // increase counter for TQ
    "bne   x8, x5, 41f",     // check for first sampling time (no => continue)
    "li    x28, 0x200",      // set sample flag
    "li    x29, 0x200",      // clear sample flag
    "j 20b",
"41:",
    "bne   x8, x6, 42f",     // check if second sampling time (no => continue)
    "li    x28, 0x200",      // set sample flag
    "li    x29, 0x200",      // clear sample flag
    "j 20b",
"42:",
    "bne   x8, x10, 43f",    // check if third sampling time (no => continue)
    "li    x28, 0x200",      // set sample flag
    "li    x29, 0x200",      // clear sample flag
    "and   x7, x1, x30",     // check eof and line idle flags
    "bnez  x7, 50f",         // if set => 40f
    "j 20b",

    // end of bit bookkeeping

"43:",
    "bltu   x8, x11, 20b",   // check if full bit time is up
    "mv    x8, x0",          // bit time is up, reset counter
    "j 20b",

    // EOF signals

"50:",
    "mv    x20, x0",         // wait for quantum
    "and   x7, x30, 1",      // check line idle flag
    "bnez  x7, 10b",         // line idle => wait for next SOF
    "addi  x8, x8, 1",       // increase quantum counter
    "bne   x8, x5, 51f",     // check for first sampling time (no => continue)
    "li    x28, 0x200",      // set sample flag
    "li    x29, 0x200",      // clear sample flag
    "j 40b",
"51:",
    "bne   x8, x6, 52f",     // check if second sampling time (no => continue)
    "li    x28, 0x200",      // set sample flag
    "li    x29, 0x200",      // clear sample flag
    "j 40b",
"52:",
    "bne   x8, x10, 53f",    // check if third sampling time (no => continue)
    "li    x28, 0x200",      // set sample flag
    "li    x29, 0x200",      // clear sample flag
    "j 50b",
"53:",
    "bne   x8, x11, 50b",    // check if full bit time is up
    "li    x28, 0x400",      // set bit boundary flag
    "li    x29, 0x400",      // clear bit boundary flag
    "mv    x8, x0",          // bit time is up, reset counter
    "j 50b"
);
