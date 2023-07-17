use crate::*;
use super::report_api;

pub fn adder_test() -> bool {
    report_api(0x5030_0000);

    let mut pio_ss = PioSharedState::new();

    let mut pio_sm = pio_ss.alloc_sm().unwrap();

    let adder_prog = pio_proc::pio_asm!(
        // Pop two 32 bit integers from the TX FIFO, add them together, and push the
        // result to the TX FIFO. Autopush/pull should be disabled as we're using
        // explicit push and pull instructions.
        //
        // This program uses the two's complement identity x + y == ~(~x - y)
        "    pull        ",
        "    mov x, ~osr ",
        "    pull        ",
        "    mov y, osr  ",
        "    jmp test    ",    // this loop is equivalent to the following C code:
        "incr:           ",    // while (y--)
        "    jmp x-- test",    //     x--;
        "test:           ",    // This has the effect of subtracting y from x, eventually.
        "    jmp y-- incr",
        "    mov isr, ~x ",
        "    push        ",
    );

    let prog_adder = LoadedProg::load(adder_prog.program, &mut pio_ss).unwrap();
    pio_sm.sm_set_enabled(false);
    prog_adder.setup_default_config(&mut pio_sm);
    pio_sm.sm_init(prog_adder.entry());
    pio_sm.sm_set_enabled(true);

    let mut state: u16 = 0x25;
    for _ in 0..10 {
        state = crate::lfsr_next(state);
        let a = state % 100;
        state = crate::lfsr_next(state);
        let b = state % 100;
        report_api(0x5030_0000 | a as u32 | ((b as u32) << 8));
        pio_sm.sm_put_blocking(a as u32);
        pio_sm.sm_put_blocking(b as u32);
        let sum = pio_sm.sm_get_blocking();
        assert!(sum == a as u32 + b as u32);
        report_api(0x5030_0000 | sum as u32);
    }
    report_api(0x5030_600D);
    // loop panics if the sum doesn't work
    true
}