use susres::{ManagedMem, SuspendResume};
use usb_device::bus::PollResult;
use utralib::generated::*;
use crate::*;
use bitfield::bitfield;
use core::ops::{Deref, DerefMut};
use core::mem::size_of;
use core::slice;
use core::sync::atomic::{AtomicPtr, Ordering, AtomicBool};
use std::sync::{Arc, Mutex};
use usb_device::{class_prelude::*, Result, UsbDirection};
use std::collections::BTreeMap;

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

const DESC_OUT: bool = false;
const DESC_IN: bool = true;
/// This is located at 0x0000-0x0047 inside the UDC region
/// This is mostly documentation, it's not actually instantiated
#[allow(dead_code)]
#[repr(C)]
pub struct SpinalUdcMem {
    endpoints: [UdcEpStatus; 16],
    setup_data: [u8; 8],
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
#[repr(C)]
pub struct SpinalUdcDescriptorHeader {
    d0: UdcDesc0,
    d1: UdcDesc1,
    d2: UdcDesc2,
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

/// this is a set of pointers that are dynamically bound to a given endpoint
/// on demand
pub struct SpinalUdcEndpoint {
    ep_status: &'static mut UdcEpStatus,
    _interval: u8,
}

fn handle_usb(_irq_no: usize, arg: *mut usize) {
    let usb = unsafe { &mut *(arg as *mut SpinalUsbDevice) };
    let pending = usb.csr.r(utra::usbdev::EV_PENDING);
    let regs = unsafe{usb.regs.load(Ordering::SeqCst).as_mut().unwrap()};
    // report if we got a double-int condition or not
    if usb.was_checked.load(Ordering::SeqCst) {
        usb.int_err.store(false, Ordering::SeqCst)
    } else {
        usb.int_err.store(true, Ordering::SeqCst)
    }
    usb.was_checked.store(false, Ordering::SeqCst);
    usb.poll_result =
        if regs.interrupts.disconnect() {
            regs.interrupts.clear_disconnect(true);
            PollResult::Reset
        } else if regs.interrupts.resume() {
            regs.interrupts.clear_reset(true);
            PollResult::Resume
        } else if regs.interrupts.reset() {
            regs.interrupts.clear_reset(true);
            PollResult::Reset
        } else if regs.interrupts.suspend() {
            regs.interrupts.clear_suspend(true);
            PollResult::Suspend
        } else if regs.interrupts.endpoint() != 0 || regs.interrupts.ep0_setup() {
            let ep_setup = if regs.interrupts.ep0_setup() {
                regs.interrupts.clear_ep0_setup(true);
                1
            } else {0};
            let mut ep_in_complete = ep_setup; // mirror the value here

            let mut ep_out = 0;
            // all of them will be handled here, so, clear the interrupts as needed
            let ep_code = regs.interrupts.endpoint() as u16;
            regs.interrupts.clear_endpoint(ep_code as u32); // clear the interrupt bitmask
            let mut bit = 1;
            for maybe_ep in usb.eps.lock().unwrap().iter() {
                if bit & ep_code != 0 {
                    if let Some(ep) = maybe_ep {
                        // form a descriptor from the memory range assigned to the EP
                        let base = usb.usb.as_ptr();
                        let descriptor = unsafe {
                            (base.add(ep.ep_status.head_offset() as usize * 16) as *const SpinalUdcDescriptorHeader).as_ref().unwrap()
                        };
                        if descriptor.d2.direction() == DESC_OUT {
                            ep_out |= bit;
                        } else {
                            ep_in_complete |= bit;
                        }

                        // *** I *think* we want to halt the EP at this point so we can get the data wthout the memory being scribbled?

                    } else {
                        log::warn!("got packet on uninitialized endpoint: bitmask 0x{:x}", bit);
                    }
                }
                bit <<= 1;
            }
            PollResult::Data { ep_out, ep_in_complete, ep_setup }
        } else {
            PollResult::None
        };

    xous::try_send_message(usb.conn,
        xous::Message::new_scalar(Opcode::UsbIrqHandler.to_usize().unwrap(), 0, 0, 0, 0)).ok();
    usb.csr.wo(utra::usbdev::EV_PENDING, pending);
}

pub struct SpinalUsbMgmt {
    csr: AtomicCsr<u32>, // consider using VolatileCell and/or refactory AtomicCsr so it is non-mutable
    srmem: ManagedMem<{ utralib::generated::HW_USBDEV_MEM_LEN / core::mem::size_of::<u32>() }>,
    regs: AtomicPtr<SpinalUdcRegs>,
    int_err: Arc::<AtomicBool>,
    was_checked: Arc::<AtomicBool>,
}
impl SpinalUsbMgmt {
    pub fn print_regs(&self) {
        log::info!("control regs: {:x?}", self.regs);
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
}
pub struct SpinalUsbDevice {
    pub(crate) conn: CID,
    usb: xous::MemoryRange,
    csr_addr: u32,
    csr: AtomicCsr<u32>, // consider using VolatileCell and/or refactory AtomicCsr so it is non-mutable
    regs: AtomicPtr<SpinalUdcRegs>,
    // 1:1 mapping of endpoint structures to offsets in the memory space for the actual ep storage
    eps: Arc::<Mutex::<[Option<SpinalUdcEndpoint>; NUM_ENDPOINTS]>>,
    // structure to track space allocations within the memory space
    allocs: Arc::<Mutex::<BTreeMap<u32, u32>>>, // key is offset, value is len
    tt: ticktimer_server::Ticktimer,
    poll_result: PollResult,
    int_err: Arc::<AtomicBool>,
    was_checked: Arc::<AtomicBool>,
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
            regs: AtomicPtr::new(unsafe {
                (usb.as_mut_ptr().add(0xFF00) as *mut SpinalUdcRegs).as_mut().unwrap()
            }),
            eps: Arc::new(Mutex::new([
                // can't derive Copy on this, and also can't make a Default.
                // But # of eps is pretty damn static even though notionally we
                // use a NUM_ENDPOINTS to represent the value for readability, so, write it out long-form.
                None, None, None, None,
                None, None, None, None,
                None, None, None, None,
                None, None, None, None,
            ])),
            allocs: Arc::new(Mutex::new(BTreeMap::new())),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            poll_result: PollResult::None,
            int_err: Arc::new(AtomicBool::new(false)),
            was_checked: Arc::new(AtomicBool::new(false)),
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

