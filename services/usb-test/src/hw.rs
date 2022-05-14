use susres::{ManagedMem, SuspendResume};
use usb_device::bus::PollResult;
use utralib::generated::*;
use crate::*;
use bitfield::bitfield;
use core::mem::size_of;
use core::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use usb_device::{class_prelude::*, Result, UsbDirection};
use std::collections::BTreeMap;
use std::fmt;

pub fn log_init() -> *mut u32 {
    let gpio_base = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map GPIO CSR range");
    let mut gpio_csr = CSR::new(gpio_base.as_mut_ptr() as *mut u32);
    // setup the initial logging output
    // 0 = kernel, 1 = log, 2 = app, 3 = invalid
    gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1);

    gpio_base.as_mut_ptr() as *mut u32
}

const NUM_ENDPOINTS: usize = 16;

fn handle_usb(_irq_no: usize, arg: *mut usize) {
    let usb = unsafe { &mut *(arg as *mut SpinalUsbDevice) };
    let pending = usb.csr.r(utra::usbdev::EV_PENDING);

    // actual interrupt handling is done in userspace, this just triggers the routine

    usb.csr.wo(utra::usbdev::EV_PENDING, pending);

    xous::try_send_message(usb.conn,
        xous::Message::new_scalar(Opcode::UsbIrqHandler.to_usize().unwrap(), 0, 0, 0, 0)).ok();
}

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
                f.write_fmt(format_args!("ep{:x} ", &self.endpoint()))?;
            }
            if self.reset() {
                f.write_str("reset ")?;
            }
            if self.ep0_setup() {
                f.write_str("ep0 ")?;
            }
            if self.suspend() {
                f.write_str("suspend ")?;
            }
            if self.resume() {
                f.write_str("resume ")?;
            }
            if self.disconnect() {
                f.write_str("disconnect ")
            } else {
                Ok(())
            }
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
    pub fn new(ptr: *mut u32) -> SpinalUdcRegs {
        SpinalUdcRegs {
            regs: AtomicPtr::new(ptr)
        }
    }
    pub fn clone(&self) -> SpinalUdcRegs {
        SpinalUdcRegs {
            regs: AtomicPtr::new(self.regs.load(Ordering::SeqCst))
        }
    }
    /// current USB frame ID
    pub fn frame_id(&self) -> u32 {
        unsafe {
            self.regs.load(Ordering::SeqCst).add(FRAME_OFFSET / size_of::<u32>()).read_volatile()
        }
    }
    /// currently active address for tokens. cleared by USB reset
    pub fn address(&self) -> u32 {
        unsafe {
            self.regs.load(Ordering::SeqCst).add(ADDRESS_OFFSET / size_of::<u32>()).read_volatile()
        }
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
        unsafe {
            self.regs.load(Ordering::SeqCst).add(HALT_OFFSET / size_of::<u32>()).write_volatile(halt.0)
        }
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
    /// the ram starting at 0 has a size of 1 << ramsize. Only the lower 4 bits are valid, but the field takes up a u32
    /// the returned value is the properly computed bytes as read out by the hardware field (no further maths needed)
    pub fn ramsize(&self) -> u32 {
        1 << (unsafe {
            self.regs.load(Ordering::SeqCst).add(RAMSIZE_OFFSET / size_of::<u32>()).read_volatile()
        } & 0xF)
    }
}
impl fmt::Debug for SpinalUdcRegs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UDC: frame{}, adr{}, ints: {:?}",
            self.frame_id(),
            self.address(),
            self.interrupts(),
        )
    }
}

