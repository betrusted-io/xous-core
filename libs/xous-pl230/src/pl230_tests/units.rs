use utralib::*;

use super::report_api;
use crate::*;

/// used to generate some test vectors
pub fn lfsr_next(state: u32) -> u32 {
    let bit = ((state >> 31) ^ (state >> 21) ^ (state >> 1) ^ (state >> 0)) & 1;

    (state << 1) + bit
}

pub fn basic_tests(pl230: &mut Pl230) -> bool {
    report_api("channels", pl230.csr.rf(utra::pl230::STATUS_CHNLS_MINUS1) + 1);
    report_api("id0", pl230.csr.r(utra::pl230::PERIPH_ID_0));
    report_api("id1", pl230.csr.r(utra::pl230::PERIPH_ID_1));
    report_api("id2", pl230.csr.r(utra::pl230::PERIPH_ID_2));

    // conjure the DMA control structure in IFRAM0. In order to guarantee Rust
    // semantics, it must be initialized to 0: 4 word-sized entries * 8 channels * 2 banks = 4 * 8 * 2
    let init_ptr = utralib::HW_IFRAM0_MEM as *mut u32;
    for i in 0..(4 * 8 * 2) {
        unsafe { init_ptr.add(i).write_volatile(0) };
    }
    // safety: we guarantee that the pointer is aligned and initialized
    let cc_struct: &mut ControlChannels =
        unsafe { (utralib::HW_IFRAM0_MEM as *mut ControlChannels).as_mut().unwrap() };

    // read the status register
    report_api("status", pl230.csr.r(utra::pl230::STATUS));
    pl230.csr.wfo(utra::pl230::CFG_MASTER_ENABLE, 1); // enable
    report_api("status after enable", pl230.csr.r(utra::pl230::STATUS));

    const DMA_LEN: usize = 16;
    // setup the PL230 to do a simple transfer between two memory regions
    // dma_mainram feature will cause us to DMA between main memory regions. This works under RTL sims.
    #[cfg(feature = "dma_mainram")]
    let mut region_a = [0u32; DMA_LEN];
    #[cfg(feature = "dma_mainram")]
    let region_b = [0u32; DMA_LEN];
    // The alternate is to DMA between IFRAM regions. This works under FPGA and RTL sim.
    #[cfg(not(feature = "dma_mainram"))]
    let region_a =
        unsafe { core::slice::from_raw_parts_mut((utralib::HW_IFRAM0_MEM + 4096) as *mut u32, DMA_LEN) };
    #[cfg(not(feature = "dma_mainram"))]
    let region_b = unsafe { core::slice::from_raw_parts_mut(utralib::HW_IFRAM1_MEM as *mut u32, DMA_LEN) };
    let mut state = 0x1111_1111;
    for d in region_a.iter_mut() {
        *d = state;
        state = lfsr_next(state);
    }

    cc_struct.channels[0].dst_end_ptr = unsafe { region_b.as_ptr().add(region_b.len() - 1) } as u32;
    cc_struct.channels[0].src_end_ptr = unsafe { region_a.as_ptr().add(region_a.len() - 1) } as u32;
    let mut cc = DmaChanControl(0);
    cc.set_src_size(DmaWidth::Word as u32);
    cc.set_src_inc(DmaWidth::Word as u32);
    cc.set_dst_size(DmaWidth::Word as u32);
    cc.set_dst_inc(DmaWidth::Word as u32);
    cc.set_r_power(ArbitrateAfter::Xfer1024 as u32);
    cc.set_n_minus_1(region_a.len() as u32 - 1);
    cc.set_cycle_ctrl(DmaCycleControl::AutoRequest as u32);
    cc_struct.channels[0].control = cc.0;

    pl230.csr.wo(utra::pl230::CTRLBASEPTR, cc_struct.channels.as_ptr() as u32);
    pl230.csr.wo(utra::pl230::CHNLREQMASKSET, 1);
    pl230.csr.wo(utra::pl230::CHNLENABLESET, 1);

    // report_api("dma_len", DMA_LEN as u32);
    report_api("baseptr", cc_struct.channels.as_ptr() as u32);
    report_api("src start", region_a.as_ptr() as u32);
    // report_api("baseptr[0]", unsafe{cc_struct.channels.as_ptr().read()}.src_end_ptr);
    report_api("dst start", region_b.as_ptr() as u32);
    // report_api("baseptr[1]", unsafe{cc_struct.channels.as_ptr().read()}.dst_end_ptr);
    // report_api("baseptr[2]", unsafe{cc_struct.channels.as_ptr().read()}.control);
    // report_api("baseptr[3]", unsafe{cc_struct.channels.as_ptr().read()}.reserved);
    // report_api("baseptr reg", pl230.csr.r(utra::pl230::CTRLBASEPTR));

    // this should kick off the DMA
    pl230.csr.wo(utra::pl230::CHNLSWREQUEST, 1);

    let mut timeout = 0;
    while (DmaChanControl(cc_struct.channels[0].control).cycle_ctrl() != 0) && timeout < 16 {
        // report_api("dma progress ", cc_struct.channels[0].control);
        report_api("progress as baseptr[2]", unsafe { cc_struct.channels.as_ptr().read() }.control);
        timeout += 1;
    }

    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
        ".word 0x500F",
        "nop",
        "nop",
        "nop",
        "nop",
        "nop",
    );
    }

    // manual flushing, as a sanity check of cache flush if needed
    /*
    {
        let flush_ptr = 0x6100_0000 as *mut u32;
        let mut dummy: u32 = 0;
        // read a bunch of data to ensure the cache is flushed
        for i in 0..131072 {
            dummy += unsafe{flush_ptr.add(i).read_volatile()};
        }
        report_api("dummy: ", dummy);
    } */

    let mut passing = true;
    let mut errs = 0;
    for (i, (src, dst)) in region_a.iter().zip(region_b.iter()).enumerate() {
        if *src != *dst {
            report_api("error in iter ", i as u32);
            report_api("src: ", *src);
            report_api("dst: ", *dst);
            passing = false;
            errs += 1;
        }
    }
    report_api("basic dma result (1=pass)", if passing { 1 } else { 0 });
    report_api("errs: ", errs);
    passing
}

