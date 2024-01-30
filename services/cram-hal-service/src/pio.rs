use cramium_hal::iox;
use xous_pio::*;

/// This is the start of the memory LCD driver program, but aborted because we now
/// realize the UDMA SPI block is much better suited for this task.
fn pio_memlcd_program() {
    #[rustfmt::skip]
    let memlcd_line = pio_proc::pio_asm!(
        // The PIO engine is configured as a simple 16-bit, LSB-first shift register
        // to send data to the LCD at a rate of 2 MHz, requesting a new word via DMA
        // whenever the shift register is empty. It takes 2 PIO cycles to send one
        // bit, so the PIO engine should be configure to run at 4 MHz.
        //
        // In addition to the shift function, the PIO asserts the chip select signal
        // high for 12 cycles (3us @ 4MHz) before the first word starts shifting
        //
        // At the end of the DMA transfer, CS & clock should be dropped manually.
        ".side_set 2 opt",

        ".define CS_SETUP_DELAY 12            ",  // setup time for CS
        "setup_line:",
        "    set y, CS_SETUP_DELAY      side 0",
        "setup_cs:",
        "    jmp y--  setup_cs          side 1",  // respect setup time
        ".wrap_target",
        "    out pins, 1                side 1",  // assert data/clock low
        "    nop                        side 3",  // clock high
        ".wrap",
        // after data transfer is done, clock and then CS should be dropped manually.
    );
}