bitfield! {
    pub struct UdcEpStatus(u32);
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
impl fmt::Debug for UdcEpStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ep{}@0x{:x}^{}: {}{}{}{}",
            if self.enable() {"ENA"} else {"DIS"},
            self.head_offset() * 16,
            self.max_packet_size(),
            if self.force_stall() {"STALL "} else {""},
            if self.force_nack() {"NACK "} else {""},
            if self.data_phase() {"DATA1 "} else {"DATA0 "},
            if self.isochronous() {"ISO "} else {""},
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
    pub next_offset, set_next_offset: 15, 4;
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
/// This structure maps onto a variable length region anywhere inside the UDC region. It is always aligned to a 16-byte offset
pub struct SpinalUdcDescriptor {
    base: AtomicPtr::<u32>,
}
#[allow(dead_code)]
impl SpinalUdcDescriptor {
    pub fn new(base: *mut u32) -> SpinalUdcDescriptor {
        SpinalUdcDescriptor { base: AtomicPtr::new(base) }
    }
    fn read(&self, offset: usize) -> u32 {
        unsafe{self.base.load(Ordering::SeqCst).add(offset).read_volatile()}
    }
    fn write(&self, offset: usize, data: u32) {
        unsafe{self.base.load(Ordering::SeqCst).add(offset).write_volatile(data)}
    }
    pub fn offset(&self) -> usize {
        UdcDesc0(self.read(0)).offset() as usize
    }
    pub fn in_progress(&self) -> bool {
        UdcDesc0(self.read(0)).code() == 0xF
    }
    pub fn next_offset(&self) -> usize {
        UdcDesc1(self.read(1)).next_offset() as usize
    }
    pub fn length(&self) -> usize {
        UdcDesc1(self.read(1)).length() as usize
    }
    pub fn direction(&self) -> UsbDirection {
        if UdcDesc2(self.read(2)).direction() {
            UsbDirection::In
        } else {
            UsbDirection::Out
        }
    }
    pub fn int_on_done(&self) -> bool {
        UdcDesc2(self.read(2)).int_on_done()
    }
    pub fn completion_on_full(&self) -> bool {
        UdcDesc2(self.read(2)).completion_on_full()
    }
    pub fn data1_on_completion(&self) -> bool {
        UdcDesc2(self.read(2)).data1_on_completion()
    }
    pub fn set_desc0(&self, offset: usize) {
        let mut d0 = UdcDesc0(0);
        d0.set_offset(offset as _);
        d0.set_code(0xF); // automatically set in_progress
        self.write(0, d0.0);
    }
    pub fn set_desc1(&self, next_offset: usize, length: usize) {
        let mut d1 = UdcDesc1(0);
        d1.set_length(length as _);
        d1.set_next_offset(next_offset as _);
        self.write(1, d1.0);
    }
    pub fn set_desc2(&self, direction: UsbDirection, int_on_done: bool, completion_on_full: bool, data1_on_completion: bool) {
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
        unsafe {
            self.base.load(Ordering::SeqCst).add(3 + offset_word).write_volatile(data)
        }
    }
    pub fn read_data(&self, offset_word: usize) -> u32 {
        unsafe {
            self.base.load(Ordering::SeqCst).add(3 + offset_word).read_volatile()
        }
    }
}
impl fmt::Debug for SpinalUdcDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Desc({}){}: {} of {} -> 0x{:x} ({}{}{}) [{:x?},{:x?},..]",
            if self.in_progress() {"<>"} else {"--"},
            match self.direction() {
                UsbDirection::In => "IN",
                UsbDirection::Out => "OUT",
            },
            self.offset(),
            self.length(),
            self.next_offset() * 16,
            if self.int_on_done() {"I"} else {"."},
            if self.completion_on_full() {"C"} else {"."},
            if self.data1_on_completion() {"1"} else {"0"},
            self.read_data(0).to_le_bytes(),
            self.read_data(1).to_le_bytes(),
        )
    }
}

