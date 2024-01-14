use core::mem::size_of;
use core::sync::atomic::{AtomicPtr, Ordering};
use std::fmt;

/// Abstractions for the Spinal UDC controller.
/// See https://spinalhdl.github.io/SpinalDoc-RTD/dev/SpinalHDL/Libraries/Com/usb_device.html
/// for documentation.
use bitfield::bitfield;
use usb_device::UsbDirection;

pub(crate) const NUM_ENDPOINTS: usize = 16;

bitfield! {
    pub struct UdcInterrupts(u32);
    pub endpoint, set_endpoint: 15, 0;
    pub reset, set_reset: 16;
    pub ep0_setup, set_ep0_setup: 17;
    pub suspend, set_suspend: 18;
    pub resume, set_resume: 19;
    pub disconnect, set_disconnect: 20;
}
impl fmt::Debug for UdcInterrupts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ints: ")?;
        if self.0 == 0 {
            f.write_str("NIL")
        } else {
            if self.endpoint() != 0 {
                for i in 0..NUM_ENDPOINTS {
                    if self.endpoint() & (1 << i) != 0 {
                        f.write_fmt(format_args!("ep{:x} ", i))?;
                    }
                }
            }
            if self.reset() {
                f.write_str("reset ")?;
            }
            if self.ep0_setup() {
                f.write_str("ep0setup ")?;
            }
            if self.suspend() {
                f.write_str("suspend ")?;
            }
            if self.resume() {
                f.write_str("resume ")?;
            }
            if self.disconnect() { f.write_str("disconnect ") } else { Ok(()) }
        }
    }
}
bitfield! {
    pub struct UdcHalt(u32);
    impl Debug;
    pub endpointid, set_endpointid: 3, 0;
    pub enable_req, set_enable_req: 4;
    pub enable_ack, _: 5;
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
    pub ramsize, _: 3, 0;
}

/// This is located at 0xFF00 offset from the base of the memory region open for the UDC
pub struct SpinalUdcRegs {
    regs: AtomicPtr<u32>,
}
// constants in UsbDeviceCtrl.scala (L111-116) are in hex
const FRAME_OFFSET: usize = 0x0;
const ADDRESS_OFFSET: usize = 0x4;
const INT_OFFSET: usize = 0x8;
const HALT_OFFSET: usize = 0xC;
const CONFIG_OFFSET: usize = 0x10;
const RAMSIZE_OFFSET: usize = 0x20;
#[allow(dead_code)]
impl SpinalUdcRegs {
    pub fn new(ptr: *mut u32) -> SpinalUdcRegs { SpinalUdcRegs { regs: AtomicPtr::new(ptr) } }

    pub fn clone(&self) -> SpinalUdcRegs {
        SpinalUdcRegs { regs: AtomicPtr::new(self.regs.load(Ordering::SeqCst)) }
    }

    /// current USB frame ID
    pub fn frame_id(&self) -> u32 {
        unsafe { self.regs.load(Ordering::SeqCst).add(FRAME_OFFSET / size_of::<u32>()).read_volatile() }
    }

    /// currently active address for tokens. cleared by USB reset
    pub fn address(&self) -> u32 {
        unsafe { self.regs.load(Ordering::SeqCst).add(ADDRESS_OFFSET / size_of::<u32>()).read_volatile() }
    }

    pub fn set_address(&self, addr: u32) {
        unsafe {
            self.regs.load(Ordering::SeqCst).add(ADDRESS_OFFSET / size_of::<u32>()).write_volatile(addr);
        }
    }

    /// interrupt flags
    pub fn interrupts(&self) -> UdcInterrupts {
        unsafe {
            UdcInterrupts(self.regs.load(Ordering::SeqCst).add(INT_OFFSET / size_of::<u32>()).read_volatile())
        }
    }

    pub fn clear_some_interrupts(&self, ints: UdcInterrupts) {
        unsafe {
            self.regs.load(Ordering::SeqCst).add(INT_OFFSET / size_of::<u32>()).write_volatile(ints.0);
        }
    }

    pub fn clear_all_interrupts(&self) {
        unsafe {
            self.regs.load(Ordering::SeqCst).add(INT_OFFSET / size_of::<u32>()).write_volatile(0xffff_ffff);
        }
    }

