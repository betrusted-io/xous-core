use crate::*;
use super::report_api;
use utralib::*;

/// used to generate some test vectors
pub fn lfsr_next(state: u32) -> u32 {
    let bit = ((state >> 31) ^
               (state >> 21) ^
               (state >>  1) ^
               (state >>  0)) & 1;

    (state << 1) + bit
}

pub fn basic_tests (pl230: &mut Pl230) {
    report_api("channels", pl230.csr.rf(utra::pl230::STATUS_CHNLS_MINUS1) + 1);
    //report_api("id0", pl230.csr.r(utra::pl230::PERIPH_ID_0));
    //report_api("id1", pl230.csr.r(utra::pl230::PERIPH_ID_1));
    //report_api("id2", pl230.csr.r(utra::pl230::PERIPH_ID_2));
    let mut cc_struct = ControlChannels::default();

    // read the status register
    report_api("status", pl230.csr.r(utra::pl230::STATUS));
    pl230.csr.wfo(utra::pl230::CFG_MASTER_ENABLE, 1); // enable
    report_api("status after enable", pl230.csr.r(utra::pl230::STATUS));

    // setup the PL230 to do a simple transfer between two memory regions
    let mut region_a = [0u32; 16];
    let region_b = [0u32; 16];
    let mut state = 0x1111_1111;
    for d in region_a.iter_mut() {
        *d = state;
        state = lfsr_next(state);
    }

    cc_struct.channels[0].dst_end_ptr = unsafe{region_b.as_ptr().add(region_b.len() - 1)} as u32;
    cc_struct.channels[0].src_end_ptr = unsafe{region_a.as_ptr().add(region_a.len() - 1)} as u32;
    let mut cc = DmaChanControl(0);
    cc.set_src_size(DmaWidth::Word as u32);
    cc.set_src_inc(DmaWidth::Word as u32);
    cc.set_dst_size(DmaWidth::Word as u32);
    cc.set_dst_inc(DmaWidth::Word as u32);
    cc.set_r_power(ArbitrateAfter::Xfer8 as u32);
    cc.set_n_minus_1(region_a.len() as u32 - 1);
    cc.set_cycle_ctrl(DmaCycleControl::AutoRequest as u32);
    cc_struct.channels[0].control = cc.0;

    pl230.csr.wo(utra::pl230::CTRLBASEPTR, cc_struct.channels.as_ptr() as u32);
    pl230.csr.wo(utra::pl230::CHNLREQMASKSET, 1);
    pl230.csr.wo(utra::pl230::CHNLENABLESET, 1);

    report_api("baseptr", cc_struct.channels.as_ptr() as u32);
    report_api("baseptr[0]", unsafe{cc_struct.channels.as_ptr().read()}.src_end_ptr);
    report_api("baseptr[1]", unsafe{cc_struct.channels.as_ptr().read()}.dst_end_ptr);
    report_api("baseptr[2]", unsafe{cc_struct.channels.as_ptr().read()}.control);
    report_api("baseptr[3]", unsafe{cc_struct.channels.as_ptr().read()}.reserved);
    report_api("baseptr reg", pl230.csr.r(utra::pl230::CTRLBASEPTR));

    // this should kick off the DMA
    pl230.csr.wo(utra::pl230::CHNLSWREQUEST, 1);

    let mut timeout = 0;
    while (DmaChanControl(cc_struct.channels[0].control).cycle_ctrl() != 0) && timeout < 16 {
        report_api("dma progress ", cc_struct.channels[0].control);
        timeout += 1;
    }
    for d in region_b {
        report_api("b: ", d);
    }
}