pub struct SpinalUsbMgmt {
    csr: AtomicCsr<u32>, // consider using VolatileCell and/or refactory AtomicCsr so it is non-mutable
    usb: AtomicPtr<u8>,
    eps: AtomicPtr<UdcEpStatus>,
    srmem: ManagedMem<{ utralib::generated::HW_USBDEV_MEM_LEN / core::mem::size_of::<u32>() }>,
    regs: SpinalUdcRegs,
}
impl SpinalUsbMgmt {
    pub fn print_regs(&self) {
        /*
        for i in 0..8 {
            let offset = 0xff00 + i * 4;
            log::info!(
                "{:x}: 0x{:x}",
                offset,
                unsafe{(self.usb.load(Ordering::SeqCst).add(offset) as *mut u32).read_volatile()},
            );
        } */
        //unsafe{self.usb.load(Ordering::SeqCst).add(0xff10 / size_of::<u32>()).write_volatile(0x4)};
        for i in 0..16 {
            let ep_status = self.status_from_index(i);
            if ep_status.enable() {
                log::info!("ep{}_status: {:x?}", i, ep_status);
                if ep_status.head_offset() != 0 {
                    let desc = self.descriptor_from_status(&ep_status);
                    log::info!("offset: {}, in_progress: {}, length: {}", desc.offset(), desc.in_progress(), desc.length());
                }
                if i == 0 {
                    let setup_data_base = unsafe{self.usb.load(Ordering::SeqCst).add(0x40) as *mut u32};
                    log::info!("setup area: {:x?}{:x?}",
                        unsafe{setup_data_base.add(0).read_volatile()}.to_le_bytes(),
                        unsafe{setup_data_base.add(1).read_volatile()}.to_le_bytes()
                    );
                }
            }
        }
        log::trace!("frame id: {}", self.regs.frame_id());
        log::debug!("usb addr: {}", self.regs.address());
        log::debug!("ints: {:x?}", self.regs.interrupts());
        log::trace!("halt: 0x{:x?}", self.regs.halt());
        log::trace!("config: 0x{:x?}", self.regs.config());
        log::trace!("ramsize: {}", self.regs.ramsize());
        assert!(4096 == self.regs.ramsize(), "hardware ramsize parameter does not match our expectations");
    }
    pub fn connect_device_core(&mut self, state: bool) {
        log::info!("previous state: {}", self.csr.rf(utra::usbdev::USBSELECT_USBSELECT));
        if state {
            log::info!("connecting USB device core");
            self.csr.wfo(utra::usbdev::USBSELECT_USBSELECT, 1);
        } else {
            log::info!("connecting USB debug core");
            self.csr.wfo(utra::usbdev::USBSELECT_USBSELECT, 0);
        }
    }
    pub fn xous_suspend(&mut self) {
        self.csr.wo(utra::usbdev::EV_PENDING, 0xFFFF_FFFF);
        self.csr.wo(utra::usbdev::EV_ENABLE, 0x0);
        self.srmem.suspend();
    }
    pub fn xous_resume(&mut self) {
        self.srmem.resume();
        let p = self.csr.r(utra::usbdev::EV_PENDING); // this has to be expanded out because AtomicPtr is potentially mutable on read
        self.csr.wo(utra::usbdev::EV_PENDING, p); // clear in case it's pending for some reason
        self.csr.wfo(utra::usbdev::EV_ENABLE_USB, 1);
    }
    pub fn descriptor_from_status(&self, ep_status: &UdcEpStatus) -> SpinalUdcDescriptor {
        SpinalUdcDescriptor::new(
            unsafe{ self.usb.load(Ordering::SeqCst).add(
                ep_status.head_offset() as usize * 16
            ) as *mut u32}
        )
    }
    pub fn status_from_index(&self, index: usize) -> UdcEpStatus {
        unsafe {
            self.eps.load(Ordering::SeqCst).add(index).read_volatile()
        }
    }
}
pub struct SpinalUsbDevice {
    pub(crate) conn: CID,
    usb: xous::MemoryRange,
    csr_addr: u32,
    csr: AtomicCsr<u32>, // consider using VolatileCell and/or refactory AtomicCsr so it is non-mutable
    regs: SpinalUdcRegs,
    // 1:1 mapping of endpoint structures to offsets in the memory space for the actual ep storage
    // data must be committed to this in a single write, and not composed dynamcally using this as scratch space
    eps: AtomicPtr<UdcEpStatus>,
    // tracks which endpoints have been allocated. ep0 is special.
    ep_allocs: [bool; 16],
    // record a copy of the ep0 IN setup, as the SETUP transaction smashes its config
    ep0_copy: Option<UdcEpStatus>,
    // structure to track space allocations within the memory space
    allocs: Arc::<Mutex::<BTreeMap<u32, u32>>>, // key is offset, value is len
    tt: ticktimer_server::Ticktimer,
}
impl SpinalUsbDevice {
    pub fn new(sid: xous::SID) -> SpinalUsbDevice {
        // this particular core does not use CSRs for control - it uses directly memory mapped registers
        let usb = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::HW_USBDEV_MEM),
            None,
            utralib::HW_USBDEV_MEM_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map USB device memory range");
        let csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::usbdev::HW_USBDEV_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map USB CSR range");

        let mut usbdev = SpinalUsbDevice {
            conn: xous::connect(sid).unwrap(),
            csr_addr: csr.as_ptr() as u32,
            csr: AtomicCsr::new(csr.as_mut_ptr() as *mut u32),
            usb,
            // Safety: the offset of the register bank is defined as 0xFF00 from the base of the
            // usb memory area. Mapping SpinalUdcRegs here is safe assuming the structure has
            // been correctly defined.
            regs: SpinalUdcRegs::new(unsafe{usb.as_mut_ptr().add(0xFF00) as *mut u32}),
            eps: AtomicPtr::new(unsafe {
                    (usb.as_mut_ptr().add(0x00) as *mut UdcEpStatus).as_mut().unwrap()
            }),
            ep0_copy: None,
            ep_allocs: [false; 16],
            allocs: Arc::new(Mutex::new(BTreeMap::new())),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
        };

