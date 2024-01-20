#![cfg_attr(feature = "baremetal", no_std)]

#[cfg(feature = "baremetal")]
mod debug;

#[cfg(feature = "tests")]
pub mod pl230_tests;

use bitfield::bitfield;
use utralib::*;

#[repr(C, align(16))]
#[derive(Default)]
struct ChannelControl {
    pub src_end_ptr: u32,
    pub dst_end_ptr: u32,
    pub control: u32,
    pub reserved: u32,
}

#[repr(C, align(256))]
#[derive(Default)]
struct ControlChannels {
    pub channels: [ChannelControl; 8],
}

pub struct Pl230 {
    pub csr: CSR<u32>,
    pub mdma: CSR<u32>,
}

impl Pl230 {
    #[cfg(feature = "baremetal")]
    pub fn new() -> Self {
        Pl230 {
            csr: CSR::new(utralib::HW_PL230_BASE as *mut u32),
            mdma: CSR::new(utralib::HW_MDMA_BASE as *mut u32),
        }
    }

    #[cfg(not(feature = "baremetal"))]
    pub fn new() -> Self {
        let csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_PL230_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();
        let mdma = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_MDMA_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        Pl230 { csr: CSR::new(csr.as_mut_ptr() as *mut u32), mdma: CSR::new(mdma.as_mut_ptr() as *mut u32) }
    }
}

#[repr(u32)]
pub enum DmaWidth {
    Byte = 0b00,
    HalfWord = 0b01,
    Word = 0b10,
    NoInc = 0b11,
}
#[repr(u32)]
#[rustfmt::skip]
pub enum ArbitrateAfter {
    XferEach = 0b0000,
    Xfer2    = 0b0001,
    Xfer4    = 0b0010,
    Xfer8    = 0b0011,
    Xfer16   = 0b0100,
    Xfer32   = 0b0101,
    Xfer64   = 0b0110,
    Xfer128  = 0b0111,
    Xfer256  = 0b1000,
    Xfer512  = 0b1001,
    Xfer1024 = 0b1010,
}
#[repr(u32)]
pub enum DmaCycleControl {
    Stop = 0b000,
    Basic = 0b001,
    AutoRequest = 0b010,
    PingPong = 0b011,
    MemoryScatterGatherPrimary = 0b100,
    MemoryScatterGatherAlt = 0b101,
    PeripheralScatterGatherPrimary = 0b110,
    PeripheralScatterGatherAlt = 0b111,
}

bitfield! {
    #[derive(Copy, Clone)]
    pub struct DmaChanControl(u32);
    impl Debug;
    pub cycle_ctrl, set_cycle_ctrl: 2, 0;
    pub next_useburst, set_next_useburst: 3;
    pub n_minus_1, set_n_minus_1: 13, 4;
    pub r_power, set_r_power: 17, 14;
    pub src_prot_ctrl, set_src_prot_ctrl: 20, 18;
    pub dst_prot_ctrl, set_dst_prot_ctrl: 23, 21;
    pub src_size, set_src_size: 25, 24;
    pub src_inc, set_src_inc: 27, 26;
    pub dst_size, set_dst_size: 29, 28;
    pub dst_inc, set_dst_inc: 31, 30;
}
