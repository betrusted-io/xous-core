use utralib::generated::*;
use crate::*;
use bitfield::bitfield;
use core::ops::{Deref, DerefMut};

bitfield! {
    pub struct UdcInterrupts(u32);
    impl Debug;
    pub endpoint, clear_endpoint: 15, 0;
    pub reset, clear_reset: 16;
    pub ep0_setup, clear_ep0_setup: 17;
    pub suspend, clear_suspend: 18;
    pub resume, clear_resume: 19;
    pub disconnect, clear_disconnect: 20;
}
bitfield! {
    pub struct UdcHalt(u32);
    impl Debug;
    pub endpointid, set_endpointid: 3, 0;
    pub enable_req, set_enable_req: 4;
    pub enable_ack, set_enable_ack: 5; // question: can we make this ... read-only?
}
bitfield! {
    pub struct UdcConfig(u32);
    impl Debug;
    // this has an odd form: you must write `1` to these respective bits like "radio buttons" to affect pullups and interrupts
    pub pullup_on, set_pullup_on: 0;
    pub pullup_off, set_pullup_off: 1;
    pub enable_ints, set_enable_ints: 2;
    pub disable_ints, set_disable_ints: 3;
}
bitfield! {
    pub struct UdcRamsize(u32);
    impl Debug;
    pub ramsize, dnu_ramsize: 3, 0;
}
bitfield! {
    pub struct UdcEpStatus(u32);
    impl Debug;
    pub enable, set_enable: 0;
    pub force_stall, set_force_stall: 1;
    pub force_nack, set_force_nack: 2;
    // selects DATA0/DATA1; 0 => DATA0. Also set by the controller automatically
    pub data_phase, set_data_phase: 3;
    // specifies the offset of the endpoint's descriptor in RAM. 0 => empty, otherwise multply by 16 to get the address
    pub head_offset, set_head_offset: 15, 4;
    pub isochronous, set_isochronous: 16;
    pub max_packet_size, set_max_packet_size: 31, 22;
}
bitfield! {
    pub struct UdcDescriptor0(u32);
    impl Debug;
    // current progress of the transfer, in bytes
    pub offset, set_offset: 15, 0;
    // 0xF -> in progress, 0x0 -> success
    pub code, set_code: 19, 16;
}
bitfield! {
    pub struct UdcDescriptor1(u32);
    impl Debug;
    // offset of the next descriptor in RAM. 0 => none, otherwise multiply by 16 to get the address in bytes
    pub next_offset, set_next_offset: 15, 4;
    // length of the data field in bytes
    pub length, set_length: 31, 16;
}
bitfield! {
    pub struct UdcDescriptor2(u32);
    impl Debug;
    // direction. 0 => OUT, 1 => IN
    pub direction, set_direction: 16;
    // if set, fires an interrupt when descriptor is completed
    pub int_on_done, set_int_on_done: 17;
    // From SpinalHDL docs directly: Normally, a descriptor completion only occurs when a USB transfer
    // is smaller than the max_packet_size. But, if this field is set,
    // then when the descriptor becomes full is also considered a completion event. (offset == length)
    pub completion_on_full, set_completion_on_full: 18;
    // forces dataphase to DATA1 when the descriptor is complete
    pub data1_on_completion, set_data1_on_completion: 19;
}

/// This is located at 0xFF00 offset from the base of the memory region open for the UDC
#[repr(C)]
#[derive(Debug)]
pub struct SpinalUdcRegs {
    /// current USB frame ID
    frame: u32,
    /// currently active address for tokens. cleared by USB reset
    address: u32,
    /// interrupt flags
    interrupts: UdcInterrupts,
    /// halt - use this to pause an endpoint to give the CPU a mutex on r/w access to its registers
    halt: UdcHalt,
    /// config
    config: UdcConfig,
    /// the ram starting at 0 has a size of 1 << ramsize. Only the lower 4 bits are valid, but the field takes up a u32
    ramsize: UdcRamsize,
}
impl Deref for SpinalUdcRegs {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const SpinalUdcRegs as *const u8, core::mem::size_of::<SpinalUdcRegs>())
                as &[u8]
        }
    }
}
impl DerefMut for SpinalUdcRegs {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut SpinalUdcRegs as *mut u8, core::mem::size_of::<SpinalUdcRegs>())
                as &mut [u8]
        }
    }
}


/// This is located at 0x0000-0x0047 inside the UDC region
#[repr(C)]
pub struct SpinalUdcMem {
    endpoints: [UdcEpStatus; 16],
    setup_data: [u8; 8],
}

/// This structure maps onto a variable length region anywhere inside the UDC region. It is always aligned to a 16-byte offset
#[repr(C)]
pub struct SpinalUdcDescriptorHeader {
    d0: UdcDescriptor0,
    d1: UdcDescriptor1,
    d2: UdcDescriptor2,
}
/// This structure is a set of references to a UDC descriptor in RAM. It's tricky to
/// construct correctly, as it requires interpreting some bit fields returned by the
/// UDC to map where the header goes and then determine the length of the data. The
/// data slice's length cannot be known at compile time, because it varies with the size
/// of the USB packet. However, the data should always be located at an address immediately
/// following the header's location.
pub struct SpinalUdcDescriptor<'a> {
    header: &'a SpinalUdcDescriptorHeader,
    data: &'a [u8],
}

pub struct UsbTest {
    pub(crate) conn: CID,
    gpio_csr: utralib::CSR<u32>,
    usb: xous::MemoryRange,
    regs: &'static mut SpinalUdcRegs,
}

impl UsbTest {
    pub fn new(sid: xous::SID) -> UsbTest {
        let gpio_base = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map GPIO CSR range");
        // this particular core does not use CSRs for control - it uses directly memory mapped registers
        let usb = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_USBDEV_MEM),
            None,
            utralib::HW_USBDEV_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map USB device memory range");

        let mut usbtest = UsbTest {
            gpio_csr: CSR::new(gpio_base.as_mut_ptr() as *mut u32),
            conn: xous::connect(sid).unwrap(),
            usb,
            // Safety: the offset of the register bank is defined as 0xFF00 from the base of the
            // usb memory area. Mapping SpinalUdcRegs here is safe assuming the structure has
            // been correctly defined.
            regs: unsafe {
                (usb.as_mut_ptr().add(0xFF00) as *mut SpinalUdcRegs).as_mut().unwrap()
            }
        };
        usbtest
    }
    pub fn print_regs(&self) {
        log::info!("control regs: {:x?}", self.regs);
    }

    pub fn connect_device_core(&mut self, state: bool) {
        if state {
            log::info!("connecting USB device core");
            self.gpio_csr.wfo(utra::gpio::USBSELECT_USBSELECT, 1);
        } else {
            log::info!("connecting USB debug core");
            self.gpio_csr.wfo(utra::gpio::USBSELECT_USBSELECT, 0);
        }
    }

    pub fn suspend(&mut self) {
    }
    pub fn resume(&mut self) {
    }
}