        xous::claim_interrupt(
            utra::usbdev::USBDEV_IRQ,
            handle_usb,
            (&mut usbdev) as *mut SpinalUsbDevice as *mut usize,
        )
        .expect("couldn't claim irq");
        let p = usbdev.csr.r(utra::usbdev::EV_PENDING);
        usbdev.csr.wo(utra::usbdev::EV_PENDING, p); // clear in case it's pending for some reason
        usbdev.csr.wfo(utra::usbdev::EV_ENABLE_USB, 1);

        let mut cfg = UdcConfig(0);
        cfg.set_pullup_on(true); // required for proper operation
        usbdev.regs.set_config(cfg);

        usbdev
    }
    pub fn get_iface(&self) -> SpinalUsbMgmt {
        SpinalUsbMgmt {
            csr: AtomicCsr::new(self.csr_addr as *mut u32),
            usb: AtomicPtr::new(self.usb.as_mut_ptr() as *mut u8),
            eps: AtomicPtr::new(unsafe {
                (self.usb.as_mut_ptr().add(0x00) as *mut UdcEpStatus).as_mut().unwrap()
            }),
            srmem: ManagedMem::new(self.usb),
            regs: self.regs.clone(),
        }
    }
    fn print_poll_result(&self, poll_result: &PollResult) {
        let info = match poll_result {
            PollResult::None => "PollResult::None".to_string(),
            PollResult::Reset => "PollResult::Reset".to_string(),
            PollResult::Resume => "PollResult::Resume".to_string(),
            PollResult::Suspend => "PollResult::Suspend".to_string(),
            PollResult::Data {ep_out, ep_in_complete, ep_setup} =>
                format!("PollResult::Data out:{:x} in:{:x} setup:{:x}", ep_out, ep_in_complete, ep_setup),
        };
        log::info!("<<<< {}", info);
    }
    #[allow(dead_code)]
    pub fn print_ep_stats(&self) {
        for i in 0..16 {
            let ep_status = self.status_read_volatile(i);
            if ep_status.enable() {
                log::info!("ep{}_status: {:x?}", i, ep_status);
                if ep_status.head_offset() != 0 {
                    let desc = self.descriptor_from_status(&ep_status);
                    log::info!("offset: {}, in_progress: {}, length: {}", desc.offset(), desc.in_progress(), desc.length());
                }
                if i == 0 {
                    let setup_data_base = unsafe{self.usb.as_ptr().add(0x40) as *mut u32};
                    log::info!("setup area: {:x?}{:x?}",
                        unsafe{setup_data_base.add(0).read_volatile()}.to_le_bytes(),
                        unsafe{setup_data_base.add(1).read_volatile()}.to_le_bytes()
                    );
                }
            }
        }
    }
    /// simple but easy to understand allocator for buffers inside the descriptor memory space
    /// See notes inside src/main.rs `alloc_inner` for the functional description. Returns
    /// the full byte-addressed offset of the region, so it must be shifted to the right by
    /// 4 before being put into a SpinalHDL descriptor (it uses 16-byte alignment and thus
    /// discards the lower 4 bits).
    pub fn alloc_region(&mut self, requested: u32) -> Option<u32> {
        alloc_inner(&mut self.allocs.lock().unwrap(), requested)
    }
    #[allow(dead_code)]
    /// returns `true` if the region was available to be deallocated
    pub fn dealloc_region(&mut self, offset: u32) -> bool {
        dealloc_inner(&mut self.allocs.lock().unwrap(), offset)
    }
    pub(crate) fn descriptor_from_status(&self, ep_status: &UdcEpStatus) -> SpinalUdcDescriptor {
        SpinalUdcDescriptor::new(
            unsafe{ self.usb.as_mut_ptr().add(
                ep_status.head_offset() as usize * 16
            ) as *mut u32}
        )
    }
    pub(crate) fn status_read_volatile(&self, index: usize) -> UdcEpStatus {
        unsafe {
            self.eps.load(Ordering::SeqCst).add(index).read_volatile()
        }
    }
    pub(crate) fn status_write_volatile(&self, index: usize, ep_status: UdcEpStatus) {
        unsafe {
            self.eps.load(Ordering::SeqCst).add(index).write_volatile(ep_status)
        }
    }
    pub(crate) fn udc_hard_halt(&self, index: usize) {
        self.regs.set_halt(UdcHalt(index as u32 | 0x10));
        let mut iters = 0;
        while !self.regs.halt().enable_ack() {
            xous::yield_slice();
            iters += 1;
            if iters == 1000 {
                log::info!("udc_hard_halt possibly timed out");
            }
        }
    }
    pub(crate) fn udc_hard_unhalt(&self, index: usize) {
        self.regs.set_halt(UdcHalt(index as u32));
    }
    pub(crate) fn get_setup(&self) -> [u8; 8] {
        let mut setup = [0u8; 8];
        let setup_data_base = unsafe{self.usb.as_ptr().add(0x40) as *const u32};
        let setup_data = unsafe{core::slice::from_raw_parts(setup_data_base, 2)};
        setup[..4].copy_from_slice(
            &setup_data[0].to_le_bytes()
        );
        setup[4..8].copy_from_slice(
            &setup_data[1].to_le_bytes()
        );
        setup
    }
}

