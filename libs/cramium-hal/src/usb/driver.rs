use core::convert::TryFrom;
use core::mem::size_of;
#[cfg(feature = "std")]
use core::sync::atomic::AtomicBool;
use core::sync::atomic::{compiler_fence, AtomicPtr, Ordering};
#[cfg(feature = "std")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "std")]
use usb_device::bus::PollResult;
#[cfg(feature = "std")]
use usb_device::{class_prelude::*, Result, UsbDirection};
#[cfg(feature = "std")]
use utralib::generated::*;

#[cfg(not(feature = "std"))]
use super::compat::AtomicCsr;
use crate::usb::utra::*;
use crate::{print, println};

const CRG_EVENT_RING_NUM: usize = 1;
const CRG_ERST_SIZE: usize = 1;
const CRG_EVENT_RING_SIZE: usize = 128;
const CRG_EP0_TD_RING_SIZE: usize = 16;
const CRG_EP_NUM: usize = 4;
const CRG_TD_RING_SIZE: usize = 1280;
const CRG_UDC_MAX_BURST: usize = 15;
const CRG_UDC_ISO_INTERVAL: usize = 3;

const CRG_INT_TARGET: u32 = 0;

/// allocate 0x100 bytes for event ring segment table, each table 0x40 bytes
const CRG_UDC_ERSTSIZE: usize = 0x100;
/// allocate 0x800 for one event ring, include 128 event TRBs , each TRB 16 bytes
const CRG_UDC_EVENTRINGSIZE: usize = 0x800 * CRG_EVENT_RING_NUM;
/// allocate 0x200 for ep context, include 30 ep context, each ep context 16 bytes
const CRG_UDC_EPCXSIZE: usize = 0x200;
/// allocate 0x400 for EP0 transfer ring, include 64 transfer TRBs, each TRB 16 bytes
const CRG_UDC_EP0_TRSIZE: usize = 0x400;
/// 1280(TRB Num) * 4(EP NUM) * 16(TRB bytes)
const CRG_UDC_EP_TRSIZE: usize = CRG_TD_RING_SIZE * CRG_EP_NUM * 16;
/// allocate 0x400 bytes for EP0 Buffer, Normally EP0 TRB transfer length will not greater than 1K
const CRG_UDC_EP0_REQBUFSIZE: usize = 0x400;

pub const CRG_IFRAM_PAGES: usize = 16;
pub const CRG_UDC_MEMBASE: usize =
    utralib::HW_IFRAM1_MEM + utralib::HW_IFRAM1_MEM_LEN - CRG_IFRAM_PAGES * 0x1000;

const CRG_UDC_ERST_OFFSET: usize = 0; // use relative offsets
const CRG_UDC_EVENTRING_OFFSET: usize = CRG_UDC_ERST_OFFSET + CRG_UDC_ERSTSIZE;
const CRG_UDC_EPCX_OFFSET: usize = CRG_UDC_EVENTRING_OFFSET + CRG_UDC_EVENTRINGSIZE;

const CRG_UDC_EP0_TR_OFFSET: usize = CRG_UDC_EPCX_OFFSET + CRG_UDC_EPCXSIZE;
const CRG_UDC_EP_TR_OFFSET: usize = CRG_UDC_EP0_TR_OFFSET + CRG_UDC_EP0_TRSIZE;
const CRG_UDC_EP0_BUF_OFFSET: usize = CRG_UDC_EP_TR_OFFSET + CRG_UDC_EP_TRSIZE;
const CRG_UDC_APP_BUFOFFSET: usize = CRG_UDC_EP0_BUF_OFFSET + CRG_UDC_EP0_REQBUFSIZE;

#[cfg(feature = "std")]
static INTERRUPT_INIT_DONE: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "std")]
fn handle_usb(_irq_no: usize, arg: *mut usize) {
    let usb = unsafe { &mut *(arg as *mut CorigineUsb) };
    let pending = usb.csr.r(utralib::utra::irqarray1::EV_PENDING);

    // actual interrupt handling is done in userspace, this just triggers the routine

    usb.csr.wo(utralib::utra::irqarray1::EV_PENDING, pending);

    xous::try_send_message(usb.conn, xous::Message::new_scalar(usb.opcode, 0, 0, 0, 0)).ok();
}

// total size 0x15300
#[derive(Debug)]
pub enum Error {
    CoreBusy,
    CmdFailure,
    InvalidState,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum CorigineEvent {
    None = 0,
    Error,
    Interrupt,
}

pub enum EpState {
    Disabled = 0,
    Running,
    Halted,
    Stopped,
}

pub enum CmdType {
    InitEp0 = 0,
    UpdateEp0 = 1,
    SetAddr = 2,
    SendDevNotification = 3,
    ConfigEp = 4,
    SetHalt = 5,
    ClearHalt = 6,
    ResetSeqNum = 7,
    StopEp = 8,
    SetTrDqPtr = 9,
    ForceFlowControl = 10,
    ReqLdmExchange = 11,
}

#[repr(u32)]
pub enum PortSpeed {
    Invalid = 0,
    Fs = 1,
    Ls = 2,
    Hs = 3,
    Ss = 4,
    SspGen2x1 = 5,
    SspGen1x2 = 6,
    SspGen2x2 = 7,
}
impl PortSpeed {
    pub fn from_portsc(portsc: u32) -> Self {
        match (portsc >> 10) & 0xff {
            1 => PortSpeed::Fs,
            2 => PortSpeed::Ls,
            3 => PortSpeed::Hs,
            4 => PortSpeed::Ss,
            5 => PortSpeed::SspGen2x1,
            6 => PortSpeed::SspGen1x2,
            7 => PortSpeed::SspGen2x2,
            _ => PortSpeed::Invalid,
        }
    }
}

#[repr(u32)]
#[derive(Debug)]
pub enum PortLinkState {
    U0 = 0,
    U1 = 1,
    U2 = 2,
    U3 = 3,
    Disabled = 4,
    RxDetect = 5,
    Inactive = 6,
    Polling = 7,
    Recovery = 8,
    HotReset = 9,
    Compliance = 10,
    TestMode = 11,
    Resume = 15,
    Reserved,
}
impl PortLinkState {
    pub fn from_portsc(portsc: u32) -> Self {
        match (portsc >> 5) & 0xF {
            0 => PortLinkState::U0,
            1 => PortLinkState::U1,
            2 => PortLinkState::U2,
            3 => PortLinkState::U3,
            4 => PortLinkState::Disabled,
            5 => PortLinkState::RxDetect,
            6 => PortLinkState::Inactive,
            7 => PortLinkState::Polling,
            8 => PortLinkState::Recovery,
            9 => PortLinkState::HotReset,
            10 => PortLinkState::Compliance,
            11 => PortLinkState::TestMode,
            15 => PortLinkState::Resume,
            _ => PortLinkState::Reserved,
        }
    }
}

#[repr(C)]
pub struct Uicr {
    iman: u32,
    imod: u32,
    erstsz: u32,
    resv0: u32,
    erstbalo: u32,
    erstbahi: u32,
    erdplo: u32,
    erdphi: u32,
}

const USB_SEND: u8 = 0;
const USB_RECV: u8 = 1;

const USB_CONTROL_ENDPOINT: u8 = 0;
const USB_ISOCHRONOUS_ENDPOINT: u8 = 1;
const USB_BULK_ENDPOINT: u8 = 2;
const USB_INTERRUPT_ENDPOINT: u8 = 3;

const CRG_UDC_CFG0_MAXSPEED_FS: u32 = 1;

const CRG_UDC_ERDPLO_EHB: u32 = 1 << 3;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrbType {
    Rsvd = 0,
    XferNormal = 1,
    Rsvd2 = 2,
    DataStage = 3,
    StatusStage = 4,
    DataIsoch = 5,
    Link = 6,
    EventTransfer = 32,
    EventCmdCompletion = 33,
    EventPortStatusChange = 34,
    MfindexWrap = 39,
    SetupPkt = 40,
}
impl TryFrom<u32> for TrbType {
    type Error = Error;