#[cfg(feature = "pio")]
pub fn pio_test(pl230: &mut Pl230) -> bool {
    use cramium_hal::iox;
    use xous_pio::*;

    report_api("channels", pl230.csr.rf(utra::pl230::STATUS_CHNLS_MINUS1) + 1);
    report_api("id0", pl230.csr.r(utra::pl230::PERIPH_ID_0));
    report_api("id1", pl230.csr.r(utra::pl230::PERIPH_ID_1));
    report_api("id2", pl230.csr.r(utra::pl230::PERIPH_ID_2));

    let mut iox = iox::Iox::new(utralib::generated::HW_IOX_BASE as *mut u32);
    let pin = iox.set_pio_bit_from_port_and_pin(iox::IoxPort::PB, 15).unwrap();
    report_api("Configured PIO pin: ", pin as u32);

    // setup PIO block as DMA target -- just take the data coming into the TX
    // FIFO and send it to the GPIO pins.
    let mut pio_ss = PioSharedState::new();
    let mut sm_a = pio_ss.alloc_sm().unwrap();
    pio_ss.clear_instruction_memory();
    sm_a.sm_set_enabled(false);
    #[rustfmt::skip]
    let a_code = pio_proc::pio_asm!(
        "out pins, 0",  // 0 is 32
    );
    let a_prog = LoadedProg::load(a_code.program, &mut pio_ss).unwrap();
    sm_a.sm_set_enabled(false);
    a_prog.setup_default_config(&mut sm_a);
    sm_a.config_set_out_pins(0, 32);
    sm_a.config_set_clkdiv(133.0); // have it run slow so this test operates in the background
    sm_a.config_set_out_shift(false, true, 32);
    sm_a.sm_set_pindirs_with_mask(1 << pin as usize, 1 << pin as usize);
    sm_a.sm_init(a_prog.entry());
    sm_a.sm_clear_fifos(); // ensure the fifos are cleared for this test

    // set a FIFO margin of 3. The DMA engine can do single requests ('sreq'), but the sreq
    // signal must *fall* in order for the next cycle to be initiated. Thus, you can only
    // write to a target that has "no FIFO" -- a margin of 3 is exactly that. One write,
    // one pulse. It would be more performant on the bus to have more words transferred in
    // one go, but actually, the MDMA controller is really terrible in terms of performance
    // anyways. Every word transferred incurs many bus cycles, because it refreshes the
    // control structures from RAM every single cycle.

    sm_a.sm_set_tx_fifo_margin(3);
    // If the margin above is not set, prime the FIFO by pushing junk items in, so that sreq can fall
    /*
    let mut i = 0;
    while !sm_a.sm_txfifo_is_full() {
        sm_a.sm_txfifo_push_u32(0xdead_0000 + i);
        i += 1 ;
    } */

    // setup control structure
    let init_ptr = utralib::HW_IFRAM0_MEM as *mut u32;
    for i in 0..(4 * 8 * 2) {
        unsafe { init_ptr.add(i).write_volatile(0) };
    }
    // safety: we guarantee that the pointer is aligned and initialized
    let cc_struct: &mut ControlChannels =
        unsafe { (utralib::HW_IFRAM0_MEM as *mut ControlChannels).as_mut().unwrap() };
    report_api("status", pl230.csr.r(utra::pl230::STATUS));
    pl230.csr.wfo(utra::pl230::CFG_MASTER_ENABLE, 1); // enable
    report_api("status after enable", pl230.csr.r(utra::pl230::STATUS));

    const DMA_LEN: usize = 1024;
    // DMA can't happen from main RAM, only IFRAM.
    let region_a = unsafe { core::slice::from_raw_parts_mut(utralib::HW_IFRAM0_MEM as *mut u32, DMA_LEN) };
    let mut state = 0x1111_0000;
    for d in region_a.iter_mut() {
        *d = state;
        state = crate::pl230_tests::units::lfsr_next(state);
    }

    cc_struct.channels[0].dst_end_ptr =
        (utra::rp_pio::SFR_TXF0.offset() * core::mem::size_of::<u32>() + utra::rp_pio::HW_RP_PIO_BASE) as u32;
    cc_struct.channels[0].src_end_ptr = unsafe { region_a.as_ptr().add(region_a.len() - 1) } as u32;
    let mut cc = DmaChanControl(0);
    cc.set_src_size(DmaWidth::Word as u32);
    cc.set_src_inc(DmaWidth::Word as u32);
    cc.set_dst_size(DmaWidth::Word as u32);
    cc.set_dst_inc(DmaWidth::NoInc as u32);
    cc.set_r_power(ArbitrateAfter::Xfer2 as u32);
    cc.set_n_minus_1(region_a.len() as u32 - 1);
    cc.set_cycle_ctrl(DmaCycleControl::Basic as u32);
    cc_struct.channels[0].control = cc.0;

    pl230.csr.wo(utra::pl230::CTRLBASEPTR, cc_struct.channels.as_ptr() as u32);
    pl230.csr.wo(utra::pl230::CHNLREQMASKCLR, 1); // don't mask the hardware request line
    pl230.csr.wo(utra::pl230::CHNLUSEBURSTCLR, 1); // don't mask single request line
    pl230.csr.wo(utra::pl230::CHNLENABLESET, 1);
    report_api("pio baseptr", cc_struct.channels.as_ptr() as u32);
    report_api("pio src start", region_a.as_ptr() as u32);
    report_api("pio dst start", unsafe { cc_struct.channels.as_ptr().read() }.dst_end_ptr);

    // setup EVC to route requests
    report_api("mdma_base", unsafe { pl230.mdma.base() } as u32);
    // select event number 83, which is PIO[0].
    // TODO: make this not hard-coded
    // WTF: oddly enough, it actually maps to... channel 163??? weird. this probably indicates a bug of some
    // sort.
    pl230.mdma.wo(utra::mdma::SFR_EVSEL_CR_EVSEL0, 163);
    // bit 0 - enable
    // bit 1 - mode: 1 is edge, 0 is level
    // bit 2 - enable dmareq
    // bit 3 - enable dmareqs
    // bit 4 - dmawaitonreq
    // Assert enable + dmareqs + dmawaitonreq
    pl230.mdma.wo(utra::mdma::SFR_CR_CR_MDMAREQ0, 0b11001);

    // now enable the PIO block so that DMA reqs can run
    sm_a.sm_irq0_source_enabled(PioIntSource::TxNotFull, true);
    sm_a.sm_set_enabled(true);

    let mut timeout = 0;
    while (DmaChanControl(cc_struct.channels[0].control).cycle_ctrl() != 0) && timeout < 32 {
        // report_api("dma progress ", cc_struct.channels[0].control);
        report_api("progress as baseptr[2]", unsafe { cc_struct.channels.as_ptr().read() }.control);
        timeout += 1;
    }

    report_api("pio irq", if sm_a.sm_irq0_status(None) { 1 } else { 0 });

    // irq0 will now fire and cause the DMA data to clock into the PIO block until the length is exhausted.

    // just return true to let the DMA run in the background while the rest of the tests happen,
    // and check that the gpio_out pins have a value of DMA_LEN - 1 at the conclusion of the test
    true

    // ... or we can wait until the transfers finish with this code here.
    /*
    const TIMEOUT: usize = 16;
    let mut timeout = 0;
    while (DmaChanControl(cc_struct.channels[0].control).cycle_ctrl() != 0) && timeout < TIMEOUT {
        report_api("progress as baseptr[2]", unsafe{cc_struct.channels.as_ptr().read()}.control);
        timeout += 1;
    }
    if timeout >= TIMEOUT {
        false
    } else {
        true
    } */
}