impl UsbBus for SpinalUsbDevice {
    /// Allocates an endpoint and specified endpoint parameters. This method is called by the device
    /// and class implementations to allocate endpoints, and can only be called before
    /// [`enable`](UsbBus::enable) is called.
    ///
    /// # Arguments
    ///
    /// * `ep_dir` - The endpoint direction.
    /// * `ep_addr` - A static endpoint address to allocate. If Some, the implementation should
    ///   attempt to return an endpoint with the specified address. If None, the implementation
    ///   should return the next available one.
    /// * `max_packet_size` - Maximum packet size in bytes.
    /// * `interval` - Polling interval parameter for interrupt endpoints.
    ///
    /// # Errors
    ///
    /// * [`EndpointOverflow`](crate::UsbError::EndpointOverflow) - Available total number of
    ///   endpoints, endpoints of the specified type, or endpoind packet memory has been exhausted.
    ///   This is generally caused when a user tries to add too many classes to a composite device.
    /// * [`InvalidEndpoint`](crate::UsbError::InvalidEndpoint) - A specific `ep_addr` was specified
    ///   but the endpoint in question has already been allocated.
    fn alloc_ep(
        &mut self,
        ep_dir: UsbDirection,
        ep_addr: Option<EndpointAddress>,
        ep_type: EndpointType,
        max_packet_size: u16,
        _interval: u8,
    ) -> Result<EndpointAddress> {
        // if ep_addr is specified, create a 1-unit range else a range through the entire space
        // note that ep_addr is a packed representation of index and direction,
        // so you must use `.index()` to get just the index part
        log::info!("alloc ep spec: {:?} of type {:?}", ep_addr, ep_type);
        if ep_addr == Some(EndpointAddress::from_parts(0, UsbDirection::Out)) { // flag the control in as a special case
            log::info!("ep0 allocation fixed to 8 bytes, returning OK");
            return Ok(EndpointAddress::from_parts(0, UsbDirection::Out))
        }
        for index in ep_addr.map(|a| a.index()..a.index() + 1).unwrap_or(1..NUM_ENDPOINTS) {
            if !self.ep_allocs[index] {
                // only if there is memory that can accommodate the max_packet_size
                if let Some(offset) = self.alloc_region(max_packet_size as _) {
                    log::info!("allocated offset {:x}({})", offset, max_packet_size);
                    let mut ep_status = UdcEpStatus(0);
                    match ep_type {
                        EndpointType::Isochronous => ep_status.set_isochronous(true),
                        _ => ep_status.set_isochronous(false),
                    }
                    log::info!("alloc ep{}@{:x?} max_packet_size {}", index, offset, max_packet_size);
                    ep_status.set_head_offset(offset / 16);
                    ep_status.set_max_packet_size(max_packet_size as u32);
                    ep_status.set_enable(true);
                    if index == 0 {
                        ep_status.set_data_phase(true); // ep0 IN always responds on data phase 1
                    }

                    // setup descriptors from the yet-to-be-written ep config
                    let descriptor = self.descriptor_from_status(&ep_status);
                    descriptor.set_desc0(0);
                    descriptor.set_desc1(offset as usize / 16, max_packet_size as _);
                    descriptor.set_desc2(
                        ep_dir,
                        true,
                        true, // this should be equal to "packet_end", but this driver doesn't have that...?
                        index == 0, // only trigger for ep0 (per spinal linux driver)
                    );

                    // now commit the ep config
                    self.status_write_volatile(index,  UdcEpStatus(ep_status.0));
                    self.ep_allocs[index] = true;
                    // stash a copy of the ep0 IN config because the SETUP packet smashes it
                    if index == 0 {
                        self.ep0_copy = Some(ep_status);
                    }

                    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
                    return Ok(EndpointAddress::from_parts(index as usize, ep_dir))
                } else {
                    return Err(UsbError::EndpointMemoryOverflow);
                }
            }
        }
        // nothing matched, so there must be an error
        Err(match ep_addr {
            Some(_) => UsbError::InvalidEndpoint,
            None => UsbError::EndpointOverflow,
        })
    }

