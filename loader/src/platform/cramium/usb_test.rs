use core::convert::TryFrom;
use core::mem::size_of;
use core::sync::atomic::{compiler_fence, Ordering};

use corigine_usb::*;
use utralib::*;

use crate::{print, println};

const CRG_EVENT_RING_NUM: usize = 1;
const CRG_ERST_SIZE: usize = 1;
const CRG_EVENT_RING_SIZE: usize = 128;
const CRG_EP0_TD_RING_SIZE: usize = 16;
const CRG_EP_NUM: usize = 4;
const CRG_TD_RING_SIZE: usize = 1280;
const CRG_UDC_MAX_BURST: usize = 15;
const CRG_UDC_ISO_INTERVAL: usize = 3;

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

const CRG_IFRAM_PAGES: usize = 16;
const CRG_UDC_MEMBASE: usize = utralib::HW_IFRAM1_MEM + utralib::HW_IFRAM1_MEM_LEN - CRG_IFRAM_PAGES * 0x1000;

const CRG_UDC_ERST_OFFSET: usize = 0; // use relative offsets
const CRG_UDC_EVENTRING_OFFSET: usize = CRG_UDC_ERST_OFFSET + CRG_UDC_ERSTSIZE;
const CRG_UDC_EPCX_OFFSET: usize = CRG_UDC_EVENTRING_OFFSET + CRG_UDC_EVENTRINGSIZE;

const CRG_UDC_EP0_TR_OFFSET: usize = CRG_UDC_EPCX_OFFSET + CRG_UDC_EPCXSIZE;
const CRG_UDC_EP_TR_OFFSET: usize = CRG_UDC_EP0_TR_OFFSET + CRG_UDC_EP0_TRSIZE;
const CRG_UDC_EP0_BUF_OFFSET: usize = CRG_UDC_EP_TR_OFFSET + CRG_UDC_EP_TRSIZE;
const CRG_UDC_APP_BUFOFFSET: usize = CRG_UDC_EP0_BUF_OFFSET + CRG_UDC_EP0_REQBUFSIZE;

// total size 0x15300
#[derive(Debug)]
pub enum Error {
    CoreBusy,
    CmdFailure,
    InvalidState,
}

#[derive(Eq, PartialEq, Copy, Clone)]
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

/// symbolic mapping of UDC registers into fields
#[repr(C)]
pub struct Uccr {
    capability: u32, /* 0x00 */
    resv0: [u32; 3],

    config0: u32, /* 0x10 */
    config1: u32,
    resv1: [u32; 2],

    control: u32, /* 0x20 USBCMD */
    status: u32,
    /* device context base address (DCBA) Pointer Low */
    dcbaplo: u32,
    /* device context base address (DCBA) Pointer High */
    dcbaphi: u32,
    /* PORT Status and Control */
    portsc: u32,
    /* USB3 Port PM Status and Control */
    u3portpmsc: u32,
    /* USB2 Port PM Status and Control */
    u2portpmsc: u32,
    /* USB3 Port Link Info */
    u3portli: u32,

    /* Door Bell Register */
    doorbell: u32, /* 0x40 */
    /* Microframe Index */
    mfindex: u32,
    ptm_ctr: u32,
    ptm_sts: u32,
    ep0_ctrl: u32,
    resv3: [u32; 3],

    ep_enable: u32, /* 0x60 */
    ep_running: u32,
    resv4: [u32; 2],

    /* Command Parameter 0 */
    cmd_param0: u32, /* 0x70 */
    /* Command Parameter 1 */
    cmd_param1: u32,
    /* Command Control */
    cmd_control: u32,
    resv5: [u32; 1],

    odb_capability: u32, /* 0x80 */
    resv6k: [u32; 3],

    /* Command Control 90-a0 */
    odb_config: [u32; 8],

    debug0: u32, /* 0xB0 */
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

