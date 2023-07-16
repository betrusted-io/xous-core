use usb_device::bus::PollResult;
use utralib::generated::*;
use crate::*;
use core::{sync::atomic::{AtomicPtr, Ordering, AtomicUsize, AtomicU16, AtomicBool}, mem::size_of};
use std::sync::{Arc, Mutex};
use usb_device::{class_prelude::*, Result, UsbDirection};
use std::collections::BTreeMap;
use std::convert::TryInto;
use xous::MemoryRange;

const WRITE_TIMEOUT_MS: u64 = 20;
/// This can be done exactly once, even if there are multiple views. Track if it's been done yet.
static INTERRUPT_INIT_DONE: AtomicBool = AtomicBool::new(false);

fn handle_usb(_irq_no: usize, arg: *mut usize) {
    let usb = unsafe { &mut *(arg as *mut SpinalUsbDevice) };
    let pending = usb.csr.r(utra::usbdev::EV_PENDING);

    // actual interrupt handling is done in userspace, this just triggers the routine

    usb.csr.wo(utra::usbdev::EV_PENDING, pending);

    xous::try_send_message(usb.conn,
        xous::Message::new_scalar(Opcode::UsbIrqHandler.to_usize().unwrap(), 0, 0, 0, 0)).ok();
}

pub struct SpinalUsbMgmt {
    csr: AtomicCsr<u32>,
    usb: AtomicPtr<u8>,
    eps: AtomicPtr<UdcEpStatus>,
    // can't use the srmem macro because the memory region window is not exactly the size of the memory to be stored.
    sr_descs: [u32; crate::END_OFFSET as usize / size_of::<u32>()],
    sr_debug: bool,
    sr_core: bool,
    regs: SpinalUdcRegs,
    tt: ticktimer_server::Ticktimer,
}
impl SpinalUsbMgmt {
    #[allow(dead_code)]
    pub fn print_regs(&self) {
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
        log::debug!("frame id: {}", self.regs.frame_id());
        log::debug!("usb addr: {}", self.regs.address());
        log::debug!("ints: {:x?}", self.regs.interrupts());
        log::debug!("halt: 0x{:x?}", self.regs.halt());
        log::debug!("config: 0x{:x?}", self.regs.config());
        log::debug!("ramsize: {}", self.regs.ramsize());
        assert!(4096 == self.regs.ramsize(), "hardware ramsize parameter does not match our expectations");
    }
    /// Force and hold the reset pin according to the state selected
    pub fn ll_reset(&mut self, state: bool) {
        self.csr.rmwf(utra::usbdev::USBSELECT_FORCE_RESET, if state {1} else {0});
    }
    /// does not encode resets, leaves timing to the caller
    pub fn ll_connect_device_core(&mut self, state: bool) {
        if state {
            log::trace!("connecting USB device core");
            self.csr.rmwf(utra::usbdev::USBSELECT_SELECT_DEVICE, 1);
        } else {
            log::trace!("connecting USB debug core");
            self.csr.rmwf(utra::usbdev::USBSELECT_SELECT_DEVICE, 0);
        }
    }
    pub fn connect_device_core(&mut self, state: bool) {
        self.sr_core = state;
        log::trace!("previous state: {}", self.csr.rf(utra::usbdev::USBSELECT_SELECT_DEVICE));
        self.csr.rmwf(utra::usbdev::USBSELECT_FORCE_RESET, 1);
        // give some time for the host to recognize that we've unplugged before swapping our behavior out
        self.tt.sleep_ms(1000).unwrap();
        if state {
            log::trace!("connecting USB device core");
            self.csr.rmwf(utra::usbdev::USBSELECT_SELECT_DEVICE, 1);
        } else {
            log::trace!("connecting USB debug core");
            self.csr.rmwf(utra::usbdev::USBSELECT_SELECT_DEVICE, 0);
        }
        self.tt.sleep_ms(1000).unwrap();
        self.csr.rmwf(utra::usbdev::USBSELECT_FORCE_RESET, 0);
    }
    pub fn is_device_connected(&self) -> bool {
        if self.csr.rf(utra::usbdev::USBSELECT_SELECT_DEVICE) == 1 {
            true
        } else {
            false
        }
    }
    pub fn disable_debug(&mut self, disable: bool) {
        if disable {
            self.csr.wfo(utra::usbdev::USBDISABLE_USBDISABLE, 1);
        } else {
            self.csr.wfo(utra::usbdev::USBDISABLE_USBDISABLE, 0);
        }
    }
    pub fn get_disable_debug(&self) -> bool {
        if self.csr.rf(utra::usbdev::USBDISABLE_USBDISABLE) == 0 {
            false
        } else {
            true
        }
    }
    pub fn xous_suspend(&mut self) {
        // log::set_max_level(log::LevelFilter::Debug);
        // self.print_regs();
        // the core select state is updated in `connect_device_core`, we don't update it here
        // but we do record the debug state.
        self.sr_debug = self.get_disable_debug();
        self.csr.wo(utra::usbdev::EV_PENDING, 0xFFFF_FFFF);
        self.csr.wo(utra::usbdev::EV_ENABLE, 0x0);
        // the descriptor window is manually managed because the sr macro does the whole window
        // it turns out only the bottom 4k is relevant; the actual state registers should be
        // the naive reset values by right, because we would be entering into this from a
        // "reset"/disconnected state anyways
        let ptr = self.usb.load(Ordering::SeqCst) as *const u32;
        for (index, d) in self.sr_descs.iter_mut().enumerate() {
            *d = unsafe{ptr.add(index).read_volatile()};
        }
    }
    pub fn xous_resume1(&mut self) {
        self.regs.clear_all_interrupts();
        let p = self.csr.r(utra::usbdev::EV_PENDING); // this has to be expanded out because AtomicPtr is potentially mutable on read
        self.csr.wo(utra::usbdev::EV_PENDING, p); // clear in case it's pending for some reason
        self.csr.wfo(utra::usbdev::EV_ENABLE_USB, 1);

        let mut cfg = UdcConfig(0);
        cfg.set_pullup_on(true); // required for proper operation
        self.regs.set_config(cfg);

        let ptr = self.usb.load(Ordering::SeqCst) as *mut u32;
        for (index, &s) in self.sr_descs.iter().enumerate() {
            unsafe{ptr.add(index).write_volatile(s)};
        }
        self.regs.clear_all_interrupts();
        // self.print_regs();
        // log::set_max_level(log::LevelFilter::Info);
    }
    pub fn xous_resume2(&mut self) {
        self.disable_debug(self.sr_debug);
        self.connect_device_core(self.sr_core);
    }
    #[allow(dead_code)]
    pub fn descriptor_from_status(&self, ep_status: &UdcEpStatus) -> SpinalUdcDescriptor {
        SpinalUdcDescriptor::new(
            unsafe{ self.usb.load(Ordering::SeqCst).add(
                ep_status.head_offset() as usize * 16
            ) as *mut u32}
        )
    }
    #[allow(dead_code)]
    pub fn status_from_index(&self, index: usize) -> UdcEpStatus {
        unsafe {
            self.eps.load(Ordering::SeqCst).add(index).read_volatile()
        }
    }
}