    /// Enables and initializes the USB peripheral. Soon after enabling the device will be reset, so
    /// there is no need to perform a USB reset in this method.
    fn enable(&mut self) {
        log::info!("enable");
        // clear all the interrupts in a single write
        self.regs.clear_all_interrupts();
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        // enable the interrupt
        let mut udc_config = self.regs.config();
        udc_config.set_enable_ints(true);
        self.regs.set_config(udc_config);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        // now confirm the config
        for (index, &ep) in self.ep_allocs.iter().enumerate() {
            if ep {
                let ep_status = self.status_read_volatile(index);
                log::info!("ep{}_status: {:?}", index, ep_status);
                let descriptor = self.descriptor_from_status(&ep_status);
                log::info!("desc{}: {:?}", index, descriptor);
            }
        }
        log::info!("{:?}", self.regs);
    }

    /// Called when the host resets the device. This will be soon called after
    /// [`poll`](crate::device::UsbDevice::poll) returns [`PollResult::Reset`]. This method should
    /// reset the state of all endpoints and peripheral flags back to a state suitable for
    /// enumeration, as well as ensure that all endpoints previously allocated with alloc_ep are
    /// initialized as specified.
    fn reset(&self) {
        log::info!("reset");
        for (index, &ep) in self.ep_allocs.iter().enumerate() {
            if ep {
                if index == 0 {
                    self.status_write_volatile(
                        0,
                        UdcEpStatus(self.ep0_copy.as_ref().expect("EP0 was not configured at reset").0));
                } else {
                    let ep_status = self.status_read_volatile(index);
                    let descriptor = self.descriptor_from_status(&ep_status);
                    if descriptor.offset() != 0 {
                        descriptor.set_desc0(0); // reset the pointer to 0
                    }
                }
                // log::info!("{:?}", ep_status);
                // log::info!("{:?}", descriptor);
            }
        }
        log::info!("{:?}", self.regs);
        // clear other registers
        // self.regs.set_address(0); // i think this is automatic in the USB core...
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Sets the device USB address to `addr`.
    fn set_device_address(&self, addr: u8) {
        log::info!("set_addr {}", addr);
        self.regs.set_address(addr as _);
    }

    /// Writes a single packet of data to the specified endpoint and returns number of bytes
    /// actually written.
    ///
    /// The only reason for a short write is if the caller passes a slice larger than the amount of
    /// memory allocated earlier, and this is generally an error in the class implementation.
    ///
    /// # Errors
    ///
    /// * [`InvalidEndpoint`](crate::UsbError::InvalidEndpoint) - The `ep_addr` does not point to a
    ///   valid endpoint that was previously allocated with [`UsbBus::alloc_ep`].
    /// * [`WouldBlock`](crate::UsbError::WouldBlock) - A previously written packet is still pending
    ///   to be sent.
    /// * [`BufferOverflow`](crate::UsbError::BufferOverflow) - The packet is too long to fit in the
    ///   transmission buffer. This is generally an error in the class implementation, because the
    ///   class shouldn't provide more data than the `max_packet_size` it specified when allocating
    ///   the endpoint.
    ///
    /// Implementations may also return other errors if applicable.
    fn write(&self, ep_addr: EndpointAddress, buf: &[u8]) -> Result<usize> {
        // restore the ep0 config because the SETUP transaction resets the settings here
        if ep_addr.index() == 0 {
            self.status_write_volatile(
                0,
                UdcEpStatus(self.ep0_copy.as_ref().expect("EP0 was not configured at reset").0));
            log::info!("restored ep0: {:?}", self.status_read_volatile(0));
        }

        let ep_status = self.status_read_volatile(ep_addr.index());
        if buf.len() > ep_status.max_packet_size() as usize {
            Err(UsbError::BufferOverflow)
        } else {
            let descriptor = self.descriptor_from_status(&ep_status);
            descriptor.set_desc0(0); // reset the write pointer to 0
            if buf.len() % 4 != 0 {
                // the linux driver doesn't handle anything other than word aligned, can we get away with it here?
                log::warn!("non word aligned buffer received, this needs to be handled");
            }
            for (index, src) in buf.chunks_exact(4).enumerate() {
                let w = u32::from_le_bytes(src.try_into().unwrap());
                descriptor.write_data(index, w);
            }
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            log::info!("ep{} write: {:x?}", ep_addr.index(), &buf);
            Ok(buf.len())
        }
    }

    /// Reads a single packet of data from the specified endpoint and returns the actual length of
    /// the packet.
    ///
    /// This should also clear any NAK flags and prepare the endpoint to receive the next packet.
    ///
    /// # Errors
    ///
    /// * [`InvalidEndpoint`](crate::UsbError::InvalidEndpoint) - The `ep_addr` does not point to a
    ///   valid endpoint that was previously allocated with [`UsbBus::alloc_ep`].
    /// * [`WouldBlock`](crate::UsbError::WouldBlock) - There is no packet to be read. Note that
    ///   this is different from a received zero-length packet, which is valid in USB. A zero-length
    ///   packet will return `Ok(0)`.
    /// * [`BufferOverflow`](crate::UsbError::BufferOverflow) - The received packet is too long to
    ///   fit in `buf`. This is generally an error in the class implementation, because the class
    ///   should use a buffer that is large enough for the `max_packet_size` it specified when
    ///   allocating the endpoint.
    ///
    /// Implementations may also return other errors if applicable.
    fn read(&self, ep_addr: EndpointAddress, buf: &mut [u8]) -> Result<usize> {
        log::trace!("read ep{} into buf of len {}", ep_addr.index(), buf.len());
        let ep_status = self.status_read_volatile(ep_addr.index());
        let descriptor = self.descriptor_from_status(&ep_status);
        if ep_addr.index() == 0 {
            let mut iters = 0;
            while descriptor.in_progress() {
                xous::yield_slice();
                iters += 1;
                if iters > 10_000 {
                    log::info!("ep{} in_progress timeout", 0);
                    break;
                }
            }
            // hard coded to 8 bytes in hardware
            if buf.len() < 8 {
                log::info!("ep0 read would overflow, aborting");
                return Err(UsbError::BufferOverflow)
            }
            // setup data is in a special, fixed location
            buf[..8].copy_from_slice(&self.get_setup());
            log::info!("ep0 read: {:x?}", &buf[..8]);
            log::info!("ep0 status: {:?}", ep_status);
            log::info!("desc0: {:?}", descriptor);
            Ok(8)
        } else {
            let mut iters = 0;
            while descriptor.in_progress() {
                xous::yield_slice();
                iters += 1;
                if iters > 10_000 {
                    log::info!("ep{} in_progress timeout", ep_addr.index());
                    break;
                }
            }
            let len = descriptor.offset();
            if len % 4 != 0 {
                log::info!("non-word aligned length encountered, need code to handle this case");
            }
            if buf.len() < len {
                log::error!("read ep{} would overflow: {} < {}", ep_addr.index(), buf.len(), len);
                return Err(UsbError::BufferOverflow)
            }
            for i in 0..len / 4 {
                log::info!("data {}", i);
                let word = descriptor.read_data(i).to_le_bytes();
                log::info!("is {:x?}", word);
                buf[i*4..(i+1)*4].copy_from_slice(&word);
                log::info!("copied slice");
            }
            descriptor.set_desc0(0); // reset the read pointer to 0
            log::info!("ep{} read: {:x?} (len {} into buf of {})", ep_addr.index(), &buf[..len], len, buf.len());
            Ok(len)
        }
    }

    /// Sets or clears the STALL condition for an endpoint. If the endpoint is an OUT endpoint, it
    /// should be prepared to receive data again.
    fn set_stalled(&self, ep_addr: EndpointAddress, stalled: bool) {
        if ep_addr.index() == 0 /* && */ /*(self.regs.interrupts().ep0_setup())*/ /*ep_addr.direction() == UsbDirection::Out*/ {
            log::debug!("not stalling ep0 during setup");
            return;
        }
        log::info!("set_stalled ep{}->{} dir {:?}", ep_addr.index(), stalled, ep_addr.direction());
        self.udc_hard_halt(ep_addr.index());
        let mut ep_status = self.status_read_volatile(ep_addr.index());
        match (stalled, ep_addr.direction()) {
            (true, UsbDirection::In) => {
                ep_status.set_force_stall(true);
            },
            (true, UsbDirection::Out) => ep_status.set_force_stall(true),
            (false, UsbDirection::In) => {
                ep_status.set_force_stall(false);
                ep_status.set_force_nack(true);
            },
            (false, UsbDirection::Out) => ep_status.set_force_stall(false),
        };
        // single volatile commit of the results
        self.status_write_volatile(ep_addr.index(), ep_status);
        self.udc_hard_unhalt(ep_addr.index());
    }

    /// Gets whether the STALL condition is set for an endpoint.
    fn is_stalled(&self, ep_addr: EndpointAddress) -> bool {
        let ep_status = self.status_read_volatile(ep_addr.index());
        log::info!("is_stalled{} -> {}", ep_addr.index(), ep_status.force_stall());
        ep_status.force_stall()
    }

    /// Causes the USB peripheral to enter USB suspend mode, lowering power consumption and
    /// preparing to detect a USB wakeup event. This will be called after
    /// [`poll`](crate::device::UsbDevice::poll) returns [`PollResult::Suspend`]. The device will
    /// continue be polled, and it shall return a value other than `Suspend` from `poll` when it no
    /// longer detects the suspend condition.
    fn suspend(&self) {
        log::warn!("USB suspend called, doing nothing");
    }

    /// Resumes from suspend mode. This may only be called after the peripheral has been previously
    /// suspended.
    fn resume(&self) {
        log::warn!("USB resume called, but suspend is not implemented");
    }

    /// Gets information about events and incoming data. Usually called in a loop or from an
    /// interrupt handler. See the [`PollResult`] struct for more information.
    fn poll(&self) -> PollResult {
        let interrupts = self.regs.interrupts();
        let mut ints_to_clear = UdcInterrupts(0);
        log::info!(">>>> frame {}: {:x?}", self.regs.frame_id(), interrupts);
        let poll_result =
        if interrupts.endpoint() != 0 || interrupts.ep0_setup() {
            let ep_setup = if interrupts.ep0_setup() {
                ints_to_clear.set_ep0_setup(true);
                1
            } else {0};
            let mut ep_in_complete = 0;

            let mut ep_out = 0;
            // all of them will be handled here, so, clear the interrupts as needed

            if interrupts.endpoint() != 0 {
                let mut bit = 0;
                loop {
                    if (interrupts.endpoint() & (1 << bit)) != 0 {
                        log::info!("ep{}", bit);
                        // form a descriptor from the memory range assigned to the EP
                        let ep_status = self.status_read_volatile(bit);
                        let descriptor = self.descriptor_from_status(&ep_status);
                        if descriptor.direction() == UsbDirection::Out {
                            ep_out |= 1 << bit;
                        } else {
                            ep_in_complete |= 1 << bit;
                        }
                        ints_to_clear.set_endpoint(1 << bit);
                        break;
                    }
                    bit += 1;
                }
            }
            PollResult::Data { ep_out, ep_in_complete, ep_setup }
        } else if interrupts.reset() {
            ints_to_clear.set_reset(true);
            log::trace!("aft reset: {:x?}", interrupts.0);
            PollResult::Reset
        } else if interrupts.resume() {
            ints_to_clear.set_reset(true);
            log::trace!("aft resume: {:x?}", interrupts.0);
            PollResult::Resume
        } else if interrupts.suspend() {
            ints_to_clear.set_suspend(true);
            log::trace!("aft suspend: {:x?}", interrupts.0);
            PollResult::Suspend
        } else if interrupts.disconnect() {
            ints_to_clear.set_disconnect(true);
            log::trace!("aft disconnect: {:x?}", interrupts.0);
            PollResult::Reset
        } else {
            PollResult::None
        };

        log::debug!("clearing ints: {:x?}", ints_to_clear);
        self.regs.clear_some_interrupts(ints_to_clear);
        if self.regs.interrupts().0 == 0 {
            log::debug!("all interrupts done");
        } else {
            log::info!("more interrupts to handle: {:x?}", self.regs.interrupts());
            // re-enter the interrupt handler to handle the next interrupt
            xous::try_send_message(self.conn,
                xous::Message::new_scalar(Opcode::UsbIrqHandler.to_usize().unwrap(), 0, 0, 0, 0)).ok();
        }

        // self.print_ep_stats();
        self.print_poll_result(&poll_result);
        poll_result
    }

    /// Simulates a disconnect from the USB bus, causing the host to reset and re-enumerate the
    /// device.
    ///
    /// The default implementation just returns `Unsupported`.
    ///
    /// # Errors
    ///
    /// * [`Unsupported`](crate::UsbError::Unsupported) - This UsbBus implementation doesn't support
    ///   simulating a disconnect or it has not been enabled at creation time.
    fn force_reset(&self) -> Result<()> {
        log::info!("force_reset");

        let mut cfg = UdcConfig(0);
        cfg.set_disable_ints(true);
        cfg.set_pullup_off(true);
        self.regs.set_config(cfg);

        self.tt.sleep_ms(5).unwrap();

        let mut cfg = UdcConfig(0);
        cfg.set_enable_ints(true); // to be enabled only when all the interrupts are handled
        cfg.set_pullup_on(true);
        self.regs.set_config(cfg);

        Ok(())
    }
}