    fn try_from(value: u32) -> core::result::Result<Self, Error> {
        match value {
            1 => Ok(TrbType::XferNormal),
            2 => Ok(TrbType::Rsvd2),
            3 => Ok(TrbType::DataStage),
            4 => Ok(TrbType::StatusStage),
            5 => Ok(TrbType::DataIsoch),
            6 => Ok(TrbType::Link),
            32 => Ok(TrbType::EventTransfer),
            33 => Ok(TrbType::EventCmdCompletion),
            34 => Ok(TrbType::EventPortStatusChange),
            39 => Ok(TrbType::MfindexWrap),
            40 => Ok(TrbType::SetupPkt),
            0 => Ok(TrbType::Rsvd),
            _ => Err(Error::InvalidState),
        }
    }
}

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum CompletionCode {
    Invalid = 0,
    Success = 1,
    UsbTransactionError = 4,
    ShortPacket = 13,
    EventRingFullError = 21,
    MissedServiceError = 23,
    Stopped = 26,
    StoppedLengthInvalid = 27,
    ProtocolStallError = 192,
    SetupTagMismatchError = 193,
    Halted = 194,
    HaltedLengthInvalid = 195,
    DisabledError = 196,
}
impl TryFrom<u32> for CompletionCode {
    type Error = Error;

    /// Note that this expects a raw dw2 value - the bit shift is built into this.
    fn try_from(dw2: u32) -> core::result::Result<Self, Error> {
        match (dw2 >> 24) & 0xFF {
            1 => Ok(CompletionCode::Success),
            4 => Ok(CompletionCode::UsbTransactionError),
            13 => Ok(CompletionCode::ShortPacket),
            21 => Ok(CompletionCode::EventRingFullError),
            23 => Ok(CompletionCode::MissedServiceError),
            26 => Ok(CompletionCode::Stopped),
            27 => Ok(CompletionCode::StoppedLengthInvalid),
            192 => Ok(CompletionCode::ProtocolStallError),
            193 => Ok(CompletionCode::SetupTagMismatchError),
            194 => Ok(CompletionCode::Halted),
            195 => Ok(CompletionCode::HaltedLengthInvalid),
            196 => Ok(CompletionCode::DisabledError),
            _ => Err(Error::InvalidState),
        }
    }
}

#[repr(C)]
#[derive(Default)]

pub struct TransferTrbS {
    dplo: u32,
    dphi: u32,
    dw2: u32,
    dw3: u32,
}
// see 8.6.3 if debug visibility is necessary
impl TransferTrbS {
    pub fn zeroize(&mut self) {
        self.dplo = 0;
        self.dphi = 0;
        self.dw2 = 0;
        self.dw3 = 0;
    }

    pub fn set_intr_target(&mut self, target: u32) { self.dw2 = (target << 22) & 0xFFC00000; }

    pub fn set_trb_type(&mut self, trb_type: TrbType) {
        self.dw3 = (self.dw3 & !0x0000FC00) | ((trb_type as u32) << 10);
    }

    pub fn get_trb_type(&self) -> TrbType {
        let trb_type = (self.dw3 >> 10) & 0x3F;
        TrbType::try_from(trb_type).expect("Unknown TRB type")
    }

    pub fn set_cycle_bit(&mut self, pcs: u8) { self.dw3 = (self.dw3 & !0x1) | (pcs as u32); }

    /// Implicitly sets the link type to Link
    pub fn set_trb_link(&mut self, toggle: bool, next_trb: *mut TransferTrbS) {
        self.dplo = next_trb as usize as u32;
        self.dphi = 0;
        self.dw2 = 0;
        self.set_trb_type(TrbType::Link);
        self.dw3 = (self.dw3 & !0x2) | if toggle { 2 } else { 0 };
        // also set interrupt enable
        self.dw3 = self.dw3 | 0x20; // set INTR_ON_COMPLETION
        compiler_fence(Ordering::SeqCst);
    }

    /// Implicitly sets link type to Status
    pub fn set_trb_status(
        &mut self,
        pcs: u8,
        set_addr: bool,
        stall: bool,
        tag: u8,
        intr_target: u32,
        dir: bool,
    ) {
        self.set_trb_type(TrbType::StatusStage);
        self.set_intr_target(intr_target);
        self.dw3 = (self.dw3 & !0x1) | (pcs & 1) as u32; // CYCLE_BIT
        self.dw3 = self.dw3 | 0x20; // set INTR_ON_COMPLETION
        self.dw3 = (self.dw3 & !0x1_0000) | if dir { 1 << 16 } else { 0 }; // DIR_MASK
        self.dw3 = (self.dw3 & !0x00060000) | ((tag as u32 & 0x3) << 17); // SETUP_TAG
        self.dw3 = (self.dw3 & !0x00080000) | if stall { 1 << 19 } else { 0 }; // STATUS_STAGE_TRB_STALL
        self.dw3 = (self.dw3 & !0x00100000) | if set_addr { 1 << 20 } else { 0 }; // STATUS_STAGE_TRB_SET_ADDR
        compiler_fence(Ordering::SeqCst);
        #[cfg(feature = "std")]
        log::info!("trb_status dw2 {:x} dw3 {:x}", self.dw2, self.dw3);
    }
}

#[repr(C)]
#[derive(Default)]

pub struct EpCxS {
    dw0: u32,
    dw1: u32,
    dw2: u32,
    dw3: u32,
}
impl EpCxS {
    // TODO: implement field accessors for dw*
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct EventTrbS {
    dw0: u32,
    dw1: u32,
    dw2: u32,
    dw3: u32,
}
impl EventTrbS {
    pub fn zeroize(&mut self) {
        self.dw0 = 0;
        self.dw1 = 0;
        self.dw2 = 0;
        self.dw3 = 0;
    }

    pub fn get_cycle_bit(&self) -> u8 { (self.dw3 & 0x1) as u8 }

    pub fn get_endpoint_id(&self) -> u8 { ((self.dw3 & 0x001F0000) >> 16) as u8 }

    pub fn get_trb_type(&self) -> TrbType {
        let trb_type = (self.dw3 >> 10) & 0x3F;
        TrbType::try_from(trb_type).expect("Unknown TRB type")
    }

    pub fn get_raw_setup(&self) -> [u8; 8] {
        let mut ret = [0u8; 8];
        ret[..4].copy_from_slice(&self.dw0.to_le_bytes());
        ret[4..].copy_from_slice(&self.dw1.to_le_bytes());
        ret
    }