#[derive(Copy, Clone)]
struct EpProps {
    pub head_offset: usize,
    pub packet_len: usize,
    pub max_chain: usize,
    pub ep_type: EndpointType,
    pub ep_addr: EndpointAddress,
    pub ep_dir: UsbDirection,
}

struct AllocView {
    // tracks which endpoints have been allocated. ep0 is special. parameter is the maximum size buffer available.
    // parameter is (address, len, chain_len); chain_len is the maximum number of `len`-length packets that could be chained
    ep_allocs: [Option<EpProps>; 16],
    // record a copy of the ep0 IN setup descriptor address - could extract from ep_allocs[0], but it's here for legacy reasons
    ep0in_head: u32,
    // structure to track space allocations within the memory space
    allocs: Arc::<Mutex::<BTreeMap<u32, u32>>>, // key is offset, value is len
}
impl Default for AllocView {
    fn default() -> Self {
        AllocView {
            ep_allocs: [None; 16],
            ep0in_head: 0,
            allocs: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

pub struct SpinalUsbDevice {
    pub(crate) conn: xous::CID,
    usb: xous::MemoryRange,
    csr_addr: u32,
    csr: AtomicCsr<u32>,
    regs: SpinalUdcRegs,
    // 1:1 mapping of endpoint structures to offsets in the memory space for the actual ep storage
    // data must be committed to this in a single write, and not composed dynamically using this as scratch space
    eps: AtomicPtr<UdcEpStatus>,
    // different views into allocation, depending on the configuration
    view: AllocView,
    tt: ticktimer_server::Ticktimer,
    address: AtomicUsize,
    // bit vector to track if a read is allowed. This prevents a race condition between polled reads and interrupted reads.
    read_allowed: AtomicU16,
    // last descriptor in write (IN packet; device->host) chain. Used to check if a transaction is in progress.
    last_wr_desc: Arc::<Mutex<[Option<SpinalUdcDescriptor>; 16]>>,
    // last descriptor in read (OUT packet; host->device) chain. Used to check if a transaction is in progress.
}
impl SpinalUsbDevice {
    pub fn new(sid: xous::SID, usb: MemoryRange, csr: MemoryRange) -> SpinalUsbDevice {
        SpinalUsbDevice {
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
            view: AllocView::default(),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            address: AtomicUsize::new(0),
            read_allowed: AtomicU16::new(0),
            last_wr_desc: Arc::new(Mutex::new([None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,])),
        }
    }
    pub fn init(&self) {
        if !INTERRUPT_INIT_DONE.fetch_or(true, Ordering::SeqCst) {
            xous::claim_interrupt(
                utra::usbdev::USBDEV_IRQ,
                handle_usb,
                self as *const SpinalUsbDevice as *mut usize,
            )
            .expect("couldn't claim irq");
            let p = self.csr.r(utra::usbdev::EV_PENDING);
            self.csr.wo(utra::usbdev::EV_PENDING, p); // clear in case it's pending for some reason
            self.csr.wfo(utra::usbdev::EV_ENABLE_USB, 1);
        }
        let mut cfg = UdcConfig(0);
        cfg.set_pullup_on(true); // required for proper operation
        self.regs.set_config(cfg);
    }
    pub fn get_iface(&self) -> SpinalUsbMgmt {
        SpinalUsbMgmt {
            csr: AtomicCsr::new(self.csr_addr as *mut u32),
            usb: AtomicPtr::new(self.usb.as_mut_ptr() as *mut u8),
            eps: AtomicPtr::new(unsafe {
                (self.usb.as_mut_ptr().add(0x00) as *mut UdcEpStatus).as_mut().unwrap()
            }),
            sr_descs: [0u32; crate::END_OFFSET as usize / size_of::<u32>()],
            sr_core: false, // device core is not selected by default
            sr_debug: true, // debug is enabled by default
            regs: self.regs.clone(),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
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
        log::debug!("<<<< {}", info);
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
    #[cfg(feature="mjolnir")]
    pub fn ll_debug(&self) {
        let mut regblock = String::new();
        let regs = unsafe{self.usb.as_mut_ptr().add(0xFF00) as *const u32};
        regblock.push_str("\r\nRegs:\r\n");
        for i in 0..16 {
            regblock.push_str(
                &format!("{:08x}, {:08x},\r\n",
                unsafe{regs.add(i*2).read_volatile()},
                unsafe{regs.add(i*2 +1).read_volatile()}));
        }
        log::info!("{}", regblock);
        let mut eps = String::from("\r\nEps/descs:");
        let d = unsafe{self.usb.as_mut_ptr().add(0) as *const u32};
        for i in 0..72 {
            if i % 4 == 0 {
                eps.push_str("\r\n");
            }
            eps.push_str(&format!("{:08x}, ", unsafe{d.add(i).read_volatile()}));
        }
        eps.push_str("\r\n");
        log::info!("{}", eps);
    }
    /// simple but easy to understand allocator for buffers inside the descriptor memory space
    /// See notes inside src/main.rs `alloc_inner` for the functional description. Returns
    /// the full byte-addressed offset of the region, so it must be shifted to the right by
    /// 4 before being put into a SpinalHDL descriptor (it uses 16-byte alignment and thus
    /// discards the lower 4 bits).
    pub fn alloc_region(&mut self, requested: u32) -> Option<u32> {
        alloc_inner(&mut self.view.allocs.lock().unwrap(), requested)
    }
    #[allow(dead_code)]
    /// returns `true` if the region was available to be deallocated
    pub fn dealloc_region(&mut self, offset: u32) -> bool {
        dealloc_inner(&mut self.view.allocs.lock().unwrap(), offset)
    }
    pub(crate) fn descriptor_from_status(&self, ep_status: &UdcEpStatus) -> SpinalUdcDescriptor {
        SpinalUdcDescriptor::new(
            unsafe{ self.usb.as_mut_ptr().add(
                ep_status.head_offset() as usize * 16
            ) as *mut u32}
        )
    }
    /// overhead of a spinal descriptor in bytes
    pub(crate) fn descriptor_overhead_bytes(&self) -> usize { 12 }
    /// create a descriptor from the head address + chain offset
    pub(crate) fn descriptor_from_head_and_chain(&self, head_offset: usize, chain_offset: usize, packet_len: usize) -> SpinalUdcDescriptor {
        SpinalUdcDescriptor::new(
            unsafe{ self.usb.as_mut_ptr().add(
                head_offset * 16
                // in the case that packet_len is exactly a multiple of 16 plus 4 bytes, it will waste 16 bytes: a 20-byte packet could fit in 32 bytes, but we ask for 48 instead
                // if changed, this also needs to be updated in alloc_ep()
                + (((packet_len + self.descriptor_overhead_bytes()) / 16) + 1) * 16 * chain_offset
            ) as *mut u32}
        )
    }
    /// compute the *next* address offset for the *current* chain and packet length. Field is pre-shifted so
    /// this is directly passed to `set_next_desc_and_len()`
    pub(crate) fn next_from_head_and_chain(&self, head_offset: usize, chain_offset: usize, packet_len: usize) -> usize {
        head_offset * 16
        // in the case that packet_len is exactly a multiple of 16 plus 4 bytes, it will waste 16 bytes: a 20-byte packet could fit in 32 bytes, but we ask for 48 instead
        // if changed, this also needs to be updated in alloc_ep()
        + ((((packet_len + self.descriptor_overhead_bytes()) / 16) + 1) * 16 * (chain_offset + 1)) >> 4
    }
    /// A dedicated, fixed descriptor that represents EP0 acting as the 0-length OUT to accept the acknowledgement
    /// of IN data write complete. The location of this is at the very top of descriptor space.
    pub(crate) fn descriptor_ep0_out(&self) -> SpinalUdcDescriptor {
        SpinalUdcDescriptor::new(
            unsafe{ self.usb.as_mut_ptr().add(
                self.ep0_out_offset() * 16
            ) as *mut u32}
        )
    }
    /// This descriptor is in a fixed location, 0x50. Divide by 16 because all descriptors are 16-byte aligned
    /// and the bottom 0's are dropped in the register format.
    pub(crate) fn ep0_out_offset(&self) -> usize { 0x50 / 16 }
    /// Reset the EP0 OUT descriptor to its default settings. This is necessary because after the
    /// descriptor is "used up" it has to be reset.
    pub(crate) fn ep0_out_reset(&self) {
        let ep0_out_desc = self.descriptor_ep0_out();
        ep0_out_desc.set_offset(0);
        ep0_out_desc.set_next_desc_and_len(0, 0);
        ep0_out_desc.set_desc_flags(UsbDirection::Out, true, true, true);
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

const CHAINED_BUF_LEN: usize = 512;
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
    ///   endpoints, endpoints of the specified type, or endpoint packet memory has been exhausted.
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
        log::debug!("alloc ep spec: {:?} of type {:?} dir {:?}", ep_addr, ep_type, ep_dir);
        if ep_addr == Some(EndpointAddress::from_parts(0, UsbDirection::Out)) { // flag the control in as a special case
            log::debug!("ep0 allocation fixed to 8 bytes, returning OK");
            // setup the default ep0 out descriptor but leave it unlinked
            self.ep0_out_reset();
            return Ok(EndpointAddress::from_parts(0, UsbDirection::Out))
        }
        for index in ep_addr.map(|a| a.index()..a.index() + 1).unwrap_or(1..NUM_ENDPOINTS) {
            if self.view.ep_allocs[index].is_none() {
                let base_packet_size = max_packet_size;
                let max_packet_size = match ep_type {
                    EndpointType::Bulk =>
                        (CHAINED_BUF_LEN as u16 / base_packet_size)
                        // in the case that packet_len is exactly a multiple of 16 plus 4 bytes, it will waste 16 bytes: a 20-byte packet could fit in 32 bytes, but we ask for 48 instead
                        // if changed, this also needs to be updated in descriptor_from_status_and_chain()
                        * (((base_packet_size + self.descriptor_overhead_bytes() as u16) / 16) + 1) * 16,
                    _ => max_packet_size
                };
                // only if there is memory that can accommodate the max_packet_size
                if let Some(offset) = self.alloc_region(max_packet_size as _) {
                    log::debug!("allocated offset {:x}({})", offset, max_packet_size);
                    let mut ep_status = UdcEpStatus(0);
                    match ep_type {
                        EndpointType::Isochronous => ep_status.set_isochronous(true),
                        _ => ep_status.set_isochronous(false),
                    }
                    log::debug!("alloc ep{}@{:x?}{} max_packet_size {}",
                        index,
                        offset,
                        match ep_dir {
                            UsbDirection::In => "IN",
                            UsbDirection::Out => "OUT",
                        },
                        max_packet_size
                    );
                    ep_status.set_head_offset(offset / 16);
                    ep_status.set_max_packet_size(max_packet_size as u32);
                    ep_status.set_enable(true);
                    if index == 0 {
                        ep_status.set_data_phase(true); // ep0 IN always responds on data phase 1
                    }

                    // setup descriptors from the yet-to-be-written ep config
                    let descriptor = self.descriptor_from_status(&ep_status);
                    descriptor.set_offset_only(0);
                    descriptor.set_next_desc_and_len(0, max_packet_size as _);
                    descriptor.set_desc_flags(
                        ep_dir,
                        ep_dir == UsbDirection::Out,
                        true, // this should be equal to "packet_end", but this driver doesn't have that...?
                        index == 0, // only trigger for ep0 (per spinal Linux driver)
                    );
                    // clear the descriptor to 0
                    let init_vec = vec![0u8; max_packet_size as _];
                    for (index, src) in init_vec.chunks_exact(4).enumerate() {
                        let w = u32::from_le_bytes(src.try_into().unwrap());
                        descriptor.write_data(index, w);
                    }
                    if init_vec.len() % 4 != 0 { // handle the odd remainder case
                        let mut remainder = [0u8; 4];
                        for (index, &src) in init_vec.chunks_exact(4).remainder().iter().enumerate() {
                            remainder[index] = src;
                        }
                        descriptor.write_data(init_vec.len() / 4, u32::from_le_bytes(remainder));
                    }

                    if index != 0 { // disable transmission on the In descriptor as there's nothing to send
                        if ep_dir == UsbDirection::In {
                            ep_status.set_head_offset(0);
                        }
                    }

                    if index == 0 {
                        // stash a copy of the ep0 IN head location, because the SETUP packet resets this to 0
                        self.view.ep0in_head = ep_status.head_offset();
                    }

                    // now commit the ep config
                    self.status_write_volatile(index, ep_status);
                    self.view.ep_allocs[index] = Some(
                        EpProps {
                            head_offset: offset as usize / 16,
                            packet_len: base_packet_size as usize,
                            max_chain: match ep_type {
                                EndpointType::Bulk => CHAINED_BUF_LEN / base_packet_size as usize,
                                _ => 1,
                            },
                            ep_type,
                            ep_addr: EndpointAddress::from(index as u8),
                            ep_dir,
                        }
                    );

                    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
                    log::info!("alloc ep{} type {:?} dir {:?}, @{:x}({})",
                        index, ep_type, ep_dir, offset, max_packet_size);
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
    }

    /// Called when the host resets the device. This will be soon called after
    /// [`poll`](crate::device::UsbDevice::poll) returns [`PollResult::Reset`]. This method should
    /// reset the state of all endpoints and peripheral flags back to a state suitable for
    /// enumeration, as well as ensure that all endpoints previously allocated with alloc_ep are
    /// initialized as specified.
    fn reset(&self) {
        log::info!("USB reset");
        self.regs.set_address(0x0); // this does *not* require the trigger
        self.address.store(0, Ordering::SeqCst);
        self.ep0_out_reset();
        for (index, &ep) in self.view.ep_allocs.iter().enumerate().rev() {
            if let Some(ep_props) = ep {
                if index == 0 {
                    log::trace!("ep0 reset");
                    self.ep0_out_reset();
                } else {
                    assert!(index as u8 == ep_props.ep_addr.into(), "EP record stored in incorrect location in ep_allocs array!");
                    let mut ep_status = UdcEpStatus(0); // rebuild from scratch, not from self.status_read_volatile(index);
                    match ep_props.ep_type {
                        EndpointType::Isochronous => ep_status.set_isochronous(true),
                        _ => ep_status.set_isochronous(false),
                    }
                    ep_status.set_head_offset(ep_props.head_offset as u32);
                    ep_status.set_max_packet_size(ep_props.packet_len as u32);
                    ep_status.set_enable(true);
                    if index == 0 {
                        ep_status.set_data_phase(true); // ep0 IN always responds on data phase 1
                    }

                    // setup descriptors from the yet-to-be-written ep config
                    let descriptor = self.descriptor_from_status(&ep_status);
                    descriptor.set_offset_only(0);
                    descriptor.set_next_desc_and_len(0, ep_props.packet_len as _);
                    descriptor.set_desc_flags(
                        ep_props.ep_dir,
                        ep_props.ep_dir == UsbDirection::Out,
                        true, // this should be equal to "packet_end", but this driver doesn't have that...?
                        index == 0, // only trigger for ep0 (per spinal Linux driver)
                    );
                    // clear the descriptor to 0
                    let init_vec = vec![0u8; ep_props.packet_len as _];
                    for (index, src) in init_vec.chunks_exact(4).enumerate() {
                        let w = u32::from_le_bytes(src.try_into().unwrap());
                        descriptor.write_data(index, w);
                    }
                    if init_vec.len() % 4 != 0 { // handle the odd remainder case
                        let mut remainder = [0u8; 4];
                        for (index, &src) in init_vec.chunks_exact(4).remainder().iter().enumerate() {
                            remainder[index] = src;
                        }
                        descriptor.write_data(init_vec.len() / 4, u32::from_le_bytes(remainder));
                    }

                    if index != 0 { // disable transmission on the In descriptor as there's nothing to send
                        if ep_props.ep_dir == UsbDirection::In {
                            ep_status.set_head_offset(0);
                        }
                    }
                    // now commit the ep config
                    self.status_write_volatile(index, ep_status);
                }
            } else {
                let mut ep_status = self.status_read_volatile(index);
                ep_status.set_enable(false);
                self.status_write_volatile(index, ep_status);
            }
        }
        if false {
            // Config confirmation for debug (change above to `true`)
            for (index, &ep) in self.view.ep_allocs.iter().enumerate() {
                if let Some(ep_props) = ep {
                    let mut ep_status = self.status_read_volatile(index);
                    ep_status.set_head_offset(ep_props.head_offset as u32);
                    log::info!("ep{}_status: {:?}", index, ep_status);
                    let descriptor = self.descriptor_from_status(&ep_status);
                    log::info!("desc{}: {:?}", index, descriptor);
                }
            }
        }
        #[cfg(feature="mjolnir")]
        self.ll_debug(); // this is the "memory dump" of the USB descriptor area.
        // clear other registers
        // self.regs.set_address(0); // i think this is automatic in the USB core...
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Sets the device USB address to `addr`.
    fn set_device_address(&self, addr: u8) {
        // note: this core requires the address setting to be done right after the ep0 SETUP
        // packet that specifies setting up an address. Therefore, this call is a dummy.
        self.address.store(addr as usize, Ordering::SeqCst);
        log::debug!("set_addr dummy {}", addr);
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
        if let Some(ep_props) = self.view.ep_allocs[ep_addr.index()] {
            if buf.len() > ep_props.max_chain * ep_props.packet_len {
                Err(UsbError::BufferOverflow)
            } else if buf.len() <= ep_props.packet_len {
                // TODO: merge this with the path below, mostly to reduce code space. This was kept "intact" so enumeration
                // would work while we figured out the details of chaining on bulk transfers.
                #[cfg(feature="mjolnir")] // mjolnir is so powerful, one must halt the USB core entirely for it to be wielded
                if ep_addr.index() == 1 { self.udc_hard_halt(ep_addr.index()); }
                let mut ep_status = self.status_read_volatile(ep_addr.index());

                if ep_addr.index() != 0 {
                    let start = self.tt.elapsed_ms();
                    let mut now = start;
                    while (ep_status.head_offset() != 0)
                    && (now - start < WRITE_TIMEOUT_MS) {
                        xous::yield_slice();
                        ep_status = self.status_read_volatile(ep_addr.index());
                        now = self.tt.elapsed_ms();
                    }
                    if now - start >= WRITE_TIMEOUT_MS {
                        log::debug!("write wait timed out: head offset {:x?} {:x?}", ep_status.head_offset() * 16,
                        self.last_wr_desc.lock().unwrap()[ep_addr.index()].as_ref().unwrap());
                    }
                }

                // this is reset to 0 after every transaction by the hardware, so we must reset it
                ep_status.set_head_offset(ep_props.head_offset as u32);
                let descriptor = self.descriptor_from_status(&ep_status);
                if ep_addr.index() != 0 {
                    descriptor.set_desc_flags(UsbDirection::In,
                        false, false, false);
                } else {
                    descriptor.set_desc_flags(UsbDirection::In,
                        true, true, false);
                }
                for (index, src) in buf.chunks_exact(4).enumerate() {
                    let w = u32::from_le_bytes(src.try_into().unwrap());
                    descriptor.write_data(index, w);
                }
                if buf.len() % 4 != 0 { // handle the odd remainder case
                    let mut remainder = [0u8; 4];
                    for (index, &src) in buf.chunks_exact(4).remainder().iter().enumerate() {
                        remainder[index] = src;
                    }
                    descriptor.write_data(buf.len() / 4, u32::from_le_bytes(remainder));
                }

                ep_status.set_max_packet_size(ep_props.packet_len as _);
                descriptor.set_next_desc_and_len(0, buf.len());
                if ep_addr.index() != 0 {
                    descriptor.set_desc_flags(UsbDirection::In,
                        false, true, false);
                }
                descriptor.set_offset(0); // reset the write pointer to 0, also sets in_progress
                self.last_wr_desc.lock().unwrap()[ep_addr.index()] = Some(descriptor);
                //if ep_addr.index() != 0 {
                //    log::info!("WR PREdesc{}: {:?}", ep_addr.index(), descriptor);
                //}
                // this is required to commit the ep_status record once all the setup is done
                self.status_write_volatile(ep_addr.index(), ep_status);

                #[cfg(feature="mjolnir")]
                { // mjolnir is so powerful, one must halt the USB core entirely for it to be wielded
                    if ep_addr.index() == 1 {
                        self.ll_debug();
                    }
                    if ep_addr.index() == 1 { self.udc_hard_unhalt(ep_addr.index()); }
                }

                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

                // I think the debug statements actually mess up the timing of the core (as it causes contention for the same
                // memory resources used by the core itself), so they are commented out.
                //let epcheck = self.status_read_volatile(ep_addr.index());
                //let epcheckdesc = self.descriptor_from_status(&epcheck);
                //if ep_addr.index() != 0 {
                //    log::info!("WR status{}: {:?}", ep_addr.index(), epcheck);
                //    log::info!("WR desc{}: {:?}", ep_addr.index(), epcheckdesc);
                //    log::info!("WR POSTstatus{}: {:?}", ep_addr.index(), self.status_read_volatile(ep_addr.index()));
                //}
                Ok(buf.len())
            } else {
                assert!(ep_addr.index() != 0); // long buffers are not allowed on EP0
                let mut ep_status = self.status_read_volatile(ep_addr.index());
                let start = self.tt.elapsed_ms();
                let mut now = start;
                while (ep_status.head_offset() != 0) && (now - start < WRITE_TIMEOUT_MS) {
                    xous::yield_slice();
                    ep_status = self.status_read_volatile(ep_addr.index());
                    now = self.tt.elapsed_ms();
                }
                if now - start >= WRITE_TIMEOUT_MS {
                    log::debug!("ll write wait timed out: head offset {:x?} {:x?}", ep_status.head_offset() * 16,
                    self.last_wr_desc.lock().unwrap()[ep_addr.index()].as_ref().unwrap());
                }

                for (chain_offset, sub_buf) in buf.chunks(ep_props.packet_len).enumerate() {
                    let descriptor = self.descriptor_from_head_and_chain(ep_props.head_offset, chain_offset, ep_props.packet_len);
                    descriptor.set_desc_flags(UsbDirection::In,
                        false, true, false);
                    for (index, src) in sub_buf.chunks_exact(4).enumerate() {
                        let w = u32::from_le_bytes(src.try_into().unwrap());
                        descriptor.write_data(index, w);
                    }
                    if sub_buf.len() % 4 != 0 { // handle the odd remainder case
                        let mut remainder = [0u8; 4];
                        for (index, &src) in sub_buf.chunks_exact(4).remainder().iter().enumerate() {
                            remainder[index] = src;
                        }
                        descriptor.write_data(sub_buf.len() / 4, u32::from_le_bytes(remainder));
                    }

                    if chain_offset == (ep_props.max_chain - 1) || sub_buf.len() != ep_props.packet_len {
                        // on the very last packet (either by chain exhaustion, or buffer source exhaustion), set completion_on_full interrupt
                        descriptor.set_next_desc_and_len(0, sub_buf.len());
                    } else {
                        // if not the last packet, setup the linked list pointer
                        let next_addr = self.next_from_head_and_chain(ep_props.head_offset, chain_offset, ep_props.packet_len);
                        descriptor.set_next_desc_and_len(next_addr, sub_buf.len());
                    }
                    descriptor.set_offset(0); // reset the write pointer to 0, also sets in_progress
                    log::debug!("bulk descriptor {} setup: {:x?}", chain_offset, descriptor);
                    self.last_wr_desc.lock().unwrap()[ep_addr.index()] = Some(descriptor);
                }

                ep_status.set_max_packet_size(ep_props.packet_len as _);
                // this is reset to 0 after every transaction by the hardware, so we must reset it
                ep_status.set_head_offset(ep_props.head_offset as u32);

                // this is required to commit the ep_status record once all the setup is done
                self.status_write_volatile(ep_addr.index(), ep_status);

                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
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
        log::trace!("read ep{} into buf of len {}", ep_addr.index(), buf.len());
        if let Some(ep_props) = self.view.ep_allocs[ep_addr.index()] {
            if ep_addr.index() == 0 {
                if buf.len() == 0 {
                    log::info!("STATUS dummy read");
                    // it's a STATUS read, just ack and move on
                    return Ok(0)
                }
                // hard coded to 8 bytes in hardware
                if buf.len() < 8 {
                    log::info!("ep0 read would overflow, aborting");
                    return Err(UsbError::BufferOverflow)
                }
                // setup data is in a special, fixed location
                buf[..8].copy_from_slice(&self.get_setup());
                log::debug!("ep0 read: {:x?}", &buf[..8]);

                // this USB core automatically handles address set timing, so we intercept the
                // address setup packet and jam it here with the "0x200" bit set which triggers
                // the state machine to do the right thing with address setup.
                if buf[0] == 0 && buf[1] == 5 {
                    log::info!("USB address set to {} + trigger", buf[2]);
                    self.regs.set_address(0x200 | buf[2] as u32);
                    self.address.store(buf[2] as usize, Ordering::SeqCst);
                }
                Ok(8)
            } else {
                // 🚨 mysterious delay alert 🚨
                // Without this delay, enumeration is not reliable. The specific failure is that the IN response from EP0
                // to a SETUP packet is sometimes not issued. The exact nature of the timing problem is hard to
                // nail down, but the delay is necessary to be inserted after the driver calls `write()` to send the
                // response to an IN configuration transaction, and something later on which seems to stop/reset
                // the IN transaction before it can happen.
                //
                // Putting this delay after every `write()` fixes the problem, but interestingly, putting the delay
                // at this specific spot in the `read()` path also fixes the problem. The specific code path that leads
                // up to this delay being encountered in a way that fixes the problem is through the device class handler.
                // Significantly, there is nothing in this *read* that seems to fix the problem. If you omit the class handler
                // entirely (so that the delay does not happen), things still break. It just so happens that the call to
                // the class handler is narrowly scoped enough so that this path represents a bottleneck between the
                // `write()` and the offending thing that aborts the IN transaction.
                //
                // Obviously, I was not able to find the thing that is aborting the IN transaction. This is in part
                // because any logging that gets inserted in the interrupt handler will always fix the problem (as
                // it essentially inserts the delay in every path). Anyways, the notes are here, and maybe someday we'll
                // get to the bottom of it. But for now it seems to work well enough and the performance is "fine" for
                // a USB HID style interface.
                //
                // I thought this was fixed by removing spurious interrupts, but instead, it seems to come back "sometimes",
                // as opposed to "always" being a problem. So the delay is re-instated.
                //
                // Update Jan 2023: I think this delay is necessary because we have an interaction of two bugs:
                // 1. Some interrupts are going missing, because the ISR doesn't run fast enough to clear
                //    the interrupt before the next packet comes in.
                // 2. This (or something else maybe) causes a race condition between when interrupts fire for reads,
                //    and when the read call actually happens, such that the read call does not line up with the intended
                //    interrupt.
                self.tt.sleep_ms(1).ok();

                if (self.read_allowed.load(Ordering::Relaxed) & (1 << ep_addr.index() as u16)) == 0 {
                    // we do get spurious reads on EP0 transactions, so this code prevents these from proceeding
                    // this is because the EP0 interrupt will fire off a poll event that also causes a read of all the endpoints.
                    // the interrupt hasn't triggered, don't allow the read
                    let ep_status = self.status_read_volatile(ep_addr.index());
                    if ep_status.head_offset() != 0 {
                        return Err(UsbError::WouldBlock);
                    }
                } else {
                    // this load-store race condition is OK because the interrupt handler doesn't modify this
                    let masked = self.read_allowed.load(Ordering::Relaxed) & !(1 << ep_addr.index() as u16); // clear the read mask
                    self.read_allowed.store(masked, Ordering::SeqCst);
                }

                let mut ep_status = self.status_read_volatile(ep_addr.index());
                // log::info!("head_offset{}: {:x}", ep_addr.index(), head_offset * 16);
                ep_status.set_head_offset(ep_props.head_offset as u32);
                let descriptor = self.descriptor_from_status(&ep_status);
                if descriptor.in_progress() || descriptor.offset() == 0 {
                    // "early polls" happen because the main loop can be sloppy and request a read report at any time,
                    // not just when there's an interrupt.
                    // return before side-effecting any structures
                    log::warn!("WouldBlock {:?}", ep_addr);
                    return Err(UsbError::WouldBlock);
                }
                self.udc_hard_halt(ep_addr.index());

                let mut len = if ep_props.max_chain > 1 {
                    0
                } else {
                    descriptor.offset()
                };
                if ep_props.max_chain > 1 { // chained descriptors
                    for (chain_offset, sub_buf) in buf.chunks_mut(ep_props.packet_len).enumerate() {
                        let descriptor = self.descriptor_from_head_and_chain(ep_props.head_offset, chain_offset, ep_props.packet_len);
                        if descriptor.offset() != 0 {
                            log::debug!("appending {:x?}", descriptor);
                            let available_len = sub_buf.len().min(descriptor.offset());
                            for (desc_offset, word_chunk) in sub_buf[..available_len].chunks_mut(size_of::<u32>()).enumerate() {
                                word_chunk.copy_from_slice(
                                    &descriptor.read_data(desc_offset)
                                    .to_le_bytes()[..word_chunk.len()]) // the upper limit index ensures that if sub_buf is an odd number of bytes, we don't panic
                            }
                            if sub_buf.len() < descriptor.offset() {
                                log::warn!("insufficient space in buffer to store incoming data (want: {} have: {}). truncating!", descriptor.offset(), sub_buf.len());
                            }
                            len += available_len;
                        } else {
                            log::debug!("breaking at {:x?}", descriptor);
                            break;
                        }
                    }
                    if buf.len() < len {
                        log::error!("read ep{} overflows: {} < {}", ep_addr.index(), buf.len(), len);
                        len = buf.len();
                    }
                    for (chain_offset, sub_buf) in buf.chunks_mut(ep_props.packet_len).enumerate() {
                        let descriptor = self.descriptor_from_head_and_chain(ep_props.head_offset, chain_offset, ep_props.packet_len);
                        descriptor.set_desc_flags(UsbDirection::Out,
                            true, true, false);
                        for (index, _slice) in sub_buf.chunks(size_of::<u32>()).enumerate() {
                            descriptor.write_data(index, 0); // zeroize the descriptor area so we don't leak data between packets
                        }
                        if chain_offset == (ep_props.max_chain - 1) || sub_buf.len() != ep_props.packet_len {
                            // mark the last item in linked list with a null pointer
                            descriptor.set_next_desc_and_len(0, sub_buf.len());
                        } else {
                            // if not the last link, setup the next linked list pointer
                            let next_addr = self.next_from_head_and_chain(ep_props.head_offset, chain_offset, ep_props.packet_len);
                            descriptor.set_next_desc_and_len(next_addr, sub_buf.len());
                        }
                        descriptor.set_offset_only(0); // reset the write pointer to 0, also sets in_progress
                        log::debug!("bulk descriptor {} setup: {:x?}", chain_offset, descriptor);
                    }
                    ep_status.set_max_packet_size(ep_props.packet_len as _);
                    // this is reset to 0 after every transaction by the hardware, so we must reset it
                    ep_status.set_head_offset(ep_props.head_offset as u32);
                } else { // single descriptor
                    if buf.len() < len {
                        log::error!("read ep{} overflows: {} < {}", ep_addr.index(), buf.len(), len);
                        len = buf.len();
                    }
                    for (index, dst) in buf[..len].chunks_exact_mut(4).enumerate() {
                        let word = descriptor.read_data(index).to_le_bytes();
                        dst.copy_from_slice(&word);
                    }
                    if len % 4 != 0 {
                        // this will "overread" the descriptor area, but it's OK because descriptors must be aligned to 16-byte boundaries
                        // so even if the length is odd, the space allocated will always include dummy padding which will keep us
                        // from reading into neighboring data.
                        let word = descriptor.read_data(len / 4).to_le_bytes();
                        // write only into the portion of the buffer that's allocated, don't write the extra 0's
                        for i in 0..len % 4 {
                            buf[(len / 4) + i] = word[i]
                        }
                    }
                    // setup for the next transaction
                    ep_status.set_max_packet_size(ep_props.packet_len as _);
                    descriptor.set_next_desc_and_len(0, buf.len());
                    descriptor.set_desc_flags(UsbDirection::Out,
                        true, true, false);
                    descriptor.set_offset_only(0); // reset the read pointer to 0, also sets in_progress
                }

                // sanity check on interrupts: it should be cleared by this point!
                let interrupts = self.regs.interrupts();
                if interrupts.0 != 0 {
                    if interrupts.0 != 1 { // ep0 interrupt can sometimes be leftover from ep0setup handling
                        log::debug!("Pending interrupts, clearing: {:?}", interrupts);
                    }
                    self.regs.clear_all_interrupts();
                }

                // this auto-toggles ep_status.set_data_phase(!ep_status.data_phase()); // toggle the data phase
                self.status_write_volatile(ep_addr.index(), ep_status);
                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

                // just reading the registers after setting them seems to upset the USB device block
                //log::debug!("read buf: {:x?}", &buf[..len]);
                //log::info!("ep{} read: {:x?} (len {} into buf of {})", ep_addr.index(), &buf[..len], len, buf.len());
                let epcheck = self.status_read_volatile(ep_addr.index());
                let descheck = self.descriptor_from_status(&epcheck);
                log::debug!("RD status{} [{:x}]: {:?}", ep_addr.index(), epcheck.0, epcheck);
                log::debug!("RD desc{} [{:x},{:x},{:x}]: {:?}", ep_addr.index(), descheck.read(0), descheck.read(1), descheck.read(2), descheck);
                #[cfg(feature="mjolnir")]
                if ep_addr.index() == 2 { // mjolnir should be use selectively on EPs that require debugging, lest the weilder be overwhelmed by its spew.
                    self.ll_debug();
                }
                self.udc_hard_unhalt(ep_addr.index());
                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
                Ok(len)
            }
        } else {
            Err(UsbError::InvalidEndpoint)
        }
    }
    /// Reconfigures EP0 to be an OUT descriptor. Used to reconfigure EP0 for the STATUS ack.
    fn set_ep0_out(&self) {
        self.ep0_out_reset();
        let mut ep0_status = self.status_read_volatile(0);
        ep0_status.set_head_offset(self.ep0_out_offset() as u32);
        self.status_write_volatile(0, ep0_status);
    }
    /// Sets or clears the STALL condition for an endpoint. If the endpoint is an OUT endpoint, it
    /// should be prepared to receive data again.
    fn set_stalled(&self, ep_addr: EndpointAddress, stalled: bool) {
        //if ep_addr.index() == 0 && ep_addr.direction() == UsbDirection::Out && stalled == false {
        //    return;
        //}
        let statcheck = self.status_read_volatile(ep_addr.index());
        if statcheck.force_stall() != stalled {
            if ep_addr.index() != 0 {
                log::info!("set_stalled ep{}->{} dir {:?}", ep_addr.index(), stalled, ep_addr.direction());
            }
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
        log::info!("{}USB.SUSPEND,{}", xous::BOOKEND_START, xous::BOOKEND_END);
        log::warn!("USB suspend called; this implementation does nothing.");
    }

    /// Resumes from suspend mode. This may only be called after the peripheral has been previously
    /// suspended.
    fn resume(&self) {
        log::info!("{}USB.RESUME,{}", xous::BOOKEND_START, xous::BOOKEND_END);
        log::info!("USB resume called.");
    }

    /// Gets information about events and incoming data. Usually called in a loop or from an
    /// interrupt handler. See the [`PollResult`] struct for more information.
    fn poll(&self) -> PollResult {
        let interrupts = self.regs.interrupts();
        let mut ints_to_clear = UdcInterrupts(0);
        log::debug!(">>>> frame {}: {:x?}", self.regs.frame_id(), interrupts);
        let poll_result =
        if interrupts.reset() {
            ints_to_clear.set_reset(true);
            log::trace!("aft reset: {:x?}", interrupts.0);
            PollResult::Reset
        } else if interrupts.ep0_setup() {
            ints_to_clear.set_ep0_setup(true);
            ints_to_clear.set_endpoint(1); // this is a bitmask, so this corresponds to ep0
            PollResult::Data { ep_out: 0, ep_in_complete: 0, ep_setup: 1 }
        } else if interrupts.endpoint() != 0 {
            let mut ep_in_complete = 0;
            let mut ep_out = 0;
            // all of them will be handled here, so, clear the interrupts as needed
            if interrupts.endpoint() != 0 {
                let mut bit = 0;
                while bit < 16 {
                    if (interrupts.endpoint() & (1 << bit)) != 0 {
                        // form a descriptor from the memory range assigned to the EP
                        let mut ep_status = self.status_read_volatile(bit);
                        if bit == 0 {
                            // EP0 SETUP overrides the descriptor offset, restore it to obtain a descriptor
                            // (but don't write it back, since we're not ready to send anything --
                            // it will get written back on the next `write`)
                            ep_status.set_head_offset(self.view.ep0in_head);
                        } else if let Some(ep_props) = self.view.ep_allocs[bit] {
                            if ep_status.head_offset() != 0 {
                                log::warn!("got INT on ep{} but head is not 0", bit);
                            }
                            ep_status.set_head_offset(ep_props.head_offset as u32);
                        }
                        let descriptor = self.descriptor_from_status(&ep_status);
                        if descriptor.direction() == UsbDirection::Out {
                            // this race condition is OK because this is not modified in the interrupt handler
                            let masked = self.read_allowed.load(Ordering::SeqCst);
                            self.read_allowed.store(masked | (1 << bit as u16), Ordering::SeqCst);
                            ep_out |= 1 << bit;
                        } else {
                            ep_in_complete |= 1 << bit;
                        }
                        ints_to_clear.set_endpoint(1 << bit);

                        if false { // full low-level readback
                            if bit != 0 {
                                log::debug!("status{}: {:?}", bit, self.status_read_volatile(bit));
                                log::debug!("desc{}: {:?}", bit, self.descriptor_from_status(&self.status_read_volatile(bit)));
                            }
                        } else { // as processed
                            if bit != 0 {
                                log::debug!("PL status{}: {:?}", bit, ep_status);
                                log::debug!("PL desc{}: {:?}", bit, descriptor);
                            }
                        }
                    }
                    bit += 1;
                }
            }
            PollResult::Data { ep_out, ep_in_complete, ep_setup: 0 }
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
            log::debug!("more interrupts to handle: {:x?}", self.regs.interrupts());
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

