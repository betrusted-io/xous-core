use crate::*;
use utralib::utra::rp_pio;
use super::report_api;

const SIM_SCALE_FACTOR: f32 = 200.0; // speedup over real time to have simulation finish in a reasonable amount of time
const BURST_PERIOD: f32 =  562.5e-6 / SIM_SCALE_FACTOR;
const CARRIER_PERIOD: f32 = 38.222e3 * SIM_SCALE_FACTOR;
pub fn carrier_burst_init(pio_sm: &mut PioSm, program: &LoadedProg, pin: usize, freq: f32) {
    pio_sm.sm_set_enabled(false);
    program.setup_default_config(pio_sm);

    // Map the SET pin group to one pin, namely the `pin`
    // parameter to this function.
    //
    pio_sm.config_set_set_pins (pin, 1);


    // Set the pin direction to output at the PIO
    pio_sm.sm_set_pindirs_with_mask(pin, 1);

    // Set the clock divider to generate the required frequency
    //
    const TICKS_PER_LOOP: f32 = 4.0;
    let div = 200.0e6 / (freq * TICKS_PER_LOOP as f32);
    pio_sm.config_set_clkdiv(div);

    // SIMULATION: setup side set for relaying the demodulated data to the Rx
    pio_sm.config_set_sideset_pins(31);

    // Apply the configuration to the state machine
    //
    pio_sm.sm_init(program.entry());

    // Set the state machine running
    //
    pio_sm.sm_set_enabled(true);
}

pub fn carrier_control_init(pio_sm: &mut PioSm, program: &LoadedProg, tick_rate: f32, bits_per_frame: usize) {
    pio_sm.sm_set_enabled(false);
    program.setup_default_config(pio_sm);

    // configure the output shift register
    //
    pio_sm.config_set_out_shift(
        true,       // shift right
        false,      // disable autopull
        bits_per_frame);

    // join the FIFOs to make a single large transmit FIFO
    //
    pio_sm.config_set_fifo_join(PioFifoJoin::JoinTx);

    // configure the clock divider
    //
    let div: f32 = 200.0e6 / tick_rate;
    pio_sm.config_set_clkdiv(div);

    // apply the configuration to the state machine
    //
    pio_sm.sm_init(program.entry());

    // set the state machine running
    //
    pio_sm.sm_set_enabled(true);
}