    pub fn get_setup_tag(&self) -> u8 { ((self.dw3 >> 21) & 0x3) as u8 }
}
#[repr(C)]
#[derive(Default)]
pub struct ErstS {
    /* 64-bit event ring segment address */
    seg_addr_lo: u32,
    seg_addr_hi: u32,
    seg_size: u32,
    /* Set to zero */
    rsvd: u32,
}

/// buffer info data structure
pub struct BufferInfo {
    vaddr: AtomicPtr<u8>,
    dma: u64,
    len: u32,
}
impl Default for BufferInfo {
    fn default() -> Self { Self { vaddr: AtomicPtr::new(core::ptr::null_mut()), dma: 0, len: 0 } }
}

pub struct UdcEp {
    // Endpoint number
    ep_num: u8,
    // Endpoint direction
    direction: u8,
    ep_type: u8,
    max_packet_size: u16,
    tran_ring_info: BufferInfo,
    first_trb: AtomicPtr<TransferTrbS>,
    last_trb: AtomicPtr<TransferTrbS>,
    enq_pt: AtomicPtr<TransferTrbS>,
    deq_pt: AtomicPtr<TransferTrbS>,
    pcs: u8,
    tran_ring_full: bool,
    ep_state: EpState,
    wedge: bool,
}
impl Default for UdcEp {
    fn default() -> Self {
        Self {
            ep_num: 0,
            direction: 0,
            ep_type: 0,
            max_packet_size: 0,
            tran_ring_info: BufferInfo::default(),
            first_trb: AtomicPtr::new(core::ptr::null_mut()),
            last_trb: AtomicPtr::new(core::ptr::null_mut()),
            enq_pt: AtomicPtr::new(core::ptr::null_mut()),
            deq_pt: AtomicPtr::new(core::ptr::null_mut()),
            pcs: 0,
            tran_ring_full: false,
            ep_state: EpState::Disabled,
            wedge: false,
        }
    }
}

// Corigine USB device controller event data structure
pub struct UdcEvent {
    erst: BufferInfo,
    p_erst: AtomicPtr<ErstS>,
    event_ring: BufferInfo,
    evt_dq_pt: AtomicPtr<EventTrbS>,
    ccs: u8,
    evt_seg0_last_trb: AtomicPtr<EventTrbS>,
}
impl Default for UdcEvent {
    fn default() -> Self {
        Self {
            erst: BufferInfo::default(),
            p_erst: AtomicPtr::new(core::ptr::null_mut()),
            event_ring: BufferInfo::default(),
            evt_dq_pt: AtomicPtr::new(core::ptr::null_mut()),
            ccs: 0,
            evt_seg0_last_trb: AtomicPtr::new(core::ptr::null_mut()),
        }
    }
}

// Corigine USB device controller power management data structure
#[derive(Default)]
pub struct SelValue {
    u2_pel_value: u16,
    u2_sel_valu: u16,
    u1_pel_value: u8,
    u1_sel_value: u8,
}

const WAIT_FOR_SETUP: u8 = 0;
const SETUP_PKT_PROCESS_IN_PROGRESS: u8 = 1;
const DATA_STAGE_XFER: u8 = 2;
const DATA_STAGE_RECV: u8 = 3;
const STATUS_STAGE_XFER: u8 = 4;
const STATUS_STAGE_RECV: u8 = 5;

/* device speed */
pub enum UsbDeviceSpeed {
    Unknown = 0,
    Low,
    Full,
    High,
    Wireless,
    Super,
    SuperPlus,
}

/* device state */
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum UsbDeviceState {
    NotAttached = 0,
    Attached,
    Powered,
    Reconnecting,
    Unauthenticated,
    Default,
    Address,
    Configured,
    Suspended,
}

pub struct CorigineUsb {
    ifram_base_ptr: usize,
    pub csr: AtomicCsr<u32>,
    // Because the init routine requires magic pokes
    magic_page: &'static mut [u32],
    #[cfg(feature = "std")]
    conn: xous::CID,
    #[cfg(feature = "std")]
    opcode: usize,

    udc_ep: [UdcEp; CRG_EP_NUM + 2],
    ep_cx: BufferInfo,
    p_epcx: AtomicPtr<EpCxS>,

    udc_event: [UdcEvent; CRG_EVENT_RING_NUM],

    // power management data
    sel_value: SelValue,

    setup_status: u8,

    setup: [u8; 8],
    setup_tag: u8,

    speed: UsbDeviceSpeed,
    device_state: UsbDeviceState,
    resume_state: u8,
    dev_addr: u16,
    set_tm: u8,
    cur_interface_num: u8,
    connected: u32,

    // actual hardware pointer value to pass to UDC; not directly accessed by Rust
    ep0_buf: AtomicPtr<u8>,

    u2_rwe: u32,
    feature_u1_enabled: u32,
    feature_u2_enabled: u32,

    setup_tag_mismatch_found: u32,
    portsc_on_reconnecting: u32,
    max_speed: u32,
}
impl CorigineUsb {
    /// Safety: this function is generally pretty unsafe because the underlying hardware needs raw pointers,
    /// and will mutate values underneath the OS with no regard for safety.
    ///
    /// However, from the standpoint of Rust, the particular guarantee we need is that the `ifram_base_ptr`
    /// maps to the exact same value in virtual memory as in physical memory. This allows us to operate on
    /// the pointer directly in virtual memory, and pass it into the hardware (which thinks in physical
    /// memory), without having to do tedious translations back and forth. On a platform where these two
    /// could not be guaranteed to the same range, the driver would need to be written with a
    /// de-virtualization/re-virtualization layer every time we extract data records from the device's mapped
    /// RAM.
    pub unsafe fn new(_conn: u32, _opcode: usize, ifram_base_ptr: usize, csr: AtomicCsr<u32>) -> Self {
        let magic_page = unsafe { core::slice::from_raw_parts_mut(csr.base() as *mut u32, 1024) };

        Self {
            ifram_base_ptr,
            csr,
            #[cfg(feature = "std")]
            conn: _conn,
            #[cfg(feature = "std")]
            opcode: _opcode,
            magic_page,
            // is there a way to make this less shitty?
            udc_ep: [
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
            ],
            ep_cx: BufferInfo::default(),
            p_epcx: AtomicPtr::new(core::ptr::null_mut()),
            udc_event: [UdcEvent::default(); CRG_EVENT_RING_NUM],
            sel_value: SelValue::default(),
            setup_status: 0,
            setup: [0u8; 8],
            setup_tag: 0,
            speed: UsbDeviceSpeed::Unknown,
            device_state: UsbDeviceState::NotAttached,
            resume_state: 0,
            dev_addr: 0,
            set_tm: 0,
            cur_interface_num: 0,
            connected: 0,
            ep0_buf: AtomicPtr::new(core::ptr::null_mut()),
            u2_rwe: 0,
            feature_u1_enabled: 0,
            feature_u2_enabled: 0,
            setup_tag_mismatch_found: 0,
            portsc_on_reconnecting: 0,
            max_speed: 0,
        }
    }

    /// This is coded in a strange way because the reference drivers are designed to handle
    /// potentially multiple Uicr; however, this specific instance of hardware has only one Uicr.
    fn uicr(&self) -> [&'static mut Uicr; 1] {
        // Safety: only safe because this is an aligned, allocated region
        // of hardware registers; all values are representable as u32; and the structure
        // fits the data.
        [unsafe { ((self.csr.base() as usize + CORIGINE_UICR_OFFSET) as *mut Uicr).as_mut().unwrap() }]
    }