    /// halt - use this to pause an endpoint to give the CPU a mutex on r/w access to its registers
    pub fn halt(&self) -> UdcHalt {
        unsafe {
            UdcHalt(self.regs.load(Ordering::SeqCst).add(HALT_OFFSET / size_of::<u32>()).read_volatile())
        }
    }

    pub fn set_halt(&self, halt: UdcHalt) {
        unsafe { self.regs.load(Ordering::SeqCst).add(HALT_OFFSET / size_of::<u32>()).write_volatile(halt.0) }
    }

    /// config
    pub fn config(&self) -> UdcConfig {
        unsafe {
            UdcConfig(self.regs.load(Ordering::SeqCst).add(CONFIG_OFFSET / size_of::<u32>()).read_volatile())
        }
    }

    pub fn set_config(&self, cfg: UdcConfig) {
        unsafe {
            self.regs.load(Ordering::SeqCst).add(CONFIG_OFFSET / size_of::<u32>()).write_volatile(cfg.0)
        }
    }

    /// the ram starting at 0 has a size of 1 << ramsize. Only the lower 4 bits are valid, but the field takes
    /// up a u32 the returned value is the properly computed bytes as read out by the hardware field (no
    /// further maths needed)
    pub fn ramsize(&self) -> u32 {
        1 << (unsafe {
            self.regs.load(Ordering::SeqCst).add(RAMSIZE_OFFSET / size_of::<u32>()).read_volatile()
        } & 0xF)
    }
}
impl fmt::Debug for SpinalUdcRegs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UDC: frame{}, adr{}, ints: {:?}", self.frame_id(), self.address(), self.interrupts(),)
    }
}

bitfield! {
    pub struct UdcEpStatus(u32);
    pub enable, set_enable: 0;
    pub force_stall, set_force_stall: 1;
    pub force_nack, set_force_nack: 2;
    // selects DATA0/DATA1; 0 => DATA0. Also set by the controller automatically
    pub data_phase, set_data_phase: 3;
    // specifies the offset of the endpoint's descriptor in RAM. 0 => empty, otherwise multiply by 16 to get the address
    pub head_offset, set_head_offset: 15, 4;
    pub isochronous, set_isochronous: 16;
    pub max_packet_size, set_max_packet_size: 31, 22;
}
impl fmt::Debug for UdcEpStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Ep{}@0x{:x}^{}: {}{}{}{}",
            if self.enable() { "ENA" } else { "DIS" },
            self.head_offset() * 16,
            self.max_packet_size(),
            if self.force_stall() { "STALL " } else { "" },
            if self.force_nack() { "NACK " } else { "" },
            if self.data_phase() { "DATA1 " } else { "DATA0 " },
            if self.isochronous() { "ISO " } else { "" },
        )
    }
}