pub fn nec_tx_init(pio_ss: &mut PioSharedState, pin_num: usize) -> PioSm {
    let mut burst_sm = pio_ss.alloc_sm().unwrap();
    let mut control_sm = pio_ss.alloc_sm().unwrap();

    let carrier_burst_prog = pio_proc::pio_asm!(
        // Generate bursts of carrier.
        //
        // Repeatedly wait for an IRQ to be set then clear it and generate 21 cycles of
        // carrier with 25% duty cycle
        //
        ".side_set 1 opt pindirs",             // SIMULATION loopback
        ".define NUM_CYCLES 21              ", // how many carrier cycles to generate
        ".define BURST_IRQ 7                ", // which IRQ should trigger a carrier burst
        ".define public TICKS_PER_LOOP 4    ", // the number of instructions in the loop (for timing)

        ".wrap_target                       ",
        "    set x, (NUM_CYCLES - 1)        ", // initialize the loop counter
        "    wait 1 irq BURST_IRQ     side 0", // wait for the IRQ then clear it
        "cycle_loop:                        ",
        "    set pins, 1              side 1", // set the pin high (1 cycle)
        "    set pins, 0 [1]                ", // set the pin low (2 cycles)
        "    jmp x--, cycle_loop            ", // (1 more cycle)
        ".wrap                              ",
    );

    let carrier_control_prog = pio_proc::pio_asm!(
        // Transmit an encoded 32-bit frame in NEC IR format.
        //
        // Accepts 32-bit words from the transmit FIFO and sends them least-significant
        // using pulse position modulation.
        //
        // Carrier bursts are generated using the nec_carrier_burst program, which is ex
        // running on a separate state machine.
        //
        // This program expects there to be 2 state machine ticks per 'normal' 562.5us
        // burst period.
        //
        ".define BURST_IRQ 7                    ", // the IRQ used to trigger a carrier burst
        ".define NUM_INITIAL_BURSTS 16          ", // how many bursts to transmit for a 'sync burst'

        ".wrap_target",
        "    pull                               ", // fetch a data word from the transmit FIFO into the
        "                                       ", // output shift register, blocking if the FIFO is empty

        "    set x, (NUM_INITIAL_BURSTS - 1)    ", // send a sync burst (9ms)
        "long_burst:",
        "    irq BURST_IRQ",
        "    jmp x-- long_burst",

        "    nop [15]                           ", // send a 4.5ms space
        "    irq BURST_IRQ [1]                  ", // send a 562.5us burst to begin the first data bit

        "data_bit:",
        "    out x, 1                           ", // shift the least-significant bit from the OSR
        "    jmp !x burst                       ", // send a short delay for a '0' bit
        "    nop [3]                            ", // send an additional delay for a '1' bit
        "burst:",
        "    irq BURST_IRQ                      ", // send a 562.5us burst to end the data bit

        "jmp !osre data_bit                     ", // continue sending bits until the OSR is empty

        ".wrap                                  ", // fetch another data word from the FIFO
    );
    let carrier_prog = LoadedProg::load(carrier_burst_prog.program, pio_ss).unwrap();
    carrier_burst_init(&mut burst_sm, &carrier_prog, pin_num, CARRIER_PERIOD);

    let control_prog = LoadedProg::load(carrier_control_prog.program, pio_ss).unwrap();
    carrier_control_init(
        &mut control_sm,
        &control_prog,
        2.0 * (1.0 / BURST_PERIOD),
        32
    );
    control_sm
}

// Create a frame in `NEC` format from the provided 8-bit address and data
//
// Returns: a 32-bit encoded frame
pub fn nec_encode_frame(address: u8, data: u8) -> u32{
    // a normal 32-bit frame is encoded as address, inverted address, data, inverse data,
    address as u32 | (address as u32 ^ 0xff) << 8 | (data as u32) << 16 | (data as u32 ^ 0xff) << 24
}

pub fn nec_receive_init(pio_sm: &mut PioSm, program: &LoadedProg, pin: usize, burst_period: f32) {
    pio_sm.sm_set_enabled(false);
    program.setup_default_config(pio_sm);

    // configure the Input Shift Register
    //
    pio_sm.config_set_in_shift(
                            true,       // shift right
                            true,       // enable autopush
                            32);        // autopush after 32 bits

    // join the FIFOs to make a single large receive FIFO
    //
    pio_sm.config_set_fifo_join(PioFifoJoin::JoinRx);

    // Map the IN pin group to one pin, namely the `pin`
    // parameter to this function.
    //
    pio_sm.config_set_in_pins(pin);

    // Map the JMP pin to the `pin` parameter of this function.
    //
    pio_sm.config_set_jmp_pin(pin);

    // Set the clock divider to 10 ticks per 562.5us burst period
    //
    let div: f32 = 200.0e6 / (10.0 / burst_period);
    pio_sm.config_set_clkdiv(div);

    // Apply the configuration to the state machine
    //
    pio_sm.sm_init(program.entry());

    // Set the state machine running
    //
    pio_sm.sm_set_enabled(true);
}