    pub fn reset(&mut self) {
        println!("devcap: {:x}", self.csr.r(DEVCAP));
        println!("max speed: {:x}", self.csr.rf(DEVCONFIG_MAX_SPEED));
        println!("usb3 disable: {:x}", self.csr.rf(DEVCONFIG_USB3_DISABLE_COUNT));

        // NOTE: the indices are byte-addressed, and so need to be divided by size_of::<u32>()
        const MAGIC_TABLE: [(usize, u32); 17] = [
            (0x084, 0x01401388),
            (0x0f4, 0x0000f023),
            (0x088, 0x3b066409),
            (0x08c, 0x0d020407),
            (0x090, 0x04055050),
            (0x094, 0x03030a07),
            (0x098, 0x05131304),
            (0x09c, 0x3b4b0d15),
            (0x0a0, 0x14168c6e),
            (0x0a4, 0x18060408),
            (0x0a8, 0x4b120c0f),
            (0x0ac, 0x03190d05),
            (0x0b0, 0x08080d09),
            (0x0b4, 0x20060b03),
            (0x0b8, 0x040a8c0e),
            (0x0bc, 0x44087d5a),
            (0x110, 0x00000000),
        ];

        for (offset, magic) in MAGIC_TABLE {
            self.magic_page[offset / size_of::<u32>()] = magic;
        }

        // udc reset
        self.csr.rmwf(USBCMD_INT_ENABLE, 0);
        self.csr.rmwf(USBCMD_SOFT_RESET, 1);

        while self.csr.rf(USBCMD_SOFT_RESET) != 0 {
            // wait for reset to finish
        }
        println!("\rUSB reset done");
    }

    pub fn init(&mut self) {
        let uicr = self.uicr();

        // NOTE: the indices are byte-addressed, and so need to be divided by size_of::<u32>()
        const MAGIC_TABLE: [(usize, u32); 17] = [
            (0x084, 0x01401388),
            (0x0f4, 0x0000f023),
            (0x088, 0x3b066409),
            (0x08c, 0x0d020407),
            (0x090, 0x04055050),
            (0x094, 0x03030a07),
            (0x098, 0x05131304),
            (0x09c, 0x3b4b0d15),
            (0x0a0, 0x14168c6e),
            (0x0a4, 0x18060408),
            (0x0a8, 0x4b120c0f),
            (0x0ac, 0x03190d05),
            (0x0b0, 0x08080d09),
            (0x0b4, 0x20060b03),
            (0x0b8, 0x040a8c0e),
            (0x0bc, 0x44087d5a),
            (0x110, 0x00000000),
        ];

        for (offset, magic) in MAGIC_TABLE {
            self.magic_page[offset / size_of::<u32>()] = magic;
        }

        // stop controller and disable interrupt
        self.csr.rmwf(USBCMD_INT_ENABLE, 0);
        self.csr.rmwf(USBCMD_RUN_STOP, 0);

        // udc reset
        self.csr.rmwf(USBCMD_SOFT_RESET, 1);
        compiler_fence(Ordering::SeqCst);

        while self.csr.rf(USBCMD_SOFT_RESET) != 0 {
            // wait for reset to finish
        }

        compiler_fence(Ordering::SeqCst);
        self.csr.wo(DEVCONFIG, 0x80 | CRG_UDC_CFG0_MAXSPEED_FS);
        compiler_fence(Ordering::SeqCst);
        #[cfg(not(feature = "std"))]
        println!("config0: {:x}", self.csr.r(DEVCONFIG));
        #[cfg(feature = "std")]
        log::info!("config0: {:x}", self.csr.r(DEVCONFIG));

        self.csr.wo(
            EVENTCONFIG,
            self.csr.ms(EVENTCONFIG_CSC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PEC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PPC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PRC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PLC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_CEC_ENABLE, 1),
        );
        compiler_fence(Ordering::SeqCst);

        // event_ring_init
        // init event ring 0
        for (index, udc_event) in self.udc_event.iter_mut().enumerate() {
            // event_ring_init, but inline

            // allocate event ring segment table
            let erst: &mut [ErstS] = unsafe {
                core::slice::from_raw_parts_mut(
                    (self.ifram_base_ptr + CRG_UDC_ERST_OFFSET + 0x40 * index) as *mut ErstS,
                    CRG_ERST_SIZE,
                )
            };
            for e in erst.iter_mut() {
                *e = ErstS::default();
            }
            udc_event.erst.len = (erst.len() * size_of::<ErstS>()) as u32;
            udc_event.erst.vaddr = AtomicPtr::new(erst.as_mut_ptr() as *mut u8); // ErstS ??
            udc_event.p_erst = AtomicPtr::new(udc_event.erst.vaddr.load(Ordering::SeqCst) as *mut ErstS);

            // allocate event ring
            let event_ring = unsafe {
                core::slice::from_raw_parts_mut(
                    (self.ifram_base_ptr
                        + CRG_UDC_EVENTRING_OFFSET
                        + CRG_EVENT_RING_SIZE * size_of::<EventTrbS>() * index)
                        as *mut u8,
                    CRG_EVENT_RING_SIZE * size_of::<EventTrbS>(),
                )
            };
            event_ring.fill(0);

            udc_event.event_ring.len = event_ring.len() as u32;
            udc_event.event_ring.vaddr = AtomicPtr::new(event_ring.as_mut_ptr()); // EventTrbS ??
            udc_event.evt_dq_pt =
                AtomicPtr::new(udc_event.event_ring.vaddr.load(Ordering::SeqCst) as *mut EventTrbS);
            udc_event.evt_seg0_last_trb = AtomicPtr::new(unsafe {
                (udc_event.event_ring.vaddr.load(Ordering::SeqCst) as *mut EventTrbS)
                    .add(CRG_EVENT_RING_SIZE - 1)
            });

            udc_event.ccs = 1;

            // copy control structure pointers to hardware-managed memory
            let p_erst =
                unsafe { udc_event.p_erst.load(Ordering::SeqCst).as_mut().expect("invalid pointer") };
            p_erst.seg_addr_lo = udc_event.event_ring.vaddr.load(Ordering::SeqCst) as u32;
            p_erst.seg_addr_hi = 0;
            p_erst.seg_size = CRG_EVENT_RING_SIZE as u32;
            p_erst.rsvd = 0;

            uicr[index].erstsz = CRG_ERST_SIZE as u32;
            uicr[index].erstbalo = udc_event.erst.vaddr.load(Ordering::SeqCst) as u32;
            uicr[index].erstbahi = 0;
            uicr[index].erdplo =
                udc_event.event_ring.vaddr.load(Ordering::SeqCst) as u32 | self.csr.ms(ERDPLO_EHB, 1);
            uicr[index].erdphi = 0;

            uicr[index].iman = self.csr.ms(IMAN_IE, 1) | self.csr.ms(IMAN_IP, 1);
            uicr[index].imod = 0;
            compiler_fence(Ordering::SeqCst);
        }

        // init_device_context
        #[cfg(feature = "std")]
        log::info!("Begin init_device_context");
        #[cfg(not(feature = "std"))]
        println!("Begin init_device_context");
        // init device context and ep context, refer to 7.6.2
        self.ep_cx.len = (CRG_EP_NUM * size_of::<EpCxS>()) as u32;
        self.ep_cx.vaddr = AtomicPtr::new((self.ifram_base_ptr + CRG_UDC_EPCX_OFFSET) as *mut u8); // EpCxS ??
        self.p_epcx = AtomicPtr::new(self.ep_cx.vaddr.load(Ordering::SeqCst) as *mut EpCxS);

        self.csr.wo(DCBAPLO, self.ep_cx.vaddr.load(Ordering::SeqCst) as u32);
        self.csr.wo(DCBAPHI, 0);
        compiler_fence(Ordering::SeqCst);
        #[cfg(not(feature = "std"))]
        {
            println!(" dcbaplo: {:x}", self.csr.r(DCBAPLO));
            println!(" dcbaphi: {:x}", self.csr.r(DCBAPHI));
        }
        #[cfg(feature = "std")]
        {
            log::info!(" dcbaplo: {:x}", self.csr.r(DCBAPLO));
            log::info!(" dcbaphi: {:x}", self.csr.r(DCBAPHI));
        }

        #[cfg(feature = "std")]
        if !INTERRUPT_INIT_DONE.fetch_or(true, Ordering::SeqCst) {
            xous::claim_interrupt(
                utralib::utra::irqarray1::IRQARRAY1_IRQ,
                handle_usb,
                self as *const CorigineUsb as *mut usize,
            )
            .expect("couldn't claim irq");
            let p = self.csr.r(utralib::utra::irqarray1::EV_PENDING);
            self.csr.wo(utralib::utra::irqarray1::EV_PENDING, p); // clear in case it's pending for some reason
            self.csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 1);

            // enable interrupts in corigine core
            self.csr.rmwf(USBCMD_INT_ENABLE, 1);
            // enable interruptor 0 via IMAN (we only map one in the current UTRA - if we need more
            // interruptors we have to update utra)
            self.csr.rmwf(IMAN_IE, 1);
            log::info!("interrupt claimed");
        }