        // also have to enable ints at the SpinalHDL layer
        let regs = unsafe{usbdev.regs.load(Ordering::SeqCst).as_mut().unwrap()};
        regs.config.set_enable_ints(true);

        usbdev
    }
    pub fn get_iface(&self) -> SpinalUsbMgmt {
        SpinalUsbMgmt {
            csr: AtomicCsr::new(self.csr_addr as *mut u32),
            srmem: ManagedMem::new(self.usb),
            regs: AtomicPtr::new(unsafe {
                (self.usb.as_mut_ptr().add(0xFF00) as *mut SpinalUdcRegs).as_mut().unwrap()
            }),
            int_err: self.int_err.clone(),
            was_checked: self.was_checked.clone(),
        }
    }
    fn print_poll_result(&self) {
        if self.int_err.load(Ordering::SeqCst) {
            log::info!("error was detected in interrupt handler: previous poll result was not used");
        }
        match self.poll_result {
            PollResult::None => log::info!("PollResult::None"),
            PollResult::Reset => log::info!("PollResult::Reset"),
            PollResult::Resume => log::info!("PollResult::Resume"),
            PollResult::Suspend => log::info!("PollResult::Suspend"),
            PollResult::Data {ep_out, ep_in_complete, ep_setup} =>
                log::info!("PollResult::Data out{:x} in{:x} setup{:x}", ep_out, ep_in_complete, ep_setup),
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
    /// returns `true` if the region was available to be deallocated
    pub fn dealloc_region(&mut self, offset: u32) -> bool {
        dealloc_inner(&mut self.allocs.lock().unwrap(), offset)
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
        interval: u8,
    ) -> Result<EndpointAddress> {
        // if ep_addr is specified, create a 1-unit range else a range through the entire space
        // note that ep_addr is a packed representation of index and direction,
        // so you must use `.index()` to get just the index part
        for index in ep_addr.map(|a| a.index()..a.index() + 1).unwrap_or(1..NUM_ENDPOINTS) {
            if self.eps.lock().unwrap()[index].is_some() {
                continue
            }
            // only if there is memory that can accommodate the max_packet_size
            if let Some(offset) = self.alloc_region(max_packet_size as _) {
                let ep = SpinalUdcEndpoint {
                    // Safety: the offset of the endpoint storage bank is defined as 0x0 + 4*index from the base of the
                    // usb memory area. Mapping UdcEpStatus here is safe assuming the structure has been correctly defined.
                    ep_status: unsafe {
                        (self.usb.as_mut_ptr().add(index * size_of::<UdcEpStatus>()) as *mut UdcEpStatus).as_mut().unwrap()
                    },
                    _interval: interval,
                };
                match ep_type {
                    EndpointType::Isochronous => ep.ep_status.set_isochronous(true),
                    _ => ep.ep_status.set_isochronous(false),
                }
                log::info!("setting ep{}@{:x?} max_packet_size {}", index, offset, max_packet_size);
                ep.ep_status.set_head_offset(offset / 16);
                ep.ep_status.set_max_packet_size(max_packet_size as u32);
                ep.ep_status.set_enable(true); // set the enable as the last op

                self.eps.lock().unwrap()[index] = Some(ep);
                return Ok(EndpointAddress::from_parts(index as usize, ep_dir))
            } else {
                return Err(UsbError::EndpointMemoryOverflow);
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
        let regs = unsafe{self.regs.load(Ordering::SeqCst).as_mut().unwrap()};
        // clear all the interrupts in a single write
        regs.interrupts.0 = 0xFFFF_FFFF;

        // clear other registers
        regs.address = 0;

        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        // enable the interrupt
        regs.config.set_enable_ints(true);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Called when the host resets the device. This will be soon called after
    /// [`poll`](crate::device::UsbDevice::poll) returns [`PollResult::Reset`]. This method should
    /// reset the state of all endpoints and peripheral flags back to a state suitable for
    /// enumeration, as well as ensure that all endpoints previously allocated with alloc_ep are
    /// initialized as specified.
    fn reset(&self) {
        let regs = unsafe{self.regs.load(Ordering::SeqCst).as_mut().unwrap()};
        // clear the endpoint RAM
        for ep in self.eps.lock().unwrap().iter_mut() {
            *ep = None;
        }
        self.allocs.lock().unwrap().clear();
        // set the RAM from 0x0-0xFF00 to all 0's
        let usbmem = unsafe{slice::from_raw_parts_mut(self.usb.as_mut_ptr(), 0xFF00)};
        for m in usbmem.iter_mut() {
            *m = 0;
        }
        // clear all the interrupts in a single write
        regs.interrupts.0 = 0xFFFF_FFFF;

        // clear other registers
        regs.address = 0;

        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Sets the device USB address to `addr`.
    fn set_device_address(&self, addr: u8) {
        let regs = unsafe{self.regs.load(Ordering::SeqCst).as_mut().unwrap()};
        regs.address = addr as u32;
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
        if let Some(ep) = &self.eps.lock().unwrap()[ep_addr.index()] {
            if buf.len() > ep.ep_status.max_packet_size() as usize {
                Err(UsbError::BufferOverflow)
            } else {
                let base = self.usb.as_mut_ptr();
                let descriptor = unsafe {
                    (base.add(ep.ep_status.head_offset() as usize * 16) as *mut SpinalUdcDescriptorHeader).as_mut().unwrap()
                };
                descriptor.d0.set_offset(0);
                descriptor.d1.set_next_offset(0);
                descriptor.d1.set_length(buf.len() as u32);
                descriptor.d2.set_direction(DESC_OUT);
                descriptor.d2.set_int_on_done(true);
                let data = unsafe {
                    slice::from_raw_parts_mut(
                        base.add(ep.ep_status.head_offset() as usize * 16 + size_of::<SpinalUdcDescriptorHeader>()),
                        ep.ep_status.max_packet_size() as usize
                )};
                for (&src, dst) in buf.iter().zip(data.iter_mut()) {
                    *dst = src;
                }
                Ok(buf.len())
            }
        } else {
            Err(UsbError::InvalidEndpoint)
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
        if let Some(ep) = &self.eps.lock().unwrap()[ep_addr.index()] {
            let base = self.usb.as_mut_ptr();
            let descriptor = unsafe {
                (base.add(ep.ep_status.head_offset() as usize * 16) as *mut SpinalUdcDescriptorHeader).as_mut().unwrap()
            };
            if descriptor.d0.code() == 0xF {
                log::info!("read called but descriptor was in progress")
            }
            descriptor.d1.set_next_offset(0);
            descriptor.d2.set_direction(DESC_IN);
            descriptor.d2.set_int_on_done(true);
            let len = descriptor.d0.offset() as usize;
            if len > buf.len() {
                Err(UsbError::BufferOverflow)
            } else {
                let data = unsafe {
                    slice::from_raw_parts_mut(
                        base.add(ep.ep_status.head_offset() as usize * 16 + size_of::<SpinalUdcDescriptorHeader>()),
                        ep.ep_status.max_packet_size() as usize
                )};
                for (&src, dst) in data[..len].iter().zip(buf.iter_mut()) {
                    *dst = src;
                }
                Ok(len)
            }
        } else {
            Err(UsbError::InvalidEndpoint)
        }
    }

    /// Sets or clears the STALL condition for an endpoint. If the endpoint is an OUT endpoint, it
    /// should be prepared to receive data again.
    fn set_stalled(&self, ep_addr: EndpointAddress, stalled: bool) {
        // it looks like a STALL condition could be forced even on unallocated endpoints, so
        // we alias into the register block and force it to happen.
        let ep_status = unsafe {
            (self.usb.as_mut_ptr().add(ep_addr.index() * size_of::<UdcEpStatus>()) as *mut UdcEpStatus).as_mut().unwrap()
        };

        match (stalled, ep_addr.direction()) {
            (true, UsbDirection::In) => {
                ep_status.set_force_nack(false);
                ep_status.set_force_stall(true);
            },
            (true, UsbDirection::Out) => ep_status.set_force_stall(true),
            (false, UsbDirection::In) => {
                ep_status.set_force_nack(true); // not sure if this is correct -- STM32 reference sets state to "nack" but the meaning might be different for this core
                ep_status.set_force_stall(false);
            },
            (false, UsbDirection::Out) => ep_status.set_force_stall(false),
        };
    }

    /// Gets whether the STALL condition is set for an endpoint.
    fn is_stalled(&self, ep_addr: EndpointAddress) -> bool {
        let ep_status = unsafe {
            (self.usb.as_mut_ptr().add(ep_addr.index() * size_of::<UdcEpStatus>()) as *mut UdcEpStatus).as_mut().unwrap()
        };
        ep_status.force_stall()
    }

    /// Causes the USB peripheral to enter USB suspend mode, lowering power consumption and
    /// preparing to detect a USB wakeup event. This will be called after
    /// [`poll`](crate::device::UsbDevice::poll) returns [`PollResult::Suspend`]. The device will
    /// continue be polled, and it shall return a value other than `Suspend` from `poll` when it no
    /// longer detects the suspend condition.
    fn suspend(&self) {
        unimplemented!(); // TODO
    }

    /// Resumes from suspend mode. This may only be called after the peripheral has been previously
    /// suspended.
    fn resume(&self) {
        unimplemented!(); // TODO
    }

    /// Gets information about events and incoming data. Usually called in a loop or from an
    /// interrupt handler. See the [`PollResult`] struct for more information.
    fn poll(&self) -> PollResult {
        if self.int_err.load(Ordering::SeqCst) {
            log::warn!("interrupt triggered twice before handler got to it; an event was lost");
        }
        self.was_checked.store(true, Ordering::SeqCst);
        self.print_poll_result();
        // because the library type does not implement Copy or Clone
        match self.poll_result {
            PollResult::None => PollResult::None,
            PollResult::Reset => PollResult::Reset,
            PollResult::Resume => PollResult::Resume,
            PollResult::Suspend => PollResult::Suspend,
            PollResult::Data {ep_out, ep_in_complete, ep_setup} =>
                PollResult::Data{ep_out, ep_in_complete, ep_setup},
        }
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
        self.csr.wfo(utra::usbdev::USBSELECT_USBSELECT, 0);
        self.tt.sleep_ms(5).unwrap();
        self.csr.wfo(utra::usbdev::USBSELECT_USBSELECT, 1);
        Ok(())
    }
}