    fn try_from(value: u32) -> Result<Self, Error> {
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
    fn try_from(dw2: u32) -> Result<Self, Error> {
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
        println!("trb_status dw2 {:x} dw3 {:x}", self.dw2, self.dw3);
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
    vaddr: *mut u8,
    dma: u64,
    len: u32,
}
impl Default for BufferInfo {
    fn default() -> Self { Self { vaddr: core::ptr::null_mut(), dma: 0, len: 0 } }
}

pub struct UdcEp {
    // Endpoint number
    ep_num: u8,
    // Endpoint direction
    direction: u8,
    ep_type: u8,
    max_packet_size: u16,
    tran_ring_info: BufferInfo,
    first_trb: *mut TransferTrbS,
    last_trb: *mut TransferTrbS,
    enq_pt: *mut TransferTrbS,
    deq_pt: *mut TransferTrbS,
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
            first_trb: core::ptr::null_mut(),
            last_trb: core::ptr::null_mut(),
            enq_pt: core::ptr::null_mut(),
            deq_pt: core::ptr::null_mut(),
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
    p_erst: *mut ErstS,
    event_ring: BufferInfo,
    evt_dq_pt: *mut EventTrbS,
    ccs: u8,
    evt_seg0_last_trb: *mut EventTrbS,
}
impl Default for UdcEvent {
    fn default() -> Self {
        Self {
            erst: BufferInfo::default(),
            p_erst: core::ptr::null_mut(),
            event_ring: BufferInfo::default(),
            evt_dq_pt: core::ptr::null_mut(),
            ccs: 0,
            evt_seg0_last_trb: core::ptr::null_mut(),
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
    #[cfg(feature = "std")]
    range: xous::MemoryRange,
    #[cfg(feature = "std")]
    ifram_range: xous::MemoryRange,
    ifram_base_ptr: usize,
    csr: CSR<u32>,
    // Because the init routine requires magic pokes
    magic_page: &'static mut [u32],
    // Seems necessary for some debug tricks
    dev_slice: &'static mut [u32],

    udc_ep: [UdcEp; CRG_EP_NUM + 2],
    ep_cx: BufferInfo,
    p_epcx: *mut EpCxS,

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
    ep0_buf: *mut u8,

    u2_rwe: u32,
    feature_u1_enabled: u32,
    feature_u2_enabled: u32,

    setup_tag_mismatch_found: u32,
    portsc_on_reconnecting: u32,
    max_speed: u32,
}
impl CorigineUsb {
    pub fn new() -> Self {
        #[cfg(feature = "std")]
        let usb_mapping = xous::syscall::map_memory(
            xous::MemoryAddress::new(CORIGINE_USB_BASE),
            None,
            CORIGINE_USB_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        );
        #[cfg(feature = "std")]
        let ifram_range = xous::syscall::map_memory(
            xous::MemoryAddress::new(CRG_UDC_MEMBASE),
            None,
            CRG_IFRAM_PAGES * 0x1000,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        );
        #[cfg(feature = "std")]
        let magic_page = usb_mapping.as_slice_mut();
        #[cfg(not(feature = "std"))]
        let magic_page = unsafe { core::slice::from_raw_parts_mut(CORIGINE_USB_BASE as *mut u32, 1024) };

        // note that the extent of this slice goes beyond the strict end of the register set because
        // I think there are extra hidden registers we may need to access later on.
        #[cfg(feature = "std")]
        let dev_slice = usb_mapping.as_slice_mut()[CORIGINE_DEV_OFFSET / size_of::<u32>()
            ..CORIGINE_DEV_OFFSET / size_of::<u32>() + 0x200 / size_of::<u32>()];
        #[cfg(not(feature = "std"))]
        let dev_slice = unsafe {
            core::slice::from_raw_parts_mut(
                (CORIGINE_USB_BASE + CORIGINE_DEV_OFFSET) as *mut u32,
                0x200 / size_of::<u32>(),
            )
        };

        Self {
            #[cfg(feature = "std")]
            range: usb_mapping,
            #[cfg(feature = "std")]
            ifram_range,
            #[cfg(feature = "std")]
            ifram_base_ptr: ifram_range.as_ptr() as usize,
            #[cfg(not(feature = "std"))]
            ifram_base_ptr: CRG_UDC_MEMBASE,
            #[cfg(feature = "std")]
            csr: CSR::new(usb_mapping.as_mut_ptr() as *mut u32),
            #[cfg(not(feature = "std"))]
            csr: CSR::new(CORIGINE_USB_BASE as *mut u32),
            magic_page,
            dev_slice,
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
            p_epcx: core::ptr::null_mut(),
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
            ep0_buf: core::ptr::null_mut(),
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

    fn uccr(&self) -> &'static mut Uccr {
        // Safety: only safe because this is an aligned, allocated region
        // of hardware registers; all values are representable as u32; and the structure
        // fits the data.
        unsafe { ((self.csr.base() as usize) as *mut Uccr).as_mut().unwrap() }
    }

    pub fn reset(&mut self) {
        let uccr = self.uccr();

        println!("devcap: {:x}", uccr.capability);
        println!("max speed: {:x}", self.csr.rf(corigine_usb::DEVCONFIG_MAX_SPEED));
        println!("usb3 disable: {:x}", self.csr.rf(corigine_usb::DEVCONFIG_USB3_DISABLE_COUNT));

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
        // dummy readback, from the sample code. not sure if important
        for i in 0..72 {
            println!("Dummy {}: {:x}", i, self.dev_slice[i]);
        }
        compiler_fence(Ordering::SeqCst);

        println!("USB reset done");
    }

    pub fn init(&mut self) {
        let uccr = self.uccr();
        let uicr = self.uicr();

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
        uccr.config0 = 0x80 | CRG_UDC_CFG0_MAXSPEED_FS;
        compiler_fence(Ordering::SeqCst);
        println!("config0: {:x}", uccr.config0);

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
            udc_event.erst.vaddr = erst.as_mut_ptr() as *mut u8; // ErstS ??
            udc_event.p_erst = udc_event.erst.vaddr as *mut ErstS;

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
            // TODO: V->P
            udc_event.event_ring.vaddr = event_ring.as_mut_ptr(); // EventTrbS ??
            udc_event.evt_dq_pt = udc_event.event_ring.vaddr as *mut EventTrbS;
            udc_event.evt_seg0_last_trb =
                unsafe { (udc_event.event_ring.vaddr as *mut EventTrbS).add(CRG_EVENT_RING_SIZE - 1) };

            udc_event.ccs = 1;

            // copy control structure pointers to hardware-managed memory
            let p_erst = unsafe { udc_event.p_erst.as_mut() }.expect("invalid pointer");
            p_erst.seg_addr_lo = udc_event.event_ring.vaddr as u32;
            p_erst.seg_addr_hi = 0;
            p_erst.seg_size = CRG_EVENT_RING_SIZE as u32;
            p_erst.rsvd = 0;

            uicr[index].erstsz = CRG_ERST_SIZE as u32;
            uicr[index].erstbalo = udc_event.erst.vaddr as u32;
            uicr[index].erstbahi = 0;
            uicr[index].erdplo = udc_event.event_ring.vaddr as u32 | self.csr.ms(ERDPLO_EHB, 1);
            uicr[index].erdphi = 0;

            uicr[index].iman = self.csr.ms(IMAN_IE, 1) | self.csr.ms(IMAN_IP, 1);
            uicr[index].imod = 0;
            compiler_fence(Ordering::SeqCst);
        }

        // init_device_context
        // init device context and ep context, refer to 7.6.2
        self.ep_cx.len = (CRG_EP_NUM * size_of::<EpCxS>()) as u32;
        self.ep_cx.vaddr = (self.ifram_base_ptr + CRG_UDC_EPCX_OFFSET) as *mut u8; // EpCxS ??
        self.p_epcx = self.ep_cx.vaddr as *mut EpCxS;

        uccr.dcbaplo = self.ep_cx.vaddr as u32;
        uccr.dcbaphi = 0;
        compiler_fence(Ordering::SeqCst);
        println!(" dcbaplo: {:x}", uccr.dcbaplo);
        println!(" dcbaphi: {:x}", uccr.dcbaphi);

        // initial ep0 transfer ring
        self.init_ep0();

        // disable u1 u2
        uccr.u3portpmsc = 0;

        // disable 2.0 LPM
        uccr.u2portpmsc = 0;

        println!("USB init done");
    }

    pub fn init_ep0(&mut self) {
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
        // TODO: V->P
        udc_ep.tran_ring_info.vaddr = ep0_tr_ring.as_mut_ptr() as *mut u8; // &[TransferTrbS] ??
        udc_ep.tran_ring_info.len = (ep0_tr_ring.len() * size_of::<TransferTrbS>()) as u32;
        udc_ep.first_trb = (&mut ep0_tr_ring[0]) as *mut TransferTrbS;
        // TODO: P->V
        udc_ep.last_trb = (&ep0_tr_ring[ep0_tr_ring.len() - 1]) as *const TransferTrbS as *mut TransferTrbS;

        udc_ep.enq_pt = udc_ep.first_trb;
        udc_ep.deq_pt = udc_ep.first_trb;
        udc_ep.pcs = 1;
        udc_ep.tran_ring_full = false;

        unsafe { udc_ep.last_trb.as_mut().expect("couldn't deref pointer") }
            .set_trb_link(true, udc_ep.tran_ring_info.vaddr as *mut TransferTrbS);

        let cmd_param0: u32 = self.csr.ms(CMDPARA0_CMD0_INIT_EP0_DQPTRLO, udc_ep.tran_ring_info.vaddr as u32)
            | self.csr.ms(CMDPARA0_CMD0_INIT_EP0_DCS, udc_ep.pcs as u32);
        let cmd_param1: u32 = 0;
        println!("ep0 ring dma addr = {:x}", udc_ep.tran_ring_info.vaddr as usize);
        println!("INIT EP0 CMD par0 = {:x} par1 = {:x}", cmd_param0, cmd_param1);

        self.issue_command(CmdType::InitEp0, cmd_param0, cmd_param1)
            .expect("couldn't issue ep0 init command");

        self.ep0_buf = (self.ifram_base_ptr + CRG_UDC_EP0_BUF_OFFSET) as *mut u8;
    }

    pub fn issue_command(&mut self, cmd: CmdType, p0: u32, p1: u32) -> Result<(), Error> {
        let uccr = self.uccr();
        let check_complete = uccr.control & self.csr.ms(USBCMD_RUN_STOP, 1) != 0;
        if check_complete {
            if uccr.cmd_control & self.csr.ms(CMDCTRL_ACTIVE, 1) != 0 {
                println!("issue_command(): prev command is not complete!");
                return Err(Error::CoreBusy);
            }
        }
        uccr.cmd_param0 = p0;
        uccr.cmd_param1 = p1;
        uccr.cmd_control = self.csr.ms(CMDCTRL_ACTIVE, 1) | self.csr.ms(CMDCTRL_TYPE, cmd as u32);
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
        println!("ringindex: {}", index);
        let tmp = uicr[index].iman;
        if (tmp & (self.csr.ms(IMAN_IE, 1) | self.csr.ms(IMAN_IP, 1)))
            != (self.csr.ms(IMAN_IE, 1) | self.csr.ms(IMAN_IP, 1))
        {
            println!("uicr iman[{}] = {:x}", index, tmp);
        }
        // clear IP
        uicr[index].iman |= self.csr.ms(IMAN_IP, 1);

        loop {
            let event = {
                let udc_event = &mut self.udc_event[index];
                if udc_event.evt_dq_pt.is_null() {
                    break;
                }
                let event_ptr = udc_event.evt_dq_pt as usize;
                unsafe { (event_ptr as *mut EventTrbS).as_mut().expect("couldn't deref pointer") }
            };

            if event.get_cycle_bit() != self.udc_event[index].ccs {
                break;
            }

            self.handle_event(event);

            let udc_event = &mut self.udc_event[index];
            if udc_event.evt_dq_pt == udc_event.evt_seg0_last_trb {
                println!(" evt_last_trb {:x}", udc_event.evt_seg0_last_trb as usize);
                udc_event.ccs = if udc_event.ccs != 0 { 0 } else { 1 };
                // does this...go to null to end the transfer??
                udc_event.evt_dq_pt = udc_event.event_ring.vaddr as *mut EventTrbS;
            } else {
                udc_event.evt_dq_pt = unsafe { udc_event.evt_dq_pt.add(1) };
            }
        }
        let udc_event = &mut self.udc_event[index];
        // update dequeue pointer
        uicr[index].erdphi = 0;
        uicr[index].erdplo = udc_event.evt_dq_pt as u32 | CRG_UDC_ERDPLO_EHB;
        compiler_fence(Ordering::SeqCst);
    }

    pub fn handle_event(&mut self, event_trb: &mut EventTrbS) -> bool {
        let pei = event_trb.get_endpoint_id();
        let udc_ep = &mut self.udc_ep[pei as usize];
        println!("event_trb: {:x?}", event_trb);
        match event_trb.get_trb_type() {
            TrbType::EventPortStatusChange => {
                let portsc_val = self.csr.r(PORTSC);
                self.csr.wo(PORTSC, portsc_val);
                let cs = (portsc_val & self.csr.ms(PORTSC_CCS, 1)) != 0;
                let pp = (portsc_val & self.csr.ms(PORTSC_PP, 1)) != 0;

                println!("  Current port link state is {:x}", portsc_val);

                if portsc_val & self.csr.ms(PORTSC_CSC, 1) != 0 {
                    if cs {
                        println!("  Port connection");
                    } else {
                        println!("  Port disconnection");
                    }
                }

                if portsc_val & self.csr.ms(PORTSC_PPC, 1) != 0 {
                    if pp {
                        println!("  Power present");
                    } else {
                        println!("  Power not present");
                    }
                }

                if (portsc_val & self.csr.ms(PORTSC_CSC, 1) != 0)
                    && (portsc_val & self.csr.ms(PORTSC_PPC, 1) != 0)
                {
                    if cs && pp {
                        println!("  Cable connect and power present");
                        self.update_current_speed();
                    }
                }

                if (portsc_val & self.csr.ms(PORTSC_PRC, 1)) != 0 {
                    if portsc_val & self.csr.ms(PORTSC_PR, 1) != 0 {
                        println!("  In port reset process");
                    } else {
                        println!("  Port reset done");
                    }
                }

                if (portsc_val & self.csr.ms(PORTSC_PLC, 1)) != 0 {
                    println!("  Port link state change: {:?}", PortLinkState::from_portsc(portsc_val));
                }

                if !cs && !pp {
                    println!("  cable disconnect and power not present");
                }

                self.csr.rmwf(EVENTCONFIG_SETUP_ENABLE, 1);
            }
            TrbType::EventTransfer => {
                let comp_code = CompletionCode::try_from(event_trb.dw2).expect("Invalid completion code");

                // update the dequeue pointer
                // Todo: physical to virtual address translation so we can check the TRB type.
                let deq_pt = unsafe {
                    (event_trb.dw0 as *mut TransferTrbS).add(1).as_mut().expect("Couldn't deref ptr")
                };
                if deq_pt.get_trb_type() == TrbType::Link {
                    udc_ep.deq_pt = udc_ep.first_trb;
                } else {
                    udc_ep.deq_pt = deq_pt as *mut TransferTrbS;
                }
                println!("EventTransfer: comp_code {:?}, PEI {}", comp_code, pei);

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
                println!("  handle_setup_pkt");
                self.setup.copy_from_slice(&event_trb.get_raw_setup());
                self.setup_tag = event_trb.get_setup_tag();
                println!(" setup_pkt = {:x?}, setup_tag = {:x}", self.setup, self.setup_tag);

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
        // TODO: P->A
        let enq_pt = unsafe { udc_ep.enq_pt.as_mut().expect("couldn't deref pointer") };
        enq_pt.set_trb_status(udc_ep.pcs, true, false, self.setup_tag, target, false);

        // TODO: fix raw pointer manips with something more sane?
        udc_ep.enq_pt = unsafe { udc_ep.enq_pt.add(1) };
        // TODO: P->A
        let enq_pt = unsafe { udc_ep.enq_pt.as_mut().expect("couldn't deref pointer") };
        if enq_pt.get_trb_type() == TrbType::Link {
            enq_pt.set_cycle_bit(udc_ep.pcs);
            udc_ep.enq_pt = udc_ep.first_trb;
            udc_ep.pcs ^= 1;
        }
        self.knock_doorbell(0);
    }

    // knock door bell then controller will start transfer for the specific endpoint
    // pei: physical endpoint index
    pub fn knock_doorbell(&mut self, pei: u32) { self.csr.wfo(DOORBELL_TARGET, pei); }

    pub fn ccs(&self) -> bool { self.csr.rf(PORTSC_CCS) != 0 }

    pub fn print_status(&self) {
        let status = self.csr.r(PORTSC);
        let bitflags = [
            (0u32, "CCS"),
            (3u32, "PP"),
            (4u32, "PR"),
            (16u32, "LWS"),
            (17u32, "CSC"),
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

        self.print_status();

        self.set_addr(0, 0);
    }
}

// TODO: migrate this to a separate file
#[allow(dead_code)]
pub mod corigine_usb {
    use utralib::{Field, Register};

    pub const DEVCAP: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0, 0xffffffff);
    pub const DEVCAP_VESION: Field = Field::new(8, 0, DEVCAP);
    pub const DEVCAP_EP_IN: Field = Field::new(4, 8, DEVCAP);
    pub const DEVCAP_EP_OUT: Field = Field::new(4, 12, DEVCAP);
    pub const DEVCAP_MAX_INTS: Field = Field::new(10, 16, DEVCAP);
    pub const DEVCAP_GEN1: Field = Field::new(1, 27, DEVCAP);
    pub const DEVCAP_GEN2: Field = Field::new(1, 28, DEVCAP);
    pub const DEVCAP_ISOCH: Field = Field::new(1, 29, DEVCAP);

    pub const DEVCONFIG: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x10 / 4, 0xFF);
    pub const DEVCONFIG_MAX_SPEED: Field = Field::new(4, 0, DEVCONFIG);
    pub const DEVCONFIG_USB3_DISABLE_COUNT: Field = Field::new(4, 4, DEVCONFIG);

    pub const EVENTCONFIG: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x14 / 4, 0xFFFF_FFFF);
    pub const EVENTCONFIG_CSC_ENABLE: Field = Field::new(1, 0, EVENTCONFIG);
    pub const EVENTCONFIG_PEC_ENABLE: Field = Field::new(1, 1, EVENTCONFIG);
    pub const EVENTCONFIG_PPC_ENABLE: Field = Field::new(1, 3, EVENTCONFIG);
    pub const EVENTCONFIG_PRC_ENABLE: Field = Field::new(1, 4, EVENTCONFIG);
    pub const EVENTCONFIG_PLC_ENABLE: Field = Field::new(1, 5, EVENTCONFIG);
    pub const EVENTCONFIG_CEC_ENABLE: Field = Field::new(1, 6, EVENTCONFIG);
    pub const EVENTCONFIG_U3_PLC_ENABLE: Field = Field::new(1, 8, EVENTCONFIG);
    pub const EVENTCONFIG_L1_PLC_ENABLE: Field = Field::new(1, 9, EVENTCONFIG);
    pub const EVENTCONFIG_U3_RESUME_PLC_ENABLE: Field = Field::new(1, 10, EVENTCONFIG);
    pub const EVENTCONFIG_L1_RESUME_PLC_ENABLE: Field = Field::new(1, 11, EVENTCONFIG);
    pub const EVENTCONFIG_INACTIVE_PLC_ENABLE: Field = Field::new(1, 12, EVENTCONFIG);
    pub const EVENTCONFIG_USB3_RESUME_NO_PLC_ENABLE: Field = Field::new(1, 13, EVENTCONFIG);
    pub const EVENTCONFIG_USB2_RESUME_NO_PLC_ENABLE: Field = Field::new(1, 14, EVENTCONFIG);
    pub const EVENTCONFIG_SETUP_ENABLE: Field = Field::new(1, 16, EVENTCONFIG);
    pub const EVENTCONFIG_STOPPED_LENGTH_INVALID_ENABLE: Field = Field::new(1, 17, EVENTCONFIG);
    pub const EVENTCONFIG_HALTED_LENGTH_INVALID_ENABLE: Field = Field::new(1, 18, EVENTCONFIG);
    pub const EVENTCONFIG_DISABLED_LENGTH_INVALID_ENABLE: Field = Field::new(1, 19, EVENTCONFIG);
    pub const EVENTCONFIG_DISABLE_EVENT_ENABLE: Field = Field::new(1, 20, EVENTCONFIG);

    pub const USBCMD: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x20 / 4, 0xFFFF_FFFF);
    pub const USBCMD_RUN_STOP: Field = Field::new(1, 0, USBCMD);
    pub const USBCMD_SOFT_RESET: Field = Field::new(1, 1, USBCMD);
    pub const USBCMD_INT_ENABLE: Field = Field::new(1, 2, USBCMD);
    pub const USBCMD_SYS_ERR_ENABLE: Field = Field::new(1, 3, USBCMD);
    pub const USBCMD_EWE: Field = Field::new(1, 10, USBCMD);
    pub const USBCMD_FORCE_TERMINATION: Field = Field::new(1, 11, USBCMD);

    pub const USBSTS: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x24 / 4, 0xFFFF_FFFF);
    pub const USBSTS_CTL_HALTED: Field = Field::new(1, 0, USBSTS);
    pub const USBSTS_SYSTEM_ERR: Field = Field::new(1, 2, USBSTS);
    pub const USBSTS_EINT: Field = Field::new(1, 3, USBSTS);
    pub const USBSTS_CTL_IDLE: Field = Field::new(1, 12, USBSTS);

    pub const DCBAPLO: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x28 / 4, 0xFFFF_FFFF);
    pub const DBCAPLO_PTR_LO: Field = Field::new(26, 6, DCBAPLO);

    pub const DCBAPHI: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x2C / 4, 0xFFFF_FFFF);
    pub const DBCAPLO_PTR_HI: Field = Field::new(32, 0, DCBAPHI);