        // initial ep0 transfer ring
        self.init_ep0();

        // disable u1 u2
        self.csr.wo(U3PORTPMSC, 0);

        // disable 2.0 LPM
        self.csr.wo(U2PORTPMSC, 0);

        #[cfg(feature = "std")]
        log::info!("USB init done");
    }

    pub fn init_ep0(&mut self) {
        #[cfg(feature = "std")]
        log::info!("Begin init_ep0");
        let udc_ep = &mut self.udc_ep[0];

        udc_ep.ep_num = 0;
        udc_ep.direction = 0;
        udc_ep.ep_type = USB_CONTROL_ENDPOINT;
        udc_ep.max_packet_size = 64;

        let ep0_tr_ring = unsafe {
            core::slice::from_raw_parts_mut(
                (self.ifram_base_ptr + CRG_UDC_EP0_TR_OFFSET) as *mut TransferTrbS,
                CRG_EP0_TD_RING_SIZE,
            )
        };
        for e in ep0_tr_ring.iter_mut() {
            e.zeroize();
        }
        udc_ep.tran_ring_info.vaddr = AtomicPtr::new(ep0_tr_ring.as_mut_ptr() as *mut u8); // &[TransferTrbS] ??
        udc_ep.tran_ring_info.len = (ep0_tr_ring.len() * size_of::<TransferTrbS>()) as u32;
        udc_ep.first_trb = AtomicPtr::new((&mut ep0_tr_ring[0]) as *mut TransferTrbS);
        udc_ep.last_trb =
            AtomicPtr::new((&ep0_tr_ring[ep0_tr_ring.len() - 1]) as *const TransferTrbS as *mut TransferTrbS);

        udc_ep.enq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
        udc_ep.deq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
        udc_ep.pcs = 1;
        udc_ep.tran_ring_full = false;

        unsafe { udc_ep.last_trb.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") }
            .set_trb_link(true, udc_ep.tran_ring_info.vaddr.load(Ordering::SeqCst) as *mut TransferTrbS);

        let cmd_param0: u32 = self
            .csr
            .ms(CMDPARA0_CMD0_INIT_EP0_DQPTRLO, udc_ep.tran_ring_info.vaddr.load(Ordering::SeqCst) as u32)
            | self.csr.ms(CMDPARA0_CMD0_INIT_EP0_DCS, udc_ep.pcs as u32);
        let cmd_param1: u32 = 0;
        #[cfg(feature = "std")]
        {
            log::info!(
                "ep0 ring dma addr = {:x}",
                udc_ep.tran_ring_info.vaddr.load(Ordering::SeqCst) as usize
            );
            log::info!("INIT EP0 CMD par0 = {:x} par1 = {:x}", cmd_param0, cmd_param1);
        }

        self.issue_command(CmdType::InitEp0, cmd_param0, cmd_param1)
            .expect("couldn't issue ep0 init command");

        self.ep0_buf = AtomicPtr::new((self.ifram_base_ptr + CRG_UDC_EP0_BUF_OFFSET) as *mut u8);
        #[cfg(feature = "std")]
        log::info!("End init_ep0");
    }

    pub fn issue_command(&mut self, cmd: CmdType, p0: u32, p1: u32) -> core::result::Result<(), Error> {
        let check_complete = self.csr.r(USBCMD) & self.csr.ms(USBCMD_RUN_STOP, 1) != 0;
        if check_complete {
            if self.csr.r(CMDCTRL) & self.csr.ms(CMDCTRL_ACTIVE, 1) != 0 {
                println!("issue_command(): prev command is not complete!");
                return Err(Error::CoreBusy);
            }
        }
        self.csr.wo(CMDPARA0, p0);
        self.csr.wo(CMDPARA1, p1);
        self.csr.wo(CMDCTRL, self.csr.ms(CMDCTRL_ACTIVE, 1) | self.csr.ms(CMDCTRL_TYPE, cmd as u32));
        compiler_fence(Ordering::SeqCst);
        if check_complete {
            loop {
                if self.csr.rf(CMDCTRL_ACTIVE) == 0 {
                    break;
                }
            }
            if self.csr.rf(CMDCTRL_STATUS) != 0 {
                println!("...issue_command(): fail");
                return Err(Error::CmdFailure);
            }
            println!("issue_command(): success");
        }
        Ok(())
    }

    pub fn udc_handle_interrupt(&mut self) -> CorigineEvent {
        let mut ret = CorigineEvent::None;
        let status = self.csr.r(USBSTS);
        if (status & self.csr.ms(USBSTS_SYSTEM_ERR, 1)) != 0 {
            println!("System error");
            self.csr.wfo(USBSTS_SYSTEM_ERR, 1);
            println!("USBCMD: {:x}", self.csr.r(USBCMD));
            ret = CorigineEvent::Error;
        }
        if (status & self.csr.ms(USBSTS_EINT, 1)) != 0 {
            // println!("USB Event");
            // this overwrites any previous error reporting. Seems bad, but
            // it's exactly what the reference code does.
            ret = CorigineEvent::Interrupt;
            self.csr.wfo(USBSTS_EINT, 1);
            for i in 0..CORIGINE_EVENT_RING_NUM {
                self.process_event_ring(i);
            }
        }
        ret
    }

    pub fn process_event_ring(&mut self, index: usize) {
        let uicr = self.uicr();
        // println!("ringindex: {}", index);
        let tmp = uicr[index].iman;
        if (tmp & (self.csr.ms(IMAN_IE, 1) | self.csr.ms(IMAN_IP, 1)))
            != (self.csr.ms(IMAN_IE, 1) | self.csr.ms(IMAN_IP, 1))
        {
            #[cfg(feature = "std")]
            log::info!("uicr iman[{}] = {:x}", index, tmp);
        }
        // clear IP
        uicr[index].iman |= self.csr.ms(IMAN_IP, 1);

        loop {
            let event = {
                let udc_event = &mut self.udc_event[index];
                if udc_event.evt_dq_pt.load(Ordering::SeqCst).is_null() {
                    break;
                }
                let event_ptr = udc_event.evt_dq_pt.load(Ordering::SeqCst) as usize;
                unsafe { (event_ptr as *mut EventTrbS).as_mut().expect("couldn't deref pointer") }
            };

            if event.get_cycle_bit() != self.udc_event[index].ccs {
                break;
            }

            self.handle_event(event);

            let udc_event = &mut self.udc_event[index];
            if udc_event.evt_dq_pt.load(Ordering::SeqCst)
                == udc_event.evt_seg0_last_trb.load(Ordering::SeqCst)
            {
                #[cfg(feature = "std")]
                log::info!(" evt_last_trb {:x}", udc_event.evt_seg0_last_trb.load(Ordering::SeqCst) as usize);
                udc_event.ccs = if udc_event.ccs != 0 { 0 } else { 1 };
                // does this...go to null to end the transfer??
                udc_event.evt_dq_pt =
                    AtomicPtr::new(udc_event.event_ring.vaddr.load(Ordering::SeqCst) as *mut EventTrbS);
            } else {
                udc_event.evt_dq_pt =
                    AtomicPtr::new(unsafe { udc_event.evt_dq_pt.load(Ordering::SeqCst).add(1) });
            }
        }
        let udc_event = &mut self.udc_event[index];
        // update dequeue pointer
        uicr[index].erdphi = 0;
        uicr[index].erdplo = udc_event.evt_dq_pt.load(Ordering::SeqCst) as u32 | CRG_UDC_ERDPLO_EHB;
        compiler_fence(Ordering::SeqCst);
    }

    pub fn handle_event(&mut self, event_trb: &mut EventTrbS) -> bool {
        let pei = event_trb.get_endpoint_id();
        let udc_ep = &mut self.udc_ep[pei as usize];
        // println!("handle_event() event_trb: {:x?}", event_trb);
        match event_trb.get_trb_type() {
            TrbType::EventPortStatusChange => {
                let portsc_val = self.csr.r(PORTSC);
                self.csr.wo(PORTSC, portsc_val);
                self.print_status(portsc_val);
                let cs = (portsc_val & self.csr.ms(PORTSC_CCS, 1)) != 0;
                let pp = (portsc_val & self.csr.ms(PORTSC_PP, 1)) != 0;
                #[cfg(feature = "std")]
                log::info!("  {:x} {:x} PORT_STATUS_CHANGE", portsc_val, event_trb.dw3);

                if portsc_val & self.csr.ms(PORTSC_CSC, 1) != 0 {
                    if cs {
                        #[cfg(not(feature = "std"))]
                        println!("  Port connection");
                        #[cfg(feature = "std")]
                        log::info!("  Port connection");
                    } else {
                        #[cfg(not(feature = "std"))]
                        println!("  Port disconnection");
                        #[cfg(feature = "std")]
                        log::info!("  Port disconnection");
                    }
                }

                if portsc_val & self.csr.ms(PORTSC_PPC, 1) != 0 {
                    if pp {
                        #[cfg(not(feature = "std"))]
                        println!("  Power present");
                        #[cfg(feature = "std")]
                        log::info!("  Power present");
                    } else {
                        #[cfg(not(feature = "std"))]
                        println!("  Power not present");
                        #[cfg(feature = "std")]
                        log::info!("  Power not present");
                    }
                }

                if (portsc_val & self.csr.ms(PORTSC_CSC, 1) != 0)
                    && (portsc_val & self.csr.ms(PORTSC_PPC, 1) != 0)
                {
                    if cs && pp {
                        #[cfg(not(feature = "std"))]
                        println!("  Cable connect and power present");
                        #[cfg(feature = "std")]
                        log::info!("  Cable connect and power present");
                        self.update_current_speed();
                    }
                }

                if (portsc_val & self.csr.ms(PORTSC_PRC, 1)) != 0 {
                    if portsc_val & self.csr.ms(PORTSC_PR, 1) != 0 {
                        #[cfg(not(feature = "std"))]
                        println!("  In port reset process");
                        #[cfg(feature = "std")]
                        log::info!("  In port reset process");
                    } else {
                        #[cfg(not(feature = "std"))]
                        println!("  Port reset done");
                        #[cfg(feature = "std")]
                        log::info!("  Port reset done");
                        self.update_current_speed();
                    }
                }

                if (portsc_val & self.csr.ms(PORTSC_PLC, 1)) != 0 {
                    #[cfg(not(feature = "std"))]
                    println!("  Port link state change: {:?}", PortLinkState::from_portsc(portsc_val));
                    #[cfg(feature = "std")]
                    log::info!("  Port link state change: {:?}", PortLinkState::from_portsc(portsc_val));
                }

                if !cs && !pp {
                    #[cfg(not(feature = "std"))]
                    println!("  cable disconnect and power not present");
                    #[cfg(feature = "std")]
                    log::info!("  cable disconnect and power not present");
                }

                self.csr.rmwf(EVENTCONFIG_SETUP_ENABLE, 1);
            }
            TrbType::EventTransfer => {
                let comp_code = CompletionCode::try_from(event_trb.dw2).expect("Invalid completion code");

                // update the dequeue pointer
                let deq_pt = unsafe {
                    (event_trb.dw0 as *mut TransferTrbS).add(1).as_mut().expect("Couldn't deref ptr")
                };
                if deq_pt.get_trb_type() == TrbType::Link {
                    udc_ep.deq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
                } else {
                    udc_ep.deq_pt = AtomicPtr::new(deq_pt as *mut TransferTrbS);
                }
                #[cfg(feature = "std")]
                log::info!("EventTransfer: comp_code {:?}, PEI {}", comp_code, pei);

                if pei == 0 {
                    self.ep0_xfer_complete(event_trb);
                } else if pei >= 2 {
                    if comp_code == CompletionCode::Success || comp_code == CompletionCode::ShortPacket {
                        self.xfer_complete(event_trb);
                    } else if comp_code == CompletionCode::MissedServiceError {
                        println!("MissedServiceError");
                    } else {
                        println!("EventTransfer event not handled");
                    }
                }
            }
            TrbType::SetupPkt => {
                #[cfg(feature = "std")]
                log::info!("  handle_setup_pkt");
                self.setup.copy_from_slice(&event_trb.get_raw_setup());
                self.setup_tag = event_trb.get_setup_tag();
                #[cfg(feature = "std")]
                log::info!(" setup_pkt = {:x?}, setup_tag = {:x}", self.setup, self.setup_tag);

                println!("setup handler: placeholder TODO");
            }
            _ => {
                println!("Unexpected trb_type {:?}", event_trb.get_trb_type());
            }
        }
        false
    }

    pub fn ep0_xfer_complete(&mut self, event_trb: &mut EventTrbS) {
        println!("ep0 xfer complete: placeholder TODO");
    }

    pub fn xfer_complete(&mut self, event_trb: &mut EventTrbS) {
        println!("xfer complete: placeholder TODO");
    }

    pub fn update_current_speed(&mut self) {
        match PortSpeed::from_portsc(self.csr.r(PORTSC)) {
            PortSpeed::SspGen2x2 | PortSpeed::SspGen1x2 | PortSpeed::SspGen2x1 => {
                self.speed = UsbDeviceSpeed::SuperPlus;
                self.update_ep0_maxpacketsize(512);
            }
            PortSpeed::Ss => {
                self.speed = UsbDeviceSpeed::Super;
                self.update_ep0_maxpacketsize(512);
            }
            PortSpeed::Hs => {
                self.speed = UsbDeviceSpeed::High;
                self.update_ep0_maxpacketsize(64);
            }
            PortSpeed::Fs => {
                self.speed = UsbDeviceSpeed::Full;
                self.update_ep0_maxpacketsize(64);
            }
            _ => self.speed = UsbDeviceSpeed::Unknown,
        }
    }

    pub fn update_ep0_maxpacketsize(&mut self, size: usize) {
        let cmd_param = self.csr.ms(CMDPARA0_CMD1_UPDATE_EP0_MPS, size as u32);
        self.issue_command(CmdType::UpdateEp0, cmd_param, 0).expect("couldn't issue command");
    }

    /// TODO: make this not a sham
    pub fn pp(&self) -> bool { true }

    pub fn stop(&mut self) {
        self.csr.rmwf(USBCMD_INT_ENABLE, 0);
        self.csr.rmwf(USBCMD_RUN_STOP, 0);
    }

    pub fn set_addr(&mut self, addr: u8, target: u32) {
        self.device_state = UsbDeviceState::Address;
        self.feature_u1_enabled = 0;
        self.feature_u2_enabled = 0;
        self.issue_command(CmdType::SetAddr, self.csr.ms(CMDPARA0_CMD2_SET_ADDR, addr as u32), 0)
            .expect("couldn't issue command");

        let udc_ep = &mut self.udc_ep[0];
        let enq_pt =
            unsafe { udc_ep.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        enq_pt.set_trb_status(udc_ep.pcs, true, false, self.setup_tag, target, false);

        // TODO: fix raw pointer manips with something more sane?
        udc_ep.enq_pt = AtomicPtr::new(unsafe { udc_ep.enq_pt.load(Ordering::SeqCst).add(1) });
        let enq_pt =
            unsafe { udc_ep.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        if enq_pt.get_trb_type() == TrbType::Link {
            enq_pt.set_cycle_bit(udc_ep.pcs);
            udc_ep.enq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
            udc_ep.pcs ^= 1;
        }
        self.knock_doorbell(0);
    }

    // knock door bell then controller will start transfer for the specific endpoint
    // pei: physical endpoint index
    pub fn knock_doorbell(&mut self, pei: u32) { self.csr.wfo(DOORBELL_TARGET, pei); }

    pub fn ccs(&self) -> bool { self.csr.rf(PORTSC_CCS) != 0 }

    pub fn print_status(&self, status: u32) {
        let bitflags = [
            (0u32, "CCS"),
            (3u32, "PP"),
            (4u32, "PR"),
            (16u32, "LWS"),
            (17u32, "CSC"),
            (18u32, "PEC"),
            (20u32, "PPC"),
            (21u32, "PRC"),
            (22u32, "PLC"),
            (23u32, "CEC"),
            (25u32, "WCE"),
            (26u32, "WDE"),
            (31u32, "WPR"),
        ];
        let plses = [
            "U0 (USB3 & USB2)",  //  0 -
            "U1 (USB3)",         //  1 -
            "U2 (USB3 & USB2)",  //  2 -
            "U3 (USB3 & USB2)",  //  3 -
            "Disabled (USB3)",   //  4 -
            "RxDetect (USB3)",   //  5 -
            "Inactive (USB3)",   //  6 -
            "Polling (USB3)",    //  7 -
            "Recovery (USB3)",   //  8 -
            "Hot Reset (USB3)",  //  9 -
            "Compliance (USB3)", // 10 -
            "Test Mode (USB2)",  // 11 -
            "Invalid12",         // 12 -
            "Invalid13",         // 13 -
            "Invalid14",         // 14 -
            "Resume (USB2)",     // 15 -
        ];
        let speeds = ["Invalid", "FS", "Invalid", "HS", "SS", "SSP", "Unknown", "Unknown"];
        #[cfg(not(feature = "std"))]
        {
            println!("Status: {:x}", status);
            print!("   ");
            for &(field, name) in bitflags.iter() {
                if (status & 1 << field) != 0 {
                    print!("{} ", name);
                }
            }
            println!("");
            println!("   Speed: {}", speeds[((status >> 10) & 0x7) as usize]);
            println!("   PLS: {}", plses[((status >> 5) & 0xF) as usize]);
        }
        #[cfg(feature = "std")]
        {
            let mut s = String::new();
            s.push_str(&format!("\n\rStatus: {:x}\n\r", status));
            s.push_str("   ");
            for &(field, name) in bitflags.iter() {
                if (status & 1 << field) != 0 {
                    s.push_str(&format!("{} ", name));
                }
            }
            s.push_str(&format!(
                "\n\r   Speed: {}\n\r   PLS: {}\n\r",
                speeds[((status >> 10) & 0x7) as usize],
                plses[((status >> 5) & 0xF) as usize]
            ));
            log::info!("{}", s);
        }
    }

    pub fn start(&mut self) {
        self.csr.wo(
            EVENTCONFIG,
            self.csr.ms(EVENTCONFIG_CSC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PEC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PPC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PRC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PLC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_CEC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_INACTIVE_PLC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_USB3_RESUME_NO_PLC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_USB2_RESUME_NO_PLC_ENABLE, 1),
        );

        self.csr.wo(
            USBCMD,
            self.csr.r(USBCMD)
                | self.csr.ms(USBCMD_SYS_ERR_ENABLE, 1)
                | self.csr.ms(USBCMD_INT_ENABLE, 1)
                | self.csr.ms(USBCMD_RUN_STOP, 1),
        );

        self.print_status(self.csr.r(PORTSC));

        self.set_addr(0, 0);
    }

    /*
     Functions below implement the "device management" APIs.

     These are mostly dummy thunks on the Cramium target because only a single core is possible
     on this SoC.

     This would be cleaner if we implemented it as a trait, I think - but we've got a lot to do
     in term sof getting the cores up and running, so we'll leave this as technical debt for when
     we uh...decide to implement a third target, or something like that.
    */
    /// Force and hold the reset pin according to the state selected
    pub fn ll_reset(&mut self, state: bool) {
        #[cfg(feature = "std")]
        log::warn!("ll_reset is UNSURE");
        // There is a PHY control, it looks like 0x1C bit 1 set to 1 will cause the device to hi-Z

        // But we might not even have to do that, I think it could be the case that we may just
        // need to do a soft-disconnect routine per 8.3.4 in manual.
        if state {
            self.reset();
        } else {
            self.init();
        }
    }

    pub fn ll_connect_device_core(&self, _state: bool) {}

    pub fn connect_device_core(&self, _state: bool) {}

    pub fn is_device_connected(&self) -> bool { true }

    pub fn disable_debug(&mut self, _disable: bool) {}

    pub fn get_disable_debug(&self) -> bool { true }

    pub fn xous_suspend(&self) {}

    pub fn xous_resume1(&self) {}

    pub fn xous_resume2(&self) {}
}

#[cfg(feature = "std")]
pub struct CorigineWrapper {
    pub hw: Arc<Mutex<CorigineUsb>>,
}
#[cfg(feature = "std")]
impl CorigineWrapper {
    pub fn new(obj: CorigineUsb) -> Self { Self { hw: Arc::new(Mutex::new(obj)) } }
}

#[cfg(feature = "std")]
impl UsbBus for CorigineWrapper {
    /// Indicates that `set_device_address` must be called before accepting the corresponding
    /// control transfer, not after.
    ///
    /// The default value for this constant is `false`, which corresponds to the USB 2.0 spec, 9.4.6
    const QUIRK_SET_ADDRESS_BEFORE_STATUS: bool = false;

    /// Allocates an endpoint and specified endpoint parameters. This method is called by the device
    /// and class implementations to allocate endpoints, and can only be called before
    /// [`enable`](UsbBus::enable) is called.
    ///
    /// # Arguments
    ///
    /// * `ep_dir` - The endpoint direction.
    /// * `ep_addr` - A static endpoint address to allocate. If Some, the implementation should attempt to
    ///   return an endpoint with the specified address. If None, the implementation should return the next
    ///   available one.
    /// * `max_packet_size` - Maximum packet size in bytes.
    /// * `interval` - Polling interval parameter for interrupt endpoints.
    ///
    /// # Errors
    ///
    /// * [`EndpointOverflow`](crate::UsbError::EndpointOverflow) - Available total number of endpoints,
    ///   endpoints of the specified type, or endpoind packet memory has been exhausted. This is generally
    ///   caused when a user tries to add too many classes to a composite device.
    /// * [`InvalidEndpoint`](crate::UsbError::InvalidEndpoint) - A specific `ep_addr` was specified but the
    ///   endpoint in question has already been allocated.
    fn alloc_ep(
        &mut self,
        ep_dir: UsbDirection,
        ep_addr: Option<EndpointAddress>,
        ep_type: EndpointType,
        max_packet_size: u16,
        interval: u8,
    ) -> Result<EndpointAddress> {
        Ok(EndpointAddress::from_parts(1, UsbDirection::Out))
    }

    /// Enables and initializes the USB peripheral. Soon after enabling the device will be reset, so
    /// there is no need to perform a USB reset in this method.
    fn enable(&mut self) {}

    /// Called when the host resets the device. This will be soon called after
    /// [`poll`](crate::device::UsbDevice::poll) returns [`PollResult::Reset`]. This method should
    /// reset the state of all endpoints and peripheral flags back to a state suitable for
    /// enumeration, as well as ensure that all endpoints previously allocated with alloc_ep are
    /// initialized as specified.
    fn reset(&self) {}

    /// Sets the device USB address to `addr`.
    fn set_device_address(&self, addr: u8) { self.hw.lock().unwrap().set_addr(addr, CRG_INT_TARGET); }

    /// Writes a single packet of data to the specified endpoint and returns number of bytes
    /// actually written.
    ///
    /// The only reason for a short write is if the caller passes a slice larger than the amount of
    /// memory allocated earlier, and this is generally an error in the class implementation.
    ///
    /// # Errors
    ///
    /// * [`InvalidEndpoint`](crate::UsbError::InvalidEndpoint) - The `ep_addr` does not point to a valid
    ///   endpoint that was previously allocated with [`UsbBus::alloc_ep`].
    /// * [`WouldBlock`](crate::UsbError::WouldBlock) - A previously written packet is still pending to be
    ///   sent.
    /// * [`BufferOverflow`](crate::UsbError::BufferOverflow) - The packet is too long to fit in the
    ///   transmission buffer. This is generally an error in the class implementation, because the class
    ///   shouldn't provide more data than the `max_packet_size` it specified when allocating the endpoint.
    ///
    /// Implementations may also return other errors if applicable.
    fn write(&self, ep_addr: EndpointAddress, buf: &[u8]) -> Result<usize> { Ok(0) }

    /// Reads a single packet of data from the specified endpoint and returns the actual length of
    /// the packet.
    ///
    /// This should also clear any NAK flags and prepare the endpoint to receive the next packet.
    ///
    /// # Errors
    ///
    /// * [`InvalidEndpoint`](crate::UsbError::InvalidEndpoint) - The `ep_addr` does not point to a valid
    ///   endpoint that was previously allocated with [`UsbBus::alloc_ep`].
    /// * [`WouldBlock`](crate::UsbError::WouldBlock) - There is no packet to be read. Note that this is
    ///   different from a received zero-length packet, which is valid in USB. A zero-length packet will
    ///   return `Ok(0)`.
    /// * [`BufferOverflow`](crate::UsbError::BufferOverflow) - The received packet is too long to fit in
    ///   `buf`. This is generally an error in the class implementation, because the class should use a buffer
    ///   that is large enough for the `max_packet_size` it specified when allocating the endpoint.
    ///
    /// Implementations may also return other errors if applicable.
    fn read(&self, ep_addr: EndpointAddress, buf: &mut [u8]) -> Result<usize> { Ok(0) }

    /// Sets or clears the STALL condition for an endpoint. If the endpoint is an OUT endpoint, it
    /// should be prepared to receive data again.
    fn set_stalled(&self, _ep_addr: EndpointAddress, _stalled: bool) {}

    /// Gets whether the STALL condition is set for an endpoint.
    fn is_stalled(&self, _ep_addr: EndpointAddress) -> bool { false }

    /// Instruct EP0 to configure itself with an OUT descriptor, so that it may receive a STATUS
    /// update during configuration. This is for devices that support an EP0 which can only either
    /// be IN or OUT, but not both at the same time. Devices with both IN/OUT may leave this as
    /// an empty stub.
    fn set_ep0_out(&self) {}

    /// Causes the USB peripheral to enter USB suspend mode, lowering power consumption and
    /// preparing to detect a USB wakeup event. This will be called after
    /// [`poll`](crate::device::UsbDevice::poll) returns [`PollResult::Suspend`]. The device will
    /// continue be polled, and it shall return a value other than `Suspend` from `poll` when it no
    /// longer detects the suspend condition.
    fn suspend(&self) {}

    /// Resumes from suspend mode. This may only be called after the peripheral has been previously
    /// suspended.
    fn resume(&self) {}

    /// Gets information about events and incoming data. Usually called in a loop or from an
    /// interrupt handler. See the [`PollResult`] struct for more information.
    fn poll(&self) -> PollResult {
        match self.hw.lock().unwrap().udc_handle_interrupt() {
            CorigineEvent::Interrupt => PollResult::None, /* PollResult::Data { ep_out: (), */
            // ep_in_complete: (), ep_setup: () },
            CorigineEvent::None => PollResult::None,
            CorigineEvent::Error => PollResult::Reset,
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
    fn force_reset(&self) -> Result<()> { Err(UsbError::Unsupported) }
}