pub fn nec_rx_init(pio_ss: &mut PioSharedState, pin_num: usize) -> PioSm {
    let mut rx_sm = pio_ss.alloc_sm().unwrap();

    let nec_receive = pio_proc::pio_asm!(
        // Decode IR frames in NEC format and push 32-bit words to the input FIFO.
        //
        // The input pin should be connected to an IR detector with an 'active low' output.
        //
        // This program expects there to be 10 state machine clock ticks per 'normal' 562.5us burst period
        // in order to permit timely detection of start of a burst. The initialization function below sets
        // the correct divisor to achieve this relative to the system clock.
        //
        // Within the 'NEC' protocol frames consists of 32 bits sent least-significant bit first; so the
        // Input Shift Register should be configured to shift right and autopush after 32 bits, as in the
        // initialization function below.
        //
        ".define BURST_LOOP_COUNTER 30                 ",  // the detection threshold for a 'frame sync' burst
        ".define BIT_SAMPLE_DELAY 15                   ",  // how long to wait after the end of the burst before sampling

        ".wrap_target",

        "next_burst:",
        "    set x, BURST_LOOP_COUNTER                 ",  // 7
        "    wait 0 pin 0                              ",  // 8 wait for the next burst to start

        "burst_loop:",
        "    jmp pin data_bit                          ",  // 9 the burst ended before the counter expired
        "    jmp x-- burst_loop                        ",  // A wait for the burst to end

        "                                              ",  // the counter expired - this is a sync burst
        "    mov isr, null                             ",  // B reset the Input Shift Register
        "    wait 1 pin 0                              ",  // C wait for the sync burst to finish
        "    jmp next_burst                            ",  // D wait for the first data bit

        "data_bit:",
        "    nop [ BIT_SAMPLE_DELAY - 1 ]              ",  // E wait for 1.5 burst periods before sampling the bit value
        "    in pins, 1                                ",  // F if the next burst has started then detect a '0' (short gap)
        "                                              ",  // otherwise detect a '1' (long gap)
        "                                              ",  // after 32 bits the ISR will autopush to the receive FIFO
        ".wrap                                         ",
    );
    let rx_prog = LoadedProg::load(nec_receive.program, pio_ss).unwrap();
    nec_receive_init(&mut rx_sm, &rx_prog, pin_num, BURST_PERIOD);

    rx_sm
}


// Validate a 32-bit frame and store the address and data at the locations
// provided.
//
// Returns: `true` if the frame was valid, otherwise `false`
pub fn nec_decode_frame(frame: u32, addr: &mut u8, d: &mut u8) -> bool {

    let [address, inverted_address, data, inverted_data] = frame.to_le_bytes();

    // a valid (non-extended) 'NEC' frame should contain 8 bit
    // address, inverted address, data and inverted data
    if address != (inverted_address ^ 0xff) ||
        data != (inverted_data ^ 0xff) {
        return false;
    }

    // store the validated address and data
    *addr = address;
    *d = data;

    true
}

pub fn nec_ir_loopback_test() {
    const TX_GPIO: usize = 14;
    const RX_GPIO: usize = 31;

    report_api(0x1300_0000);

    let mut pio_ss = PioSharedState::new();
    pio_ss.pio.wo(rp_pio::SFR_IO_I_INV, 0x8000_0000); // invert the input to emulate hardware behavior

    let mut tx_sm = nec_tx_init(&mut pio_ss, TX_GPIO);
    report_api(0x1300_0001);
    let mut rx_sm = nec_rx_init(&mut pio_ss, RX_GPIO);
    report_api(0x1300_0002);

    let tx_frame = nec_encode_frame(0xAA, 0x33);
    report_api(0x1300_0003);
    report_api(tx_frame);
    tx_sm.sm_put_blocking(tx_frame);
    report_api(0x1300_0004);

    // insert a delay of some sort here to wait for the loop back

    let rx_frame = rx_sm.sm_get_blocking();
    report_api(0x1300_0005);
    report_api(rx_frame);
    let mut addr: u8 = 0;
    let mut data: u8 = 0;
    assert!(nec_decode_frame(rx_frame, &mut addr, &mut data));
    assert!(addr == 0xAA);
    assert!(data == 0x33);

    pio_ss.pio.wo(rp_pio::SFR_IO_I_INV, 0x0000_0000); // cleanup the inversion for next tests

    report_api(0x1300_600D);
}