    pub const PORTSC: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x30 / 4, 0xFFFF_FFFF);
    pub const PORTSC_CCS: Field = Field::new(1, 0, PORTSC);
    pub const PORTSC_PP: Field = Field::new(1, 3, PORTSC);
    pub const PORTSC_PR: Field = Field::new(1, 4, PORTSC);
    pub const PORTSC_PLS: Field = Field::new(4, 5, PORTSC);
    pub const PORTSC_SPEED: Field = Field::new(4, 10, PORTSC);
    pub const PORTSC_LWS: Field = Field::new(1, 16, PORTSC);
    pub const PORTSC_CSC: Field = Field::new(1, 17, PORTSC);
    pub const PORTSC_PPC: Field = Field::new(1, 20, PORTSC);
    pub const PORTSC_PRC: Field = Field::new(1, 21, PORTSC);
    pub const PORTSC_PLC: Field = Field::new(1, 22, PORTSC);
    pub const PORTSC_CEC: Field = Field::new(1, 23, PORTSC);
    pub const PORTSC_WCE: Field = Field::new(1, 25, PORTSC);
    pub const PORTSC_WDE: Field = Field::new(1, 26, PORTSC);
    pub const PORTSC_WPR: Field = Field::new(1, 31, PORTSC);

    // pub const U3PORTPMSC: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x34 / 4, 0xFFFF_FFFF);

    // pub const U2PORTPMSC: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x38 / 4, 0xFFFF_FFFF);

    // pub const U3PORTLI: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x3C / 4, 0xFFFF_FFFF);

    pub const DOORBELL: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x40 / 4, 0xFFFF_FFFF);
    pub const DOORBELL_TARGET: Field = Field::new(5, 0, DOORBELL);

    pub const MFINDEX: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x44 / 4, 0xFFFF_FFFF);
    pub const MFINDEX_SYNC_EN: Field = Field::new(1, 0, MFINDEX);
    pub const MFINDEX_OUT_OF_SYNC_EN: Field = Field::new(1, 1, MFINDEX);
    pub const MFINDEX_IN_SYNC_EN: Field = Field::new(1, 2, MFINDEX);
    pub const MFINDEX_INDEX_OUT_OF_SYNC_EN: Field = Field::new(1, 3, MFINDEX);
    pub const MFINDEX_MFINDEX_EN: Field = Field::new(14, 4, MFINDEX);
    pub const MFINDEX_MFOFFSET_EN: Field = Field::new(13, 18, MFINDEX);

    pub const PTMCTRL: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x48 / 4, 0xFFFF_FFFF);
    pub const PTMCTRL_DELAY: Field = Field::new(14, 0, PTMCTRL);

    pub const PTMSTS: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x4C / 4, 0xFFFF_FFFF);
    pub const PTMSTS_MFINDEX_IN_SYNC: Field = Field::new(1, 2, PTMSTS);
    pub const PTMSTS_MFINDEX: Field = Field::new(14, 4, PTMSTS);
    pub const PTMSTS_MFOFFSET: Field = Field::new(13, 18, PTMSTS);

    pub const EPENABLE: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x60 / 4, 0xFFFF_FFFF);
    pub const EPENABLE_ENABLED: Field = Field::new(30, 2, EPENABLE);

    pub const EPRUNNING: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x64 / 4, 0xFFFF_FFFF);
    pub const EPRUNNING_RUNNING: Field = Field::new(30, 2, EPRUNNING);

    pub const CMDPARA0: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x70 / 4, 0xFFFF_FFFF);
    pub const CMDPARA0_CMD0_INIT_EP0_DQPTRLO: Field = Field::new(28, 4, CMDPARA0);
    pub const CMDPARA0_CMD0_INIT_EP0_DCS: Field = Field::new(1, 0, CMDPARA0);
    pub const CMDPARA0_CMD1_UPDATE_EP0_MPS: Field = Field::new(16, 16, CMDPARA0);
    pub const CMDPARA0_CMD2_SET_ADDR: Field = Field::new(8, 0, CMDPARA0);

    pub const CMDPARA1: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x74 / 4, 0xFFFF_FFFF);

    pub const CMDCTRL: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x78 / 4, 0xFFFF_FFFF);
    pub const CMDCTRL_ACTIVE: Field = Field::new(1, 0, CMDCTRL);
    pub const CMDCTRL_IOC: Field = Field::new(1, 1, CMDCTRL);
    pub const CMDCTRL_TYPE: Field = Field::new(4, 4, CMDCTRL);
    pub const CMDCTRL_STATUS: Field = Field::new(4, 16, CMDCTRL);

    pub const ODBCAP: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x80 / 4, 0xFFFF_FFFF);
    pub const OBDCAP_RAM_SIZE: Field = Field::new(11, 0, ODBCAP);

    pub const ODBCONFIG0: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x90 / 4, 0xFFFF_FFFF);
    pub const ODBCONFIG0_EP0_OFFSET: Field = Field::new(10, 0, ODBCONFIG0);
    pub const ODBCONFIG0_EP0_SIZE: Field = Field::new(3, 10, ODBCONFIG0);
    pub const ODBCONFIG0_EP1_OFFSET: Field = Field::new(10, 16, ODBCONFIG0);
    pub const ODBCONFIG0_EP1_SIZE: Field = Field::new(3, 26, ODBCONFIG0);

    pub const ODBCONFIG1: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x94 / 4, 0xFFFF_FFFF);
    pub const ODBCONFIG1_EP2_OFFSET: Field = Field::new(10, 0, ODBCONFIG1);
    pub const ODBCONFIG1_EP2_SIZE: Field = Field::new(3, 10, ODBCONFIG1);
    pub const ODBCONFIG1_EP3_OFFSET: Field = Field::new(10, 16, ODBCONFIG1);
    pub const ODBCONFIG1_EP3_SIZE: Field = Field::new(3, 26, ODBCONFIG1);

    pub const ODBCONFIG2: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x98 / 4, 0xFFFF_FFFF);
    pub const ODBCONFIG2_EP4_OFFSET: Field = Field::new(10, 0, ODBCONFIG2);
    pub const ODBCONFIG2_EP4_SIZE: Field = Field::new(3, 10, ODBCONFIG2);
    pub const ODBCONFIG2_EP5_OFFSET: Field = Field::new(10, 16, ODBCONFIG2);
    pub const ODBCONFIG2_EP5_SIZE: Field = Field::new(3, 26, ODBCONFIG2);

    pub const ODBCONFIG3: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x9C / 4, 0xFFFF_FFFF);
    pub const ODBCONFIG3_EP6_OFFSET: Field = Field::new(10, 0, ODBCONFIG3);
    pub const ODBCONFIG3_EP6_SIZE: Field = Field::new(3, 10, ODBCONFIG3);
    pub const ODBCONFIG3_EP7_OFFSET: Field = Field::new(10, 16, ODBCONFIG3);
    pub const ODBCONFIG3_EP7_SIZE: Field = Field::new(3, 26, ODBCONFIG3);

    pub const ODBCONFIG4: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xA0 / 4, 0xFFFF_FFFF);
    pub const ODBCONFIG4_EP8_OFFSET: Field = Field::new(10, 0, ODBCONFIG4);
    pub const ODBCONFIG4_EP8_SIZE: Field = Field::new(3, 10, ODBCONFIG4);
    pub const ODBCONFIG4_EP9_OFFSET: Field = Field::new(10, 16, ODBCONFIG4);
    pub const ODBCONFIG4_EP9_SIZE: Field = Field::new(3, 26, ODBCONFIG4);

    pub const ODBCONFIG5: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xA4 / 4, 0xFFFF_FFFF);
    pub const ODBCONFIG5_EP10_OFFSET: Field = Field::new(10, 0, ODBCONFIG5);
    pub const ODBCONFIG5_EP10_SIZE: Field = Field::new(3, 10, ODBCONFIG5);
    pub const ODBCONFIG5_EP11_OFFSET: Field = Field::new(10, 16, ODBCONFIG5);
    pub const ODBCONFIG5_EP11_SIZE: Field = Field::new(3, 26, ODBCONFIG5);

    pub const ODBCONFIG6: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xA8 / 4, 0xFFFF_FFFF);
    pub const ODBCONFIG6_EP12_OFFSET: Field = Field::new(10, 0, ODBCONFIG6);
    pub const ODBCONFIG6_EP12_SIZE: Field = Field::new(3, 10, ODBCONFIG6);
    pub const ODBCONFIG6_EP13_OFFSET: Field = Field::new(10, 16, ODBCONFIG6);
    pub const ODBCONFIG6_EP13_SIZE: Field = Field::new(3, 26, ODBCONFIG6);

    pub const ODBCONFIG7: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xAC / 4, 0xFFFF_FFFF);
    pub const ODBCONFIG7_EP14_OFFSET: Field = Field::new(10, 0, ODBCONFIG7);
    pub const ODBCONFIG7_EP14_SIZE: Field = Field::new(3, 10, ODBCONFIG7);
    pub const ODBCONFIG7_EP15_OFFSET: Field = Field::new(10, 16, ODBCONFIG7);
    pub const ODBCONFIG7_EP15_SIZE: Field = Field::new(3, 26, ODBCONFIG7);

    pub const DEBUG0: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0xB0 / 4, 0xFFFF_FFFF);
    pub const DEBUG0_DEV_ADDR: Field = Field::new(7, 0, DEBUG0);
    pub const DEBUG0_NUMP_LIMIT: Field = Field::new(4, 8, DEBUG0);

    pub const IMAN: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x100 / 4, 0xFFFF_FFFF);
    pub const IMAN_IP: Field = Field::new(1, 0, IMAN);
    pub const IMAN_IE: Field = Field::new(1, 1, IMAN);

    pub const IMOD: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x104 / 4, 0xFFFF_FFFF);
    pub const IMOD_MOD_INTERVAL: Field = Field::new(16, 0, IMOD);
    pub const IMOD_MOD_COUNTER: Field = Field::new(16, 32, IMOD);

    pub const ERSTSZ: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x108 / 4, 0xFFFF_FFFF);
    pub const ERSTSZ_RING_SEG_TABLE: Field = Field::new(16, 0, ERSTSZ);

    pub const ERSTBALO: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x110 / 4, 0xFFFF_FFFF);
    pub const ERSTBAL0_BASE_ADDR_LO: Field = Field::new(26, 6, ERSTBALO);

    pub const ERSTBAHI: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x114 / 4, 0xFFFF_FFFF);
    pub const ERSTBAHI_BASE_ADDR_HI: Field = Field::new(32, 0, ERSTBAHI);

    pub const ERDPLO: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x118 / 4, 0xFFFF_FFFF);
    pub const ERDPLO_DESI: Field = Field::new(3, 0, ERDPLO);
    pub const ERDPLO_EHB: Field = Field::new(1, 3, ERDPLO);
    pub const ERDPLO_DQ_PTR: Field = Field::new(28, 4, ERDPLO);

    pub const ERDPHI: Register = Register::new(CORIGINE_DEV_OFFSET / 4 + 0x11C / 4, 0xFFFF_FFFF);
    pub const ERDPHI_DQ_PTR: Field = Field::new(32, 0, ERDPHI);

    pub const CORIGINE_EVENT_RING_NUM: usize = 1;
    pub const CORIGINE_USB_BASE: usize = 0x5020_2000;
    pub const CORIGINE_DEV_OFFSET: usize = 0x400;
    pub const CORIGINE_UICR_OFFSET: usize = CORIGINE_DEV_OFFSET + 0x100;
    pub const CORIGINE_USB_LEN: usize = 0x3000;
}