bitfield! {
    pub struct UdcDesc0(u32);
    impl Debug;
    // current progress of the transfer, in bytes
    pub offset, set_offset: 15, 0;
    // 0xF -> in progress, 0x0 -> success
    pub code, set_code: 19, 16;
}
bitfield! {
    pub struct UdcDesc1(u32);
    impl Debug;
    // offset of the next descriptor in RAM. 0 => none, otherwise multiply by 16 to get the address in bytes
    pub next_descriptor_addr, set_next_descriptor_addr: 15, 4;
    // length of the data field in bytes
    pub length, set_length: 31, 16;
}
bitfield! {
    pub struct UdcDesc2(u32);
    impl Debug;
    // direction. 0 => OUT, 1 => IN (see DESC_OUT, DESC_IN for interpretations)
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
/// This structure maps onto a variable length region anywhere inside the UDC region. It is always aligned to
/// a 16-byte offset
pub struct SpinalUdcDescriptor {
    base: AtomicPtr<u32>,
}
#[allow(dead_code)]
impl SpinalUdcDescriptor {
    pub fn new(base: *mut u32) -> SpinalUdcDescriptor { SpinalUdcDescriptor { base: AtomicPtr::new(base) } }

    pub(crate) fn read(&self, offset: usize) -> u32 {
        // we don't do asserts on reads because for debugging sometimes we reveal invalid descriptors and
        // that's OK
        unsafe { self.base.load(Ordering::SeqCst).add(offset).read_volatile() }
    }

    fn write(&self, offset: usize, data: u32) {
        // -16 for the dedicated ep0-out space
        assert!(
            (self.base.load(Ordering::SeqCst) as u32) & 0xFFF >= crate::START_OFFSET - 16,
            "descriptor is illegal (too low)! 0x{:x}",
            self.base.load(Ordering::SeqCst) as u32
        );
        assert!(
            (self.base.load(Ordering::SeqCst) as u32) & 0xFFF < crate::END_OFFSET,
            "descriptor is illegal (too high)! 0x{:x}",
            self.base.load(Ordering::SeqCst) as u32
        );
        unsafe { self.base.load(Ordering::SeqCst).add(offset).write_volatile(data) }
    }

    pub fn offset(&self) -> usize { UdcDesc0(self.read(0)).offset() as usize }

    pub fn in_progress(&self) -> bool { UdcDesc0(self.read(0)).code() == 0xF }

    pub fn next_descriptor_addr(&self) -> usize { UdcDesc1(self.read(1)).next_descriptor_addr() as usize }

    pub fn length(&self) -> usize { UdcDesc1(self.read(1)).length() as usize }

    pub fn direction(&self) -> UsbDirection {
        if UdcDesc2(self.read(2)).direction() { UsbDirection::In } else { UsbDirection::Out }
    }

    pub fn int_on_done(&self) -> bool { UdcDesc2(self.read(2)).int_on_done() }

    pub fn completion_on_full(&self) -> bool { UdcDesc2(self.read(2)).completion_on_full() }

    pub fn data1_on_completion(&self) -> bool { UdcDesc2(self.read(2)).data1_on_completion() }

    pub fn set_offset(&self, offset: usize) {
        let mut d0 = UdcDesc0(0);
        d0.set_offset(offset as _);
        d0.set_code(0xF); // automatically set in_progress
        self.write(0, d0.0);
    }

    pub fn set_offset_only(&self, offset: usize) {
        let mut d0 = UdcDesc0(0);
        d0.set_offset(offset as _);
        d0.set_code(0x0); // clears in_progress
        self.write(0, d0.0);
    }

    pub fn set_next_desc_and_len(&self, next_addr: usize, length: usize) {
        let mut d1 = UdcDesc1(0);
        d1.set_length(length as _);
        d1.set_next_descriptor_addr(next_addr as _);
        self.write(1, d1.0);
    }

    pub fn set_desc_flags(
        &self,
        direction: UsbDirection,
        int_on_done: bool,
        completion_on_full: bool,
        data1_on_completion: bool,
    ) {
        let mut d2 = UdcDesc2(0);
        match direction {
            UsbDirection::In => d2.set_direction(true),
            UsbDirection::Out => d2.set_direction(false),
        }
        d2.set_int_on_done(int_on_done);
        d2.set_completion_on_full(completion_on_full);
        d2.set_data1_on_completion(data1_on_completion);
        self.write(2, d2.0);
    }

    pub fn write_data(&self, offset_word: usize, data: u32) {
        // -16 for the dedicated ep0-out space
        assert!(
            (self.base.load(Ordering::SeqCst) as u32) & 0xFFF >= crate::START_OFFSET - 16,
            "descriptor is illegal (too low)!"
        );
        assert!(
            (self.base.load(Ordering::SeqCst) as u32) & 0xFFF < crate::END_OFFSET,
            "descriptor is illegal (too high)!"
        );
        unsafe { self.base.load(Ordering::SeqCst).add(3 + offset_word).write_volatile(data) }
    }

    pub fn read_data(&self, offset_word: usize) -> u32 {
        unsafe { self.base.load(Ordering::SeqCst).add(3 + offset_word).read_volatile() }
    }
}
impl fmt::Debug for SpinalUdcDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Desc({}){}: {} of {} -> 0x{:x} ({}{}{}) [{:x?},{:x?},..] @ {:x?}",
            if self.in_progress() { "<>" } else { "--" },
            match self.direction() {
                UsbDirection::In => "IN",
                UsbDirection::Out => "OUT",
            },
            self.offset(),
            self.length(),
            self.next_descriptor_addr() * 16,
            if self.int_on_done() { "I" } else { "." },
            if self.completion_on_full() { "C" } else { "." },
            if self.data1_on_completion() { "1" } else { "0" },
            self.read_data(0).to_le_bytes(),
            self.read_data(1).to_le_bytes(),
            self.base,
        )
    }
}
