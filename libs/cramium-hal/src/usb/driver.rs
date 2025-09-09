use core::convert::TryFrom;
use core::mem::size_of;
#[cfg(feature = "std")]
use core::sync::atomic::AtomicBool;
use core::sync::atomic::{AtomicPtr, Ordering, compiler_fence};
#[cfg(feature = "std")]
use std::sync::{Arc, Mutex};

use bitfield::bitfield;
#[cfg(feature = "std")]
use usb_device::bus::PollResult;
#[cfg(feature = "std")]
use usb_device::{Result, UsbDirection, class_prelude::*};
#[cfg(feature = "std")]
use utralib::generated::*;

#[cfg(not(feature = "std"))]
use super::compat::AtomicCsr;
#[cfg(not(feature = "std"))]
use crate::print;
use crate::println;
use crate::usb::utra::*;

// Directional nomenclature.
//
// Manual says "outbound" means outbound packets from device going to host. This is an IN packet in USB.
// "inbound" means inbound packets to device, coming from host. This is the OUT packet in USB.
pub const USB_SEND: bool = false; // USB IN -> corigine USB_SEND (outbound) -> 0/even on PEI
pub const USB_RECV: bool = true; // USB OUT -> corigine USB_RECV (inbound) -> 1/odd on PEI
// these names are picked so that the boolean value maps to the same thing in the reference code
// this effectively does the "reversal of direction" from USB spec to corigine speak
pub const CRG_IN: bool = false;
pub const CRG_OUT: bool = true;

const CRG_EVENT_RING_NUM: usize = 1;
const CRG_ERST_SIZE: usize = 1;
const CRG_EVENT_RING_SIZE: usize = 32;
const CRG_EP0_TD_RING_SIZE: usize = 16;
pub const CRG_EP_NUM: usize = 8;
const CRG_TD_RING_SIZE: usize = 64; // was 1280 in original code. not even sure we need ... 64?
const CRG_UDC_MAX_BURST: u32 = 15;
const CRG_UDC_ISO_INTERVAL: u8 = 3;

pub const CRG_INT_TARGET: u32 = 0;

/// allocate 0x100 bytes for event ring segment table, each table 0x40 bytes
const CRG_UDC_ERSTSIZE: usize = 0x100;
/// allocate 0x200 for one event ring, include 128 event TRBs , each TRB 16 bytes
const CRG_UDC_EVENTRINGSIZE: usize = CRG_EVENT_RING_SIZE * size_of::<EventTrbS>() * CRG_EVENT_RING_NUM;
/// allocate 0x200 for ep context, include 30 ep context, each ep context 16 bytes
const CRG_UDC_EPCXSIZE: usize = 0x200;
/// allocate 0x400 for EP0 transfer ring, include 64 transfer TRBs, each TRB 16 bytes (this doesn't line up, I
/// think we have 16 * 16)
const CRG_UDC_EP0_TRSIZE: usize = 0x100;
/// 1280(TRB Num) * 4(EP NUM) * 16(TRB bytes)  // * 2 because we need one for each direction??
const CRG_UDC_EP_TRSIZE: usize = CRG_TD_RING_SIZE * CRG_EP_NUM * 2 * size_of::<TransferTrbS>();
/// allocate 0x400 bytes for EP0 Buffer, Normally EP0 TRB transfer length will not greater than 1K
pub const CRG_UDC_EP0_REQBUFSIZE: usize = 256;
pub const CRG_UDC_APP_BUF_LEN: usize = 512;
pub const CRG_UDC_APP_BUFSIZE: usize = CRG_EP_NUM * 2 * CRG_UDC_APP_BUF_LEN;

pub const CRG_IFRAM_PAGES: usize = 8;
pub const CRG_UDC_MEMBASE: usize =
    utralib::HW_IFRAM1_MEM + utralib::HW_IFRAM1_MEM_LEN - CRG_IFRAM_PAGES * 0x1000;

const CRG_UDC_ERST_OFFSET: usize = 0; // use relative offsets
const CRG_UDC_EVENTRING_OFFSET: usize = CRG_UDC_ERST_OFFSET + CRG_UDC_ERSTSIZE;
const CRG_UDC_EPCX_OFFSET: usize = CRG_UDC_EVENTRING_OFFSET + CRG_UDC_EVENTRINGSIZE;

pub const CRG_UDC_EP0_TR_OFFSET: usize = CRG_UDC_EPCX_OFFSET + CRG_UDC_EPCXSIZE;
pub const CRG_UDC_EP_TR_OFFSET: usize = CRG_UDC_EP0_TR_OFFSET + CRG_UDC_EP0_TRSIZE;
pub const CRG_UDC_EP0_BUF_OFFSET: usize = CRG_UDC_EP_TR_OFFSET + CRG_UDC_EP_TRSIZE;
pub const CRG_UDC_APP_BUFOFFSET: usize = CRG_UDC_EP0_BUF_OFFSET + CRG_UDC_EP0_REQBUFSIZE;
pub const CRG_UDC_TOTAL_MEM_LEN: usize = CRG_UDC_APP_BUFOFFSET + CRG_UDC_APP_BUFSIZE;

const MAX_TRB_XFER_LEN: usize = 1024;

/* usb transfer flags */
pub const CRG_XFER_NO_INTR: u8 = 1 << 0; //no interrupt after this transfer
pub const CRG_XFER_NO_DB: u8 = 1 << 1; //will not knock doorbell
pub const CRG_XFER_SET_CHAIN: u8 = 1 << 2; //set chain bit at the last trb in this transfer
//#define CRG_XFER_ISOC_ASAP		1 << 3	//isoc as soon as possible
pub const CRG_XFER_AZP: u8 = 1 << 4; //append zero length packet after a max packet

#[cfg(feature = "std")]
static INTERRUPT_INIT_DONE: AtomicBool = AtomicBool::new(false);

/*
#[cfg(feature = "std")]
fn handle_usb(_irq_no: usize, arg: *mut usize) {
    let usb = unsafe { &mut *(arg as *mut CorigineUsb) };
    let pending = usb.irq_csr.r(utralib::utra::irqarray1::EV_PENDING);

    // actual interrupt handling is done in userspace, this just triggers the routine
    usb.irq_csr.wo(utralib::utra::irqarray1::EV_ENABLE, 0);

    usb.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, pending);

    xous::try_send_message(usb.conn, xous::Message::new_scalar(usb.opcode, 0, 0, 0, 0)).ok();
}
*/

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

#[derive(Debug, Eq, PartialEq)]
pub enum EpState {
    Disabled = 0,
    Running,
    Halted,
    Stopped,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone)]
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
impl TryFrom<u32> for CmdType {
    type Error = Error;

    fn try_from(value: u32) -> core::result::Result<Self, Error> {
        match value {
            0 => Ok(CmdType::InitEp0),
            1 => Ok(CmdType::UpdateEp0),
            2 => Ok(CmdType::SetAddr),
            3 => Ok(CmdType::SendDevNotification),
            4 => Ok(CmdType::ConfigEp),
            5 => Ok(CmdType::SetHalt),
            6 => Ok(CmdType::ClearHalt),
            7 => Ok(CmdType::ResetSeqNum),
            8 => Ok(CmdType::StopEp),
            9 => Ok(CmdType::SetTrDqPtr),
            10 => Ok(CmdType::ForceFlowControl),
            11 => Ok(CmdType::ReqLdmExchange),
            _ => Err(Error::InvalidState),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
/// This structure is a little bit f'd up. It's a direct copy from the reference
/// driver, where they conflate the endpoint type `enum` with direction by using
/// `Invalid2` as a value we can add to the `enum`. The reference driver liberally
/// (ab)uses this motif. In order to make initial code porting easier, we adopt their
/// method, but eventually this tech debt should be cleaned up.
pub enum EpType {
    ControlOrInvalid = 0,
    IsochOutbound = 1,
    BulkOutbound = 2,
    IntrOutbound = 3,
    Invalid2 = 4,
    IsochInbound = 5,
    BulkInbound = 6,
    IntrInbound = 7,
}
impl TryFrom<u8> for EpType {
    type Error = Error;

    fn try_from(value: u8) -> core::result::Result<Self, Error> {
        match value {
            0 => Ok(EpType::ControlOrInvalid),
            1 => Ok(EpType::IsochOutbound),
            2 => Ok(EpType::BulkOutbound),
            3 => Ok(EpType::IntrOutbound),
            4 => Ok(EpType::Invalid2),
            5 => Ok(EpType::IsochInbound),
            6 => Ok(EpType::BulkInbound),
            7 => Ok(EpType::IntrInbound),
            _ => Err(Error::InvalidState),
        }
    }
}

bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq, Default)]
    pub struct PortSc(u32);
    impl Debug;
    pub ccs, set_ccs: 0;
    pub pp, set_pp: 3;
    pub pr, set_pr: 4;
    pub pls, set_pl: 8, 5;
    pub speed, set_speed: 13, 10;
    pub lws, set_lws: 16;
    pub csc, set_csc: 17;
    pub ppc, set_ppc: 20;
    pub prc, set_prc: 21;
    pub plc, set_plc: 22;
    pub cec, set_cec: 23;
    pub wce, set_wce: 25;
    pub wde, set_wde: 26;
    pub wdr, set_wdr: 31;
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

const CRG_UDC_CFG0_MAXSPEED_FS: u32 = 1;
// surprisingly, just swapping this constant in "sort of works"
// some to-do around figuring out why the protocol breaks, could
// just be signal integrity because too many connectors, but very
// likely this is also an inability to handle the longer packet
// sizes mandated by the HS protocol.
//
// leave as a warning so we have this as a TODO.
const CRG_UDC_CFG0_MAXSPEED_HS: u32 = 3;

pub const CRG_UDC_ERDPLO_EHB: u32 = 1 << 3;

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

    fn try_from(code: u32) -> core::result::Result<Self, Error> {
        match code {
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

/// We make our own custom event type because PollResult doesn't have debug, eq, etc...
#[derive(Debug, PartialEq, Eq)]
pub enum CrgEvent {
    None,
    Connect,
    /// out, in_complete, setup
    Data(u16, u16, u16),
    Error,
}
bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq, Default)]
    pub struct ControlTrbDw2(u32);
    impl Debug;
    pub transfer_len, set_transfer_len: 17, 0;
    pub td_size, set_td_size: 21, 18;
    pub intr_target, set_intr_target: 31, 22;
}

bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq, Default)]
    pub struct ControlTrbDw3(u32);
    impl Debug;
    pub u8, cycle_bit, set_cycle_bit: 0;
    pub link_toggle_cycle, set_link_toggle_cycle: 1;
    pub intr_on_short_pkt, set_intr_on_short_pkt: 2;
    pub no_snoop, set_no_snoop: 3, 3;
    pub trb_chain, set_trb_chain: 4;
    pub intr_on_completion, set_intr_on_completion: 5;
    pub append_zlp, set_append_zlp: 7;
    pub block_event_int, set_block_event_int: 9, 9;
    pub trb_type, set_trb_type: 15, 10;
    pub dir, set_dir: 16;
    pub u8, setup_tag, set_setup_tag: 18, 17;
    pub status_stage_trb_stall, set_status_stage_trb_stall: 19;
    pub status_stage_set_addr, set_status_stage_set_addr: 20;
    pub u16, isoc_trb_frame_id, set_isoc_trb_frame_id: 30, 20;
    pub isoc_trb_sia, set_isoc_trb_sia: 31;
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct TransferTrbS {
    pub dplo: u32,
    pub dphi: u32,
    pub dw2: ControlTrbDw2,
    pub dw3: ControlTrbDw3,
}
// see 8.6.3 if debug visibility is necessary
impl TransferTrbS {
    pub fn zeroize(&mut self) {
        self.dplo = 0;
        self.dphi = 0;
        self.dw2 = ControlTrbDw2(0);
        self.dw3 = ControlTrbDw3(0);
    }

    pub fn get_trb_type(&self) -> TrbType {
        TrbType::try_from(self.dw3.trb_type()).expect("Unknown TRB type")
    }

    /// Implicitly sets the link type to Link
    pub fn setup_link_trb(&mut self, toggle: bool, next_trb: *mut TransferTrbS) {
        self.dplo = next_trb as usize as u32 & 0xFFFF_FFF0;
        self.dphi = 0;
        self.dw2 = ControlTrbDw2(0);

        self.dw3 = ControlTrbDw3(0);
        self.dw3.set_trb_type(TrbType::Link as u32);
        self.dw3.set_link_toggle_cycle(toggle);

        compiler_fence(Ordering::SeqCst);
    }

    /// Implicitly sets link type to Status
    pub fn control_status_trb(
        &mut self,
        pcs: bool,
        set_addr: bool,
        stall: bool,
        tag: u8,
        intr_target: u32,
        dir: bool,
    ) {
        self.dw2 = ControlTrbDw2(0);
        self.dw2.set_intr_target(intr_target);

        self.dw3 = ControlTrbDw3(0);
        self.dw3.set_cycle_bit(pcs);
        self.dw3.set_intr_on_completion(true);
        self.dw3.set_trb_type(TrbType::StatusStage as u32);

        self.dw3.set_dir(dir);

        self.dw3.set_setup_tag(tag);
        self.dw3.set_status_stage_trb_stall(stall);
        self.dw3.set_status_stage_set_addr(set_addr);
        /*
        self.dw3 = (self.dw3 & !0x1) | (pcs & 1) as u32; // CYCLE_BIT
        self.dw3 = self.dw3 | 0x20; // set INTR_ON_COMPLETION
        self.dw3 = (self.dw3 & !0x1_0000) | if dir { 1 << 16 } else { 0 }; // DIR_MASK
        self.dw3 = (self.dw3 & !0x00060000) | ((tag as u32 & 0x3) << 17); // SETUP_TAG
        self.dw3 = (self.dw3 & !0x00080000) | if stall { 1 << 19 } else { 0 }; // STATUS_STAGE_TRB_STALL
        self.dw3 = (self.dw3 & !0x00100000) | if set_addr { 1 << 20 } else { 0 }; // STATUS_STAGE_TRB_SET_ADDR
        */
        compiler_fence(Ordering::SeqCst);
    }

    pub fn control_data_trb(
        &mut self,
        dma: u32,
        pcs: bool,
        _num_trb: u32,
        transfer_length: u32,
        td_size: u32,
        ioc: bool,
        azp: bool,
        dir: bool,
        setup_tag: u8,
        intr_target: u32,
    ) {
        self.dplo = dma;
        self.dphi = 0;

        self.dw2 = ControlTrbDw2(0);
        self.dw2.set_transfer_len(transfer_length);
        self.dw2.set_td_size(td_size);
        self.dw2.set_intr_target(intr_target);

        self.dw3 = ControlTrbDw3(0);
        self.dw3.set_cycle_bit(pcs);
        self.dw3.set_intr_on_short_pkt(true);
        self.dw3.set_intr_on_completion(ioc);
        self.dw3.set_trb_type(TrbType::DataStage as u32);
        self.dw3.set_append_zlp(azp);
        self.dw3.set_dir(dir);
        self.dw3.set_setup_tag(setup_tag);
        compiler_fence(Ordering::SeqCst);
    }

    pub fn prepare_transfer_trb(
        &mut self,
        xfer_len: usize,
        xfer_buf_addr: usize,
        td_size: u32,
        pcs: bool,
        trb_type: TrbType,
        short_pkt: bool,
        chain_bit: bool,
        intr_on_compl: bool,
        b_setup_stage: bool,
        usb_dir: bool,
        b_isoc: bool,
        _tlb_pc: u8,
        frame_i_d: u16,
        sia: bool,
        azp: bool,
        intr_target: u32,
    ) {
        self.dplo = xfer_buf_addr as u32;
        self.dphi = 0;

        self.dw2 = ControlTrbDw2(0);
        self.dw2.set_transfer_len(xfer_len as u32);
        self.dw2.set_td_size(td_size);
        self.dw2.set_intr_target(intr_target);

        self.dw3 = ControlTrbDw3(0);
        self.dw3.set_cycle_bit(pcs);
        self.dw3.set_intr_on_short_pkt(short_pkt);
        self.dw3.set_trb_chain(chain_bit);
        self.dw3.set_intr_on_completion(intr_on_compl);
        self.dw3.set_append_zlp(azp);
        self.dw3.set_trb_type(trb_type as u32);

        if b_setup_stage {
            self.dw3.set_dir(usb_dir);
        }
        if b_isoc {
            self.dw3.set_isoc_trb_frame_id(frame_i_d);
            self.dw3.set_isoc_trb_sia(sia);
        }
        compiler_fence(Ordering::SeqCst);
    }
}

bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq, Default)]
    pub struct EpCxDw0(u32);
    impl Debug;
    pub u8, ep_num, set_ep_num: 6, 3;
    pub u8, interval, set_interval: 23, 16;
}
bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq, Default)]
    pub struct EpCxDw1(u32);
    impl Debug;
    pub ep_type, set_ep_type: 5, 3;
    pub max_burst_size, set_max_burst_size: 15, 8;
    pub u16, max_packet_size, set_max_packet_size: 31, 16;
}
bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq, Default)]
    pub struct EpCxDw2(u32);
    impl Debug;
    pub deq_cyc_state, set_deq_cyc_state: 0;
    pub deq_ptr_lo, set_deq_ptr_lo: 31, 4;
}
#[repr(C)]
#[derive(Default, Debug)]
pub struct EpCxS {
    dw0: EpCxDw0,
    dw1: EpCxDw1,
    dw2: EpCxDw2,
    dw3: u32,
}
impl EpCxS {
    pub fn epcx_setup(&mut self, udc_ep: &UdcEp) {
        // corigine gadget dir should be opposite to host dir
        let ep_type = if udc_ep.direction == USB_RECV {
            // transforms the base type into INBOUND
            EpType::try_from(udc_ep.ep_type as u8 + EpType::Invalid2 as u8).unwrap()
        } else {
            // leave as base type which is OUTBOUND
            udc_ep.ep_type
        };
        #[cfg(feature = "std")]
        crate::println!("final HW EpType: {:?}", ep_type);
        let max_size = udc_ep.max_packet_size & 0x7FF;

        self.dw0 = EpCxDw0(0);
        self.dw0.set_ep_num(udc_ep.ep_num);
        if udc_ep.ep_type == EpType::IsochOutbound || udc_ep.ep_type == EpType::IntrOutbound {
            self.dw0.set_interval(CRG_UDC_ISO_INTERVAL);
        } else {
            self.dw0.set_interval(0);
        }

        self.dw1 = EpCxDw1(0);
        self.dw1.set_ep_type(ep_type as u32);
        self.dw1.set_max_packet_size(max_size);
        self.dw1.set_max_burst_size(CRG_UDC_MAX_BURST);

        self.dw2 = EpCxDw2(0);
        self.dw2.set_deq_ptr_lo(udc_ep.tran_ring_info.dma as u32 >> 4); // this gets shifted << 4, which effectively masks lower 4 bits
        self.dw2.set_deq_cyc_state(udc_ep.pcs);

        self.dw3 = 0;
        compiler_fence(Ordering::SeqCst);
    }
}

bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq, Default)]
    pub struct EventTrbDw2(u32);
    impl Debug;
    pub trb_tran_len, set_trb_tran_len: 16, 0;
    pub compl_code, set_compl_code: 31, 24;
}
bitfield! {
    #[derive(Copy, Clone, PartialEq, Eq, Default)]
    pub struct EventTrbDw3(u32);
    impl Debug;
    pub cycle_bit, set_cycle_bit: 0;
    pub trb_type, set_trb_type: 15, 10;
    pub endpoint_id, set_endpoint_id: 20, 16;
    pub setup_tag, set_setup_tag: 22, 21;
}

#[repr(C)]
#[derive(Default, Debug)]
pub struct EventTrbS {
    pub dw0: u32,
    pub dw1: u32,
    pub dw2: EventTrbDw2,
    pub dw3: EventTrbDw3,
}
impl EventTrbS {
    pub fn zeroize(&mut self) {
        self.dw0 = 0;
        self.dw1 = 0;
        self.dw2 = EventTrbDw2(0);
        self.dw3 = EventTrbDw3(0);
    }

    pub fn get_cycle_bit(&self) -> bool { self.dw3.cycle_bit() }

    pub fn get_endpoint_id(&self) -> u8 { self.dw3.endpoint_id() as u8 }

    pub fn get_trb_type(&self) -> TrbType {
        let trb_type = self.dw3.trb_type();
        TrbType::try_from(trb_type).unwrap_or(TrbType::Rsvd)
    }

    pub fn get_raw_setup(&self) -> [u8; 8] {
        let mut ret = [0u8; 8];
        ret[..4].copy_from_slice(&self.dw0.to_le_bytes());
        ret[4..].copy_from_slice(&self.dw1.to_le_bytes());
        ret
    }

    pub fn get_setup_tag(&self) -> u8 { self.dw3.setup_tag() as u8 }
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
    pub vaddr: AtomicPtr<u8>,
    pub dma: u64,
    pub len: usize,
}
impl Default for BufferInfo {
    fn default() -> Self { Self { vaddr: AtomicPtr::new(core::ptr::null_mut()), dma: 0, len: 0 } }
}

pub struct UdcEp {
    // Endpoint number
    ep_num: u8,
    // Endpoint direction
    direction: bool,
    ep_type: EpType,
    max_packet_size: u16,
    tran_ring_info: BufferInfo,
    pub first_trb: AtomicPtr<TransferTrbS>,
    last_trb: AtomicPtr<TransferTrbS>,
    pub enq_pt: AtomicPtr<TransferTrbS>,
    pub deq_pt: AtomicPtr<TransferTrbS>,
    pub pcs: bool,
    tran_ring_full: bool,
    ep_state: EpState,
    _wedge: bool,
    pub completion_handler: Option<fn(&mut CorigineUsb, usize, u32, u8)>,
}
impl Default for UdcEp {
    fn default() -> Self {
        Self {
            ep_num: 0,
            direction: USB_RECV,
            ep_type: EpType::ControlOrInvalid,
            max_packet_size: 0,
            tran_ring_info: BufferInfo::default(),
            first_trb: AtomicPtr::new(core::ptr::null_mut()),
            last_trb: AtomicPtr::new(core::ptr::null_mut()),
            enq_pt: AtomicPtr::new(core::ptr::null_mut()),
            deq_pt: AtomicPtr::new(core::ptr::null_mut()),
            pcs: true,
            tran_ring_full: false,
            ep_state: EpState::Disabled,
            _wedge: false,
            completion_handler: None,
        }
    }
}
impl UdcEp {
    pub fn increment_enq_pt(&mut self) -> (&mut TransferTrbS, bool) {
        unsafe {
            // increment to the next record
            self.enq_pt = AtomicPtr::new(self.enq_pt.load(Ordering::SeqCst).add(1));
            // unpack the record
            let ret = self.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer");
            if ret.dw3.trb_type() == TrbType::Link as u32 {
                // check if it's a link; if so, cycle the link, and go back to first_trb
                ret.dw3.set_cycle_bit(self.pcs);
                #[cfg(feature = "verbose-debug")]
                crate::println!(">>toggling PCS<<");
                self.pcs = !self.pcs;
                self.enq_pt = AtomicPtr::new(self.first_trb.load(Ordering::SeqCst));
                (self.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer"), self.pcs)
            } else {
                (ret, self.pcs)
            }
        }
    }

    pub fn assign_completion_handler(&mut self, f: fn(&mut CorigineUsb, usize, u32, u8)) {
        self.completion_handler = Some(f);
    }
}

// Corigine USB device controller event data structure
pub struct UdcEvent {
    pub erst: BufferInfo,
    pub p_erst: AtomicPtr<ErstS>,
    pub event_ring: BufferInfo,
    pub evt_dq_pt: AtomicPtr<EventTrbS>,
    pub ccs: bool,
    pub evt_seg0_last_trb: AtomicPtr<EventTrbS>,
}
impl Default for UdcEvent {
    fn default() -> Self {
        Self {
            erst: BufferInfo::default(),
            p_erst: AtomicPtr::new(core::ptr::null_mut()),
            event_ring: BufferInfo::default(),
            evt_dq_pt: AtomicPtr::new(core::ptr::null_mut()),
            ccs: true,
            evt_seg0_last_trb: AtomicPtr::new(core::ptr::null_mut()),
        }
    }
}

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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum UsbDeviceState {
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

#[allow(dead_code)]
#[derive(Eq, PartialEq, Debug, Clone, Copy)]
pub enum UmsState {
    CommandPhase,
    DataPhase,
    StatusPhase,
    Idle,
    AbortBulkOut,
    Reset,
    InterfaceChange,
    ConfigChange,
    Disconnect,
    Exit,
    Terminated,
}

pub struct AppPtr {
    pub addr: usize,
    pub len: usize,
    pub ep: u8,
}

pub struct CorigineUsb {
    pub ifram_base_ptr: usize,
    pub csr: AtomicCsr<u32>,
    pub irq_csr: AtomicCsr<u32>,

    pub udc_ep: [UdcEp; CRG_EP_NUM * 2 + 2], /* each EP gets an in/out statically allocated, + in/out for
                                              * EP0.
                                              * Reference driver has a bug? */
    p_epcx: AtomicPtr<EpCxS>,
    p_epcx_len: usize,

    pub udc_event: UdcEvent,

    /// A place to put data received from the hardware immediately, before it
    /// is processed by the driver interface.
    pub readout: [Option<[u8; CRG_UDC_APP_BUF_LEN]>; CRG_EP_NUM],
    pub setup: Option<[u8; 8]>,
    pub setup_tag: u8,
    stall_spec: [Option<bool>; CRG_EP_NUM * 2 + 2],

    pub max_packet_size: [Option<usize>; CRG_EP_NUM * 2 + 2],
    app_enq_index: [usize; CRG_EP_NUM + 1],
    app_deq_index: [usize; CRG_EP_NUM + 1],

    speed: UsbDeviceSpeed,

    // actual hardware pointer value to pass to UDC; not directly accessed by Rust
    pub ep0_buf: AtomicPtr<u8>,

    // event handler. Allows for divergence between no-std and std environments.
    handler: Option<fn(&mut Self, &mut EventTrbS) -> CrgEvent>,
    event_inner: Option<CrgEvent>,
    // data pointer of the current TRB to the application layer. Tuple is (addr, len).
    // we form the unsafe slice later depending on if the application needs mutability or not :-/
    app_ptr: Option<AppPtr>,

    pub state: UsbDeviceState,
    pub cur_interface_num: u8,

    // used by USB mass storage stacks to track connection state. Maybe we
    // can find a better place for it, but we need a spot that is accessible
    // via the interrupt handler.
    pub ms_state: UmsState,
    // addres, length tuples
    pub callback_wr: Option<(usize, usize)>,
    pub remaining_rd: Option<(usize, usize)>,
    pub remaining_wr: Option<(usize, usize)>,
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
    pub unsafe fn new(ifram_base_ptr: usize, csr: AtomicCsr<u32>, irq_csr: AtomicCsr<u32>) -> Self {
        Self {
            ifram_base_ptr,
            csr,
            irq_csr,
            // is there a way to make this less shitty?
            udc_ep: [
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
                UdcEp::default(),
            ],
            p_epcx: AtomicPtr::new(core::ptr::null_mut()),
            p_epcx_len: 0,
            udc_event: UdcEvent::default(),
            readout: [None; CRG_EP_NUM],
            setup: None,
            stall_spec: [None; CRG_EP_NUM * 2 + 2],
            max_packet_size: [None; CRG_EP_NUM * 2 + 2],
            app_enq_index: [0; CRG_EP_NUM + 1],
            app_deq_index: [0; CRG_EP_NUM + 1],
            setup_tag: 0,
            speed: UsbDeviceSpeed::Unknown,
            ep0_buf: AtomicPtr::new(core::ptr::null_mut()),
            handler: None,
            state: UsbDeviceState::NotAttached,
            cur_interface_num: 0,
            ms_state: UmsState::Idle,
            remaining_rd: None,
            callback_wr: None,
            remaining_wr: None,
            event_inner: None,
            app_ptr: None,
        }
    }

    pub fn pending_ep(&self) -> Option<usize> {
        if let Some(ap) = &self.app_ptr { Some(ap.ep as usize) } else { None }
    }

    pub fn setup_big_read(&mut self, app_buf: &mut [u8], disk: &[u8], offset: usize, length: usize) {
        crate::println!(
            "BIG READ offset {:x} len {:x} app_buf: {:x} disk: {:x?}",
            offset,
            length,
            app_buf.as_ptr() as usize,
            &disk[..length.min(8)]
        );
        let actual_len = length.min(app_buf.len());
        let remaining_len = if length <= app_buf.len() { 0 } else { length - actual_len };
        let chain = if remaining_len > 0 {
            self.remaining_rd = Some((offset + actual_len, remaining_len));
            CRG_XFER_SET_CHAIN
        } else {
            self.remaining_rd = None;
            0
        };
        app_buf[..actual_len].copy_from_slice(&disk[offset..offset + actual_len as usize]);
        self.bulk_xfer(1, USB_SEND, app_buf.as_ptr() as usize, actual_len, 0, chain);
    }

    pub fn setup_big_write(
        &mut self,
        app_buf_addr: usize,
        app_buf_len: usize,
        to_offset: usize,
        total_length: usize,
    ) {
        crate::println!(
            "BIG WRITE offset {:x} len {:x} app_buf: {:x} app_len: {:x}",
            to_offset,
            total_length,
            app_buf_addr,
            app_buf_len
        );
        let actual_len = total_length.min(app_buf_len);
        let remaining_len = if total_length <= app_buf_len { 0 } else { total_length - actual_len };
        let chain = if remaining_len > 0 {
            self.remaining_wr = Some((to_offset + actual_len, remaining_len));
            CRG_XFER_SET_CHAIN
        } else {
            self.remaining_wr = None;
            0
        };

        self.bulk_xfer(1, USB_RECV, app_buf_addr, actual_len, 0, chain);
        self.callback_wr = Some((to_offset, actual_len));
    }

    pub fn set_device_state(&mut self, state: UsbDeviceState) { self.state = state; }

    pub fn get_device_state(&self) -> UsbDeviceState { self.state }

    pub fn is_halted(&self, ep_num: u8, dir: bool) -> bool {
        let pei = CorigineUsb::pei(ep_num, dir);
        self.udc_ep[pei].ep_state == EpState::Halted
    }

    pub fn assign_handler(&mut self, handler_fn: fn(&mut Self, &mut EventTrbS) -> CrgEvent) {
        self.handler = Some(handler_fn);
    }

    pub fn pei(ep_num: u8, dir: bool) -> usize { (2 * ep_num + if dir { 1 } else { 0 }) as usize }

    pub fn pei_to_dir(pei: usize) -> bool { if (pei % 2) == 0 { CRG_IN } else { CRG_OUT } }

    pub fn pei_to_ep(pei: usize) -> u8 { (pei / 2) as u8 }

    pub fn assign_completion_handler(
        &mut self,
        handler_fn: fn(&mut Self, usize, u32, u8),
        ep_num: u8,
        dir: bool,
    ) {
        self.udc_ep[CorigineUsb::pei(ep_num, dir)].completion_handler = Some(handler_fn);
    }

    /// For use in simple applications that don't require concurrent application buffer pointers
    pub fn cbw_ptr(&self) -> usize { self.ifram_base_ptr + CRG_UDC_APP_BUFOFFSET }

    pub fn get_app_buf_ptr(&mut self, ep_num: u8, dir: bool) -> Option<usize> {
        let mut enq_index = self.app_enq_index[ep_num as usize];
        let pei = CorigineUsb::pei(ep_num, dir);
        let mps = self.max_packet_size[pei].expect("max packet size was not initialized!");
        let ep_num = ep_num as usize;
        let mut new_index = self.app_enq_index[ep_num] + mps;

        // normally check for overflow, but in the case of serial it seems to do better
        // if we don't do that. I'm still not sure why we seem to miss some IN ACKs.
        // TODO: figure this out
        if new_index + mps > CRG_UDC_APP_BUF_LEN {
            // ignore the the dq pointer, overflow for now -- for some reason, we aren't
            // getting all the interrupts we expect to be getting. Maybe some of them are
            // being combined in a race condition or something like that?
            new_index = 0;
            enq_index = 0;
            self.app_enq_index[ep_num] = 0;
        }
        if
        /* new_index + mps > CRG_UDC_APP_BUF_LEN */
        false {
            // we could do a circular buffer for enq/deq but I think a couple entries with a reset
            // is typically enough. If we hit this, then yah, we have to implement a full circular buffer.
            #[cfg(feature = "verbose-debug")]
            crate::println!("enqueue overflow");
            None
        } else {
            self.app_enq_index[ep_num] = new_index;
            if ep_num == 0 {
                let addr = self.ifram_base_ptr + CRG_UDC_EP0_BUF_OFFSET + enq_index;
                #[cfg(feature = "verbose-debug")]
                crate::println!("ep0 app_ptr: {:x} index {}", addr, enq_index);
                Some(addr)
            } else if ep_num <= CRG_EP_NUM {
                let addr = self.ifram_base_ptr
                    + CRG_UDC_APP_BUFOFFSET
                    + ((ep_num - 1) as usize * 2 + if dir { 1 } else { 0 }) * CRG_UDC_APP_BUF_LEN
                    + enq_index;
                #[cfg(feature = "verbose-debug")]
                crate::println!("ep0 app_ptr: {:x} index {}", addr, enq_index);
                Some(addr)
            } else {
                crate::println!("ep_num {} is out of range", ep_num);
                panic!("ep_num is out of range");
            }
        }
    }

    pub fn retire_app_buf_ptr(&mut self, ep_num: u8, dir: bool) -> usize {
        let ep_num = ep_num as usize;
        let deq_index = self.app_deq_index[ep_num];
        self.app_deq_index[ep_num] +=
            self.max_packet_size[ep_num].expect("max packet size was not initialized!");
        #[cfg(feature = "verbose-debug")]
        crate::println!(
            "ep{} retire: enq_index {}; deq_index {}",
            ep_num,
            self.app_enq_index[ep_num],
            self.app_deq_index[ep_num]
        );
        if self.app_deq_index[ep_num] == self.app_enq_index[ep_num] {
            #[cfg(feature = "verbose-debug")]
            crate::println!("Deq reset pointers to 0, ep{}", ep_num);
            // reset the pointers to 0 if we're empty
            self.app_deq_index[ep_num] = 0;
            self.app_enq_index[ep_num] = 0;
        }
        if ep_num == 0 {
            self.ifram_base_ptr + CRG_UDC_EP0_BUF_OFFSET + deq_index
        } else if ep_num <= CRG_EP_NUM {
            self.ifram_base_ptr
                + CRG_UDC_APP_BUFOFFSET
                + ((ep_num - 1) as usize * 2 + if dir { 1 } else { 0 }) * CRG_UDC_APP_BUF_LEN
                + deq_index
        } else {
            panic!("ep_num is out of range");
        }
    }

    pub fn reset(&mut self) {
        #[cfg(not(feature = "std"))]
        {
            println!("devcap: {:x}", self.csr.r(DEVCAP));
            println!("max speed: {:x}", self.csr.rf(DEVCONFIG_MAX_SPEED));
            println!("usb3 disable: {:x}", self.csr.rf(DEVCONFIG_USB3_DISABLE_COUNT));
        }
        /*
        Configured as: 1 interrupt, 4 phys EPIN, 4 phys EPOUT
        INFO:cramium_hal::usb::driver: devcap: 20014401 (libs/cramium-hal/src/usb/driver.rs:781)
        INFO:cramium_hal::usb::driver: max speed: 1 (libs/cramium-hal/src/usb/driver.rs:782)
        INFO:cramium_hal::usb::driver: usb3 disable: 8 (libs/cramium-hal/src/usb/driver.rs:783)
         */

        // NOTE: the indices are byte-addressed, and so need to be divided by size_of::<u32>()
        const MAGIC_TABLE: [(usize, u32); 18] = [
            (0x0fc, 0x00000001),
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
        #[cfg(feature = "magic-manual")]
        const MAGIC_TABLE: [(usize, u32); 17] = [
            (0x0fc, 0x00000001),
            (0x084, 0x01401388),
            (0x0f4, 0x0000f023),
            (0x088, 0x06060a09),
            (0x08c, 0x0d020509),
            (0x090, 0x04050603),
            (0x094, 0x0303000a),
            (0x098, 0x05131304),
            (0x09c, 0x06070d15),
            (0x0a0, 0x14160e0b),
            (0x0a4, 0x18060408),
            (0x0a8, 0x4b120c0f),
            (0x0ac, 0x03640d05),
            (0x0b0, 0x08080d09),
            (0x0b4, 0x20060914),
            (0x0b8, 0x040a0e0f),
            (0x0bc, 0x44080c09),
        ];

        for (offset, magic) in MAGIC_TABLE {
            unsafe { self.csr.base().add(offset / size_of::<u32>()).write_volatile(magic) };
        }

        // udc reset
        self.csr.rmwf(USBCMD_INT_ENABLE, 0);
        self.csr.rmwf(USBCMD_SOFT_RESET, 1);

        while self.csr.rf(USBCMD_SOFT_RESET) != 0 {
            // wait for reset to finish
        }

        // a dummy readback is in the reference code
        let mut dummy = 0;
        for i in 0..72 {
            dummy += unsafe { self.csr.base().add(i).read_volatile() };
        }
        println!("USB reset done: {:x}", dummy);
    }

    pub fn init(&mut self) {
        self.csr.rmwf(USBCMD_INT_ENABLE, 0);
        self.csr.rmwf(USBCMD_RUN_STOP, 0);

        self.csr.rmwf(USBCMD_SOFT_RESET, 1);

        while self.csr.rf(USBCMD_SOFT_RESET) != 0 {
            // wait for reset to finish
        }

        self.csr.wo(DEVCONFIG, 0x80 | CRG_UDC_CFG0_MAXSPEED_FS | CRG_UDC_CFG0_MAXSPEED_HS);

        self.csr.wo(
            EVENTCONFIG,
            self.csr.ms(EVENTCONFIG_CSC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PEC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PPC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PRC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_PLC_ENABLE, 1)
                | self.csr.ms(EVENTCONFIG_CEC_ENABLE, 1),
        );

        // event_ring_init
        // init event ring 0

        // event_ring_init, but inline
        // allocate event ring segment table
        let erst: &mut [ErstS] = unsafe {
            core::slice::from_raw_parts_mut(
                (self.ifram_base_ptr + CRG_UDC_ERST_OFFSET) as *mut ErstS,
                CRG_ERST_SIZE,
            )
        };
        for e in erst.iter_mut() {
            *e = ErstS::default();
        }
        self.udc_event.erst.len = erst.len() * size_of::<ErstS>();
        self.udc_event.erst.vaddr = AtomicPtr::new(erst.as_mut_ptr() as *mut u8); // ErstS ??
        self.udc_event.p_erst =
            AtomicPtr::new(self.udc_event.erst.vaddr.load(Ordering::SeqCst) as *mut ErstS);

        // allocate event ring
        let event_ring = unsafe {
            core::slice::from_raw_parts_mut(
                (self.ifram_base_ptr + CRG_UDC_EVENTRING_OFFSET) as *mut u8,
                CRG_EVENT_RING_SIZE * size_of::<EventTrbS>(),
            )
        };
        event_ring.fill(0);

        self.udc_event.event_ring.len = event_ring.len();
        self.udc_event.event_ring.vaddr = AtomicPtr::new(event_ring.as_mut_ptr()); // EventTrbS ??
        self.udc_event.evt_dq_pt =
            AtomicPtr::new(self.udc_event.event_ring.vaddr.load(Ordering::SeqCst) as *mut EventTrbS);
        self.udc_event.evt_seg0_last_trb = AtomicPtr::new(unsafe {
            (self.udc_event.event_ring.vaddr.load(Ordering::SeqCst) as *mut EventTrbS)
                .add(CRG_EVENT_RING_SIZE - 1)
        });

        self.udc_event.ccs = true;

        // copy control structure pointers to hardware-managed memory
        let p_erst =
            unsafe { self.udc_event.p_erst.load(Ordering::SeqCst).as_mut().expect("invalid pointer") };
        p_erst.seg_addr_lo = self.udc_event.event_ring.vaddr.load(Ordering::SeqCst) as u32;
        p_erst.seg_addr_hi = 0;
        p_erst.seg_size = CRG_EVENT_RING_SIZE as u32;
        p_erst.rsvd = 0;

        self.csr.wo(ERSTSZ, CRG_ERST_SIZE as u32);
        self.csr.wo(ERSTBALO, self.udc_event.erst.vaddr.load(Ordering::SeqCst) as u32);
        self.csr.wo(ERSTBAHI, 0);
        self.csr.wo(
            ERDPLO,
            (self.udc_event.event_ring.vaddr.load(Ordering::SeqCst) as u32 & 0xFFFF_FFF0)
                | self.csr.ms(ERDPLO_EHB, 1),
        );
        self.csr.wo(ERDPHI, 0);

        self.csr.wo(IMAN, self.csr.ms(IMAN_IE, 1) | self.csr.ms(IMAN_IP, 1));
        self.csr.wo(IMOD, 0);
        compiler_fence(Ordering::SeqCst);

        // Set up storage for Endpoint contexts
        // init device context and ep context, refer to 7.6.2
        self.p_epcx = AtomicPtr::new((self.ifram_base_ptr + CRG_UDC_EPCX_OFFSET) as *mut EpCxS);
        self.p_epcx_len = CRG_EP_NUM * 2 * size_of::<EpCxS>();

        assert!(self.p_epcx.load(Ordering::SeqCst) as u32 & 0x3F == 0, "EpCxS storage misaligned");
        self.csr.wo(DCBAPLO, self.p_epcx.load(Ordering::SeqCst) as u32 & 0xFFFF_FFC0);
        self.csr.wo(DCBAPHI, 0);
        compiler_fence(Ordering::SeqCst);

        // initial ep0 transfer ring
        self.init_ep0();

        // disable u1 u2
        self.csr.wo(U3PORTPMSC, 0);

        // disable 2.0 LPM
        self.csr.wo(U2PORTPMSC, 0);

        crate::println!("USB hw init done");
    }

    pub fn init_ep0(&mut self) {
        #[cfg(feature = "verbose-debug")]
        crate::println!("Begin init_ep0");
        let udc_ep = &mut self.udc_ep[0];

        udc_ep.ep_num = 0;
        udc_ep.direction = USB_SEND;
        udc_ep.ep_type = EpType::ControlOrInvalid;
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
        udc_ep.tran_ring_info.vaddr = AtomicPtr::new(ep0_tr_ring.as_mut_ptr() as *mut u8);
        udc_ep.tran_ring_info.len = ep0_tr_ring.len() * size_of::<TransferTrbS>();
        udc_ep.first_trb = AtomicPtr::new((&mut ep0_tr_ring[0]) as *mut TransferTrbS);
        udc_ep.last_trb =
            AtomicPtr::new((&ep0_tr_ring[ep0_tr_ring.len() - 1]) as *const TransferTrbS as *mut TransferTrbS);

        udc_ep.enq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
        udc_ep.deq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
        #[cfg(feature = "verbose-debug")]
        crate::println!(
            "ep0.enq_pt {:x}, ep0.deq_pt {:x}",
            udc_ep.enq_pt.load(Ordering::SeqCst) as usize,
            udc_ep.deq_pt.load(Ordering::SeqCst) as usize
        );
        udc_ep.pcs = true;
        udc_ep.tran_ring_full = false;

        unsafe { udc_ep.last_trb.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") }
            .setup_link_trb(true, udc_ep.tran_ring_info.vaddr.load(Ordering::SeqCst) as *mut TransferTrbS);

        let cmd_param0: u32 = (udc_ep.tran_ring_info.vaddr.load(Ordering::SeqCst) as u32) & 0xFFFF_FFF0
            | self.csr.ms(CMDPARA0_CMD0_INIT_EP0_DCS, udc_ep.pcs as u32);
        let cmd_param1: u32 = 0;
        #[cfg(feature = "verbose-debug")]
        {
            println!("ep0 ring dma addr = {:x}", udc_ep.tran_ring_info.vaddr.load(Ordering::SeqCst) as usize);
            println!("INIT EP0 CMD par0 = {:x} par1 = {:x}", cmd_param0, cmd_param1);
        }

        self.issue_command(CmdType::InitEp0, cmd_param0, cmd_param1)
            .expect("couldn't issue ep0 init command");

        self.ep0_buf = AtomicPtr::new((self.ifram_base_ptr + CRG_UDC_EP0_BUF_OFFSET) as *mut u8);
    }

    #[cfg(feature = "std")]
    /// This must be called before `start()` is invoked if a custom handler is to be hooked.
    /// The main reason this API exists is to provide some backward compatibility with prior APIs
    /// that relied on the stock handler.
    ///
    /// Safety: only safe to call if you actually claimed the interrupt, before calling `start()`
    pub unsafe fn irq_claimed(&self) { INTERRUPT_INIT_DONE.store(true, Ordering::SeqCst); }

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

        /*
        #[cfg(feature = "std")]
        if !INTERRUPT_INIT_DONE.fetch_or(true, Ordering::SeqCst) {
            xous::claim_interrupt(
                utralib::utra::irqarray1::IRQARRAY1_IRQ,
                handle_usb,
                self as *const CorigineUsb as *mut usize,
            )
            .expect("couldn't claim irq");
            crate::println!("interrupt claimed");
        }
        */
        // self.irq_csr.wfo(utralib::utra::irqarray1::EV_EDGE_TRIGGERED_USE_EDGE, 1);
        // self.irq_csr.wfo(utralib::utra::irqarray1::EV_POLARITY_RISING, 0);
        // self.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xffff_ffff); // blanket clear
        // self.irq_csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 1);
        // enable interruptor 0 via IMAN (we only map one in the current

        // UTRA - if we need more interruptors we have to update utra)
        self.csr.rmwf(IMAN_IE, 1);

        self.print_status(self.csr.r(PORTSC));

        self.set_addr(0, CRG_INT_TARGET);
    }

    pub fn issue_command(&mut self, cmd: CmdType, p0: u32, p1: u32) -> core::result::Result<(), Error> {
        // don't allow overlapping commands. This can hang the system if the USB core is wedged.
        loop {
            if self.csr.rf(CMDCTRL_ACTIVE) == 0 {
                break;
            }
        }
        self.csr.wo(CMDPARA0, p0);
        self.csr.wo(CMDPARA1, p1);
        self.csr.wo(CMDCTRL, self.csr.ms(CMDCTRL_ACTIVE, 1) | self.csr.ms(CMDCTRL_TYPE, cmd as u32));
        #[cfg(feature = "verbose-debug")]
        crate::println!(
            "issue_command: {:?} <- {:x}, {:x}",
            CmdType::try_from(self.csr.rf(CMDCTRL_TYPE)),
            self.csr.r(CMDPARA0),
            self.csr.r(CMDPARA1),
        );
        compiler_fence(Ordering::SeqCst);
        loop {
            if self.csr.rf(CMDCTRL_ACTIVE) == 0 {
                break;
            }
        }
        if self.csr.rf(CMDCTRL_STATUS) != 0 {
            // println!("...issue_command(): fail");
            return Err(Error::CmdFailure);
        }
        // println!("issue_command(): success");
        Ok(())
    }

    pub fn udc_handle_interrupt(&mut self) -> CrgEvent {
        let mut ret = CrgEvent::None;
        let status = self.csr.r(USBSTS);
        // self.print_status(status);
        if (status & self.csr.ms(USBSTS_SYSTEM_ERR, 1)) != 0 {
            println!("System error");
            self.csr.wfo(USBSTS_SYSTEM_ERR, 1);
            println!("USBCMD: {:x}", self.csr.r(USBCMD));
            CrgEvent::Error
        } else {
            if (status & self.csr.ms(USBSTS_EINT, 1)) != 0 {
                self.csr.wfo(USBSTS_EINT, 1);
                ret = self.process_event_ring(); // there is only one event ring
            }
            if self.csr.rf(IMAN_IE) != 0 {
                self.csr.rmwf(IMAN_IE, 1);
            }
            self.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xFFFF_FFFF);
            // re-enable interrupts
            self.irq_csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 1);
            ret
        }
    }

    pub fn process_event_ring(&mut self) -> CrgEvent {
        // clear IP
        self.csr.rmwf(IMAN_IP, 1);

        // aggregated event status
        let mut ep_out: u16 = 0;
        let mut ep_in_complete: u16 = 0;
        let mut ep_setup: u16 = 0;
        let mut connect = false;
        loop {
            let event = {
                if self.udc_event.evt_dq_pt.load(Ordering::SeqCst).is_null() {
                    // break;
                    #[cfg(feature = "std")]
                    crate::println!("null pointer in process_event_ring");
                    return CrgEvent::None;
                }
                let event_ptr = self.udc_event.evt_dq_pt.load(Ordering::SeqCst) as usize;
                unsafe { (event_ptr as *mut EventTrbS).as_mut().expect("couldn't deref pointer") }
            };

            if event.dw3.cycle_bit() != self.udc_event.ccs {
                break;
            }

            if let Some(handler) = self.handler {
                match handler(self, event) {
                    CrgEvent::Connect => connect = true,
                    CrgEvent::Error => {
                        #[cfg(feature = "std")]
                        crate::println!("Error in handle_event; ignoring error... {:?}", event);
                    }
                    CrgEvent::None => (),
                    CrgEvent::Data(o, i, s) => {
                        ep_out |= o;
                        ep_in_complete |= i;
                        ep_setup |= s;
                    }
                }
            } else {
                #[cfg(feature = "std")]
                crate::println!("No packet handler set, event lost");
            }

            if self.udc_event.evt_dq_pt.load(Ordering::SeqCst)
                == self.udc_event.evt_seg0_last_trb.load(Ordering::SeqCst)
            {
                #[cfg(feature = "verbose-debug")]
                crate::println!(
                    " evt_last_trb {:x}",
                    self.udc_event.evt_seg0_last_trb.load(Ordering::SeqCst) as usize
                );
                self.udc_event.ccs = !self.udc_event.ccs;
                // does this...go to null to end the transfer??
                self.udc_event.evt_dq_pt =
                    AtomicPtr::new(self.udc_event.event_ring.vaddr.load(Ordering::SeqCst) as *mut EventTrbS);
            } else {
                self.udc_event.evt_dq_pt =
                    AtomicPtr::new(unsafe { self.udc_event.evt_dq_pt.load(Ordering::SeqCst).add(1) });
            }
        }

        // update dequeue pointer
        self.csr.wo(ERDPHI, 0);
        self.csr.wo(
            ERDPLO,
            (self.udc_event.evt_dq_pt.load(Ordering::SeqCst) as u32 & 0xFFFF_FFF0) | CRG_UDC_ERDPLO_EHB,
        );
        compiler_fence(Ordering::SeqCst);

        // aggregate events and form a report
        if connect && ((ep_out | ep_in_complete | ep_setup) != 0) {
            #[cfg(feature = "std")]
            crate::println!("*** Connect event concurrent with packets, API cannot handle ***");
        }
        if connect {
            CrgEvent::Connect
        } else {
            if (ep_out | ep_in_complete | ep_setup) != 0 {
                CrgEvent::Data(ep_out, ep_in_complete, ep_setup)
            } else {
                CrgEvent::None
            }
        }
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

    pub fn pp(&self) -> bool { self.csr.rf(PORTSC_PP) != 0 }

    pub fn portsc_val(&self) -> u32 { self.csr.r(PORTSC) }

    pub fn stop(&mut self) {
        self.csr.rmwf(IMAN_IE, 0);
        self.csr.rmwf(USBCMD_INT_ENABLE, 0);
        self.csr.rmwf(USBCMD_RUN_STOP, 0);
        self.csr.wo(EVENTCONFIG, 0);
        self.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xffff_ffff); // blanket clear
        self.irq_csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 0);
    }

    pub fn set_addr(&mut self, addr: u8, target: u32) {
        self.issue_command(CmdType::SetAddr, self.csr.ms(CMDPARA0_CMD2_SET_ADDR, addr as u32), 0)
            .expect("couldn't issue command");

        let udc_ep = &mut self.udc_ep[0];
        let enq_pt =
            unsafe { udc_ep.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        enq_pt.control_status_trb(udc_ep.pcs, true, false, self.setup_tag, target, USB_SEND);
        // crate::println!("enq_pt {:x}: {:x?}", enq_pt as *const TransferTrbS as usize, enq_pt);
        let (_enq_pt, _pcs) = udc_ep.increment_enq_pt();
        compiler_fence(Ordering::SeqCst);
        self.knock_doorbell(0);
        #[cfg(feature = "std")]
        crate::println!(" ******* set address done {}", addr);
    }

    // knock door bell then controller will start transfer for the specific endpoint
    // pei: physical endpoint index
    pub fn knock_doorbell(&mut self, pei: u32) {
        compiler_fence(Ordering::SeqCst);
        #[cfg(feature = "verbose-debug")]
        crate::println!(">>doorbell: {:x}<<", pei);
        self.csr.wfo(DOORBELL_TARGET, pei);
    }

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
            println!("Config0,1: {:x}, {:x}", self.csr.r(DEVCONFIG), self.csr.r(EVENTCONFIG));
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
            crate::println!("Config0,1: {:x}, {:x}", self.csr.r(DEVCONFIG), self.csr.r(EVENTCONFIG));
            let mut s = String::new();
            s.push_str(&format!("Status: {:x} | ", status));
            for &(field, name) in bitflags.iter() {
                if (status & 1 << field) != 0 {
                    s.push_str(&format!("{} ", name));
                }
            }
            s.push_str(&format!(
                "| Speed: {} PLS: {}",
                speeds[((status >> 10) & 0x7) as usize],
                plses[((status >> 5) & 0xF) as usize]
            ));
            crate::println!("{}", s);
        }
    }

    pub fn ep0_send(&mut self, addr: usize, len: usize, intr_target: u32) {
        #[cfg(feature = "verbose-debug")]
        unsafe {
            crate::println!("ep0 send ({}) {:x?}", len, core::slice::from_raw_parts(addr as *const u8, len));
        }
        let mut enq_pt =
            unsafe { self.udc_ep[0].enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        let mut pcs = self.udc_ep[0].pcs;
        let tag = self.setup_tag;
        if len != 0 {
            enq_pt.control_data_trb(
                addr as u32,
                pcs,
                1,
                len as u32,
                0,
                true,
                false,
                USB_SEND,
                tag,
                intr_target,
            );
            (enq_pt, pcs) = self.udc_ep[0].increment_enq_pt();
        }

        enq_pt.control_status_trb(pcs, false, false, tag, intr_target, USB_RECV);

        let (_enq_pt, _pcs) = self.udc_ep[0].increment_enq_pt();
        compiler_fence(Ordering::SeqCst);
        self.knock_doorbell(0);
    }

    pub fn ep0_enqueue(&mut self, addr: usize, len: usize, intr_target: u32) {
        let enq_pt =
            unsafe { self.udc_ep[0].enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        enq_pt.control_data_trb(
            addr as u32,
            self.udc_ep[0].pcs,
            1,
            len as u32,
            0,
            true,
            false,
            USB_SEND,
            self.setup_tag,
            intr_target,
        );
        let (_enq_pt, _pcs) = self.udc_ep[0].increment_enq_pt();
        self.knock_doorbell(0);
    }

    pub fn ep0_enqueue_zlp(&mut self, stall: bool, intr_target: u32) {
        let udc_ep = &mut self.udc_ep[0];
        let enq_pt =
            unsafe { udc_ep.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        enq_pt.control_status_trb(udc_ep.pcs, false, stall, self.setup_tag, intr_target, USB_RECV);
        let (_, _) = udc_ep.increment_enq_pt();
        self.knock_doorbell(0);
    }

    pub fn ep0_status(&mut self, stall: bool, intr_target: u32) {
        let udc_ep = &mut self.udc_ep[0];
        let enq_pt =
            unsafe { udc_ep.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        enq_pt.control_status_trb(udc_ep.pcs, false, stall, self.setup_tag, intr_target, USB_SEND);
        let (_, _) = udc_ep.increment_enq_pt();
        self.knock_doorbell(0);
    }

    pub fn ep0_receive(&mut self, addr: usize, length: usize, intr_target: u32) {
        let udc_ep = &mut self.udc_ep[0];
        let mut enq_pt =
            unsafe { udc_ep.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        let tag = self.setup_tag;
        let mut pcs = udc_ep.pcs;
        if length != 0 {
            enq_pt.control_data_trb(
                addr as u32,
                pcs,
                1,
                length as u32,
                0,
                false,
                false,
                true,
                tag,
                intr_target,
            );
            (enq_pt, pcs) = udc_ep.increment_enq_pt();
        }

        #[cfg(feature = "verbose-debug")]
        crate::println!("ep0_rx: {:x?}", enq_pt);
        enq_pt.control_status_trb(
            pcs,
            pcs,
            self.stall_spec[0].take().unwrap_or(false),
            tag,
            intr_target,
            false,
        );

        let (_enq_pt, _pcs) = udc_ep.increment_enq_pt();
        compiler_fence(Ordering::SeqCst);
        self.knock_doorbell(0);
    }

    pub fn ep_xfer(
        &mut self,
        ep_num: u8,
        dir: bool,
        addr: usize,
        len: usize,
        intr_target: u32,
        no_intr: bool,
        no_knock: bool,
        append_zero_packet: bool,
    ) {
        let pei = CorigineUsb::pei(ep_num, dir);
        let num_trb = if len != 0 {
            len / MAX_TRB_XFER_LEN + if len % MAX_TRB_XFER_LEN != 0 { 1 } else { 0 }
        } else {
            1
        };
        let udc_ep = &mut self.udc_ep[pei];
        let mut enq_pt =
            unsafe { udc_ep.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        #[cfg(feature = "verbose-debug")]
        crate::println!(
            "ep_xfer() pei: {}, enq_pt: {:x}, buf_addr: {:x}, pcs: {}, len: {:x}",
            pei,
            enq_pt as *mut TransferTrbS as usize,
            addr,
            udc_ep.pcs,
            len
        );
        let mut tmp_len: usize = 0;
        let mut ioc: bool = true;
        let mut chain_bit: bool = false;
        let mut pcs = udc_ep.pcs;
        for index in 0..num_trb {
            if num_trb == 1 {
                tmp_len = len;
                ioc = true;
                chain_bit = false;
            } else if (index != (num_trb - 1)) && (num_trb > 1) {
                tmp_len = MAX_TRB_XFER_LEN;
                ioc = false;
                chain_bit = true;
            } else if (index == (num_trb - 1)) && (num_trb > 1) {
                tmp_len = if len % MAX_TRB_XFER_LEN != 0 { len % MAX_TRB_XFER_LEN } else { MAX_TRB_XFER_LEN };
                ioc = true;
                chain_bit = false;
            }

            if no_intr {
                ioc = false;
            }

            enq_pt.prepare_transfer_trb(
                tmp_len,
                addr + MAX_TRB_XFER_LEN * index,
                1,
                pcs,
                TrbType::XferNormal,
                false,
                chain_bit,
                ioc,
                false,
                false, // This is only valid if b_setup_stage is true
                false,
                0,
                0,
                false,
                append_zero_packet,
                intr_target,
            );

            (enq_pt, pcs) = udc_ep.increment_enq_pt();
        }
        compiler_fence(Ordering::SeqCst);
        if !no_knock {
            self.knock_doorbell(pei as u32);
        }
    }

    pub fn ep_enable(&mut self, ep_num: u8, dir: bool, max_packet_size: u16, ep_type: EpType) {
        let mut baseline_enable = self.csr.r(EPENABLE);
        if ep_num == 0 {
            panic!("Can't use ep_enable on EP0, use init_ep0 instead!");
        }
        let pei = CorigineUsb::pei(ep_num, dir);
        let udc_ep = &mut self.udc_ep[pei];
        let len = CRG_TD_RING_SIZE * size_of::<TransferTrbS>();
        let vaddr = self.ifram_base_ptr + CRG_UDC_EP_TR_OFFSET + (pei - 2) * len;
        #[cfg(feature = "std")]
        crate::println!(
            "udc_ep->PEI = {}, xfer ring addr {:x}, dir {}, mps: {}",
            pei,
            vaddr,
            if dir { "OUT" } else { "IN" },
            max_packet_size,
        );
        assert!(
            vaddr != 0 && vaddr <= CRG_UDC_EP0_TR_OFFSET + self.ifram_base_ptr + CRG_UDC_EP_TRSIZE,
            "failed to allocate trb ring"
        );
        udc_ep.ep_num = ep_num;
        udc_ep.direction = dir;
        udc_ep.max_packet_size = max_packet_size;
        udc_ep.ep_type = ep_type;

        // setup TransferTrb ring - TD_RING_SIZE entries of TransferTrbS
        udc_ep.tran_ring_info.vaddr = AtomicPtr::new(vaddr as *mut u8);
        udc_ep.tran_ring_info.dma = vaddr as u64;
        udc_ep.tran_ring_info.len = len;
        udc_ep.first_trb = AtomicPtr::new(vaddr as *mut TransferTrbS);
        udc_ep.last_trb =
            unsafe { AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst).add(CRG_TD_RING_SIZE - 1)) };
        // clear the entire TRB region
        let clear_region =
            unsafe { core::slice::from_raw_parts_mut(vaddr as *mut u32, len / size_of::<u32>()) };
        clear_region.fill(0);

        let last_trb =
            unsafe { udc_ep.last_trb.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        last_trb.setup_link_trb(true, udc_ep.tran_ring_info.dma as *mut TransferTrbS);

        udc_ep.enq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
        udc_ep.deq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
        udc_ep.pcs = true;
        udc_ep.tran_ring_full = false;

        // setup endpoint context: EpCxS
        let epcx = unsafe {
            self.p_epcx.load(Ordering::SeqCst).add(pei - 2).as_mut().expect("couldn't deref pointer")
        };
        epcx.epcx_setup(&udc_ep);
        #[cfg(feature = "verbose-debug")]
        crate::println!(
            "dcbap {:x}/{:x}; ecpx *{:x}; epcx: {:x?}",
            self.csr.r(DCBAPHI),
            self.csr.r(DCBAPLO),
            epcx as *const EpCxS as usize,
            epcx,
        );
        self.issue_command(CmdType::ConfigEp, 1 << pei as u32, 0).expect("couldn't issue command");
        self.udc_ep[pei].ep_state = EpState::Running;

        #[cfg(feature = "std")]
        crate::println!("waiting for EP{} to go to enabled, baseline: {:x}", ep_num, baseline_enable);
        loop {
            let new_enable = self.csr.r(EPENABLE);
            if baseline_enable != new_enable {
                #[cfg(feature = "verbose-debug")]
                crate::println!("EPENABLE {:x}, EPRUN {:x}", self.csr.r(EPENABLE), self.csr.r(EPRUNNING));
                baseline_enable = new_enable;
            }
            if self.csr.r(EPENABLE) & (1 << pei) != 0 {
                break;
            }
        }
        #[cfg(feature = "verbose-debug")]
        crate::println!("ENABLED");
    }

    pub fn ep_disable(&mut self, ep_num: u8, dir: bool) {
        #[cfg(feature = "std")]
        crate::println!("Disable ep {}, dir {}", ep_num, if dir { "OUT" } else { "IN" });
        let pei = CorigineUsb::pei(ep_num, dir);
        let param0 = 1 << pei as u32;
        if param0 & self.csr.r(EPRUNNING) != 0 {
            self.issue_command(CmdType::StopEp, param0, 0).expect("couldn't issue commmand");
            loop {
                if self.csr.r(EPRUNNING) & param0 == 0 {
                    break;
                }
            }
        }
        let zeroize = unsafe {
            core::slice::from_raw_parts_mut(
                self.p_epcx.load(Ordering::SeqCst).add(pei - 2) as *mut u8,
                size_of::<EpCxS>(),
            )
        };
        zeroize.fill(0);
        self.csr.wo(EPENABLE, 1 << pei as u32);
        self.udc_ep[pei].ep_state = EpState::Disabled;
        compiler_fence(Ordering::SeqCst);
    }

    pub fn ep_halt(&mut self, ep_num: u8, dir: bool) {
        let mut pei = CorigineUsb::pei(ep_num, dir);
        if pei == 0 || pei == 1 {
            pei = 0;
        }
        if pei == 0 {
            self.ep0_status(true, 0);
        } else if self.udc_ep[pei].ep_state == EpState::Running {
            self.issue_command(CmdType::SetHalt, 1 << pei as u32, 0).expect("couldn't issue command");
            while self.csr.rf(EPRUNNING_RUNNING) != 0 {
                // busy wait
            }
            self.udc_ep[pei].ep_state = EpState::Halted;
        }
    }

    pub fn ep_unhalt(&mut self, ep_num: u8, dir: bool) {
        let pei = CorigineUsb::pei(ep_num, dir);
        self.issue_command(CmdType::ClearHalt, 1 << pei as u32, 0).expect("couldn't issue command");

        let ep_cx_s = unsafe { self.p_epcx.load(Ordering::SeqCst).as_mut().expect("couldn't deref ptr") };
        let deq_pt = self.udc_ep[pei].deq_pt.load(Ordering::SeqCst);
        ep_cx_s.dw2 = EpCxDw2(0);
        ep_cx_s.dw2.set_deq_cyc_state(self.udc_ep[pei].pcs);
        ep_cx_s.dw2.set_deq_ptr_lo(deq_pt as u32 >> 4);
        ep_cx_s.dw3 = 0;
        compiler_fence(Ordering::SeqCst);
        self.issue_command(CmdType::SetTrDqPtr, 1 << pei as u32, 0).expect("couldn't isssue command");
        while self.csr.rf(EPRUNNING_RUNNING) == 0 {
            // busy wait
        }
        self.udc_ep[pei].ep_state = EpState::Running;
        self.knock_doorbell(pei as u32);
    }

    pub fn handle_set_stalled(&mut self, ep_num: u8, dir: bool, stalled: bool) {
        let pei = CorigineUsb::pei(ep_num, dir);
        // Note: in this case, we don't differentiate EP0 PEI, because in and out
        // stall is handled separately despite being one physical endpoint.

        // TODO: resolve the problem with stalls
        //   - figure out the actual protocol spec for this
        //   - figure out how corigine actually handles stalls

        // this works with linux, but not with windows.
        if stalled != self.stall_spec[pei].unwrap_or(false) {
            self.stall_spec[pei] = Some(stalled);
            // this code is sus, too
            if ep_num == 0 {
                if dir == USB_RECV {
                    self.ep0_enqueue_zlp(stalled, CRG_INT_TARGET);
                } else {
                    self.ep0_status(stalled, CRG_INT_TARGET);
                }
            } else {
                if stalled {
                    self.ep_halt(ep_num, dir);
                } else {
                    self.ep_unhalt(ep_num, dir);
                }
            }
        }
    }

    pub fn bulk_xfer(
        &mut self,
        ep_num: u8,
        dir: bool,
        addr: usize,
        len: usize,
        intr_target: u32,
        transfer_flag: u8,
    ) {
        const TD_SIZE: u32 = 1;
        let mut ioc: bool = true;
        let mut azp: bool = false;
        let mut tmp_len = 0;
        let mut num_trb: usize = 1;
        let mut chain_bit: bool = false;
        // struct crg_udc_ep *udc_ep_ptr;
        // struct transfer_trb_s *enq_pt;
        let pei = CorigineUsb::pei(ep_num, dir);
        if len != 0 {
            num_trb = len / MAX_TRB_XFER_LEN + if len % MAX_TRB_XFER_LEN != 0 { 1 } else { 0 };
        }
        let udc_ep = &mut self.udc_ep[pei];
        let mut enq_pt =
            unsafe { udc_ep.enq_pt.load(Ordering::SeqCst).as_mut().expect("couldn't deref pointer") };
        let mut pcs = udc_ep.pcs;

        /*
        crate::println!(
            "PEI = {} bufaddr = 0x{:x} pcs = {} length = 0x{:x}, enq_pt = 0x{:x?}",
            pei,
            addr,
            udc_ep.pcs,
            len,
            enq_pt,
        );
        */
        for i in 0..num_trb {
            if num_trb == 1 {
                //only 1 trb
                tmp_len = len;
                ioc = true;
                chain_bit = false;
            } else if (i != (num_trb - 1)) && (num_trb > 1) {
                //num_trb > 1,  not last trb
                tmp_len = MAX_TRB_XFER_LEN;
                ioc = false;
                chain_bit = true;
            } else if (i == (num_trb - 1)) && (num_trb > 1) {
                //num_trb > 1,  last trb
                tmp_len = if len % MAX_TRB_XFER_LEN != 0 { len % MAX_TRB_XFER_LEN } else { MAX_TRB_XFER_LEN };
                ioc = true;
                chain_bit = true;
            }

            if transfer_flag & CRG_XFER_NO_INTR != 0 {
                ioc = false;
            }

            if transfer_flag & CRG_XFER_SET_CHAIN != 0 {
                chain_bit = true;
            }

            if transfer_flag & CRG_XFER_AZP != 0 {
                azp = true;
            }

            enq_pt.prepare_transfer_trb(
                tmp_len,
                addr + MAX_TRB_XFER_LEN * i,
                TD_SIZE,
                pcs,
                TrbType::XferNormal,
                false,
                chain_bit,
                ioc,
                false,
                false,
                false,
                0,
                0,
                false,
                azp,
                intr_target as u32,
            );

            (enq_pt, pcs) = udc_ep.increment_enq_pt();
        }

        if transfer_flag & CRG_XFER_NO_DB != 0 {
            return;
        }

        self.knock_doorbell(pei as _);
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
        crate::println!("ll_reset is UNSURE");
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
    /// The hardware stack works with endpoints mapped as "PEI", where IN/OUT are paired
    /// into a single index and the meaning of IN and OUT are fixed based on the position in
    /// the index. The allocator as written cannot handle devices allocated with a `None`
    /// EP specifier while also having only either an IN or an OUT EP (but not the other).
    pub free_pei: usize,
    /// Tuple is (type of endpoint, max packet size)
    pub ep_meta: [Option<(EpType, usize)>; CRG_EP_NUM],
    pub ep_out_ready: Box<[AtomicBool]>,
    pub address_is_set: Arc<AtomicBool>,
    pub event: Option<CrgEvent>,
}
#[cfg(feature = "std")]
impl CorigineWrapper {
    pub fn new(obj: CorigineUsb) -> Self {
        let c = Self {
            hw: Arc::new(Mutex::new(obj)),
            free_pei: 2,
            ep_meta: [None; CRG_EP_NUM],
            ep_out_ready: (0..CRG_EP_NUM + 1)
                .map(|_| AtomicBool::new(false))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            event: None,
            address_is_set: Arc::new(AtomicBool::new(false)),
        };
        c
    }

    pub fn clone(&self) -> Self {
        let mut c = Self {
            hw: self.hw.clone(),
            free_pei: 2,
            ep_meta: [None; CRG_EP_NUM],
            ep_out_ready: (0..CRG_EP_NUM + 1)
                .map(|_| AtomicBool::new(false))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            event: None,
            address_is_set: self.address_is_set.clone(),
        };
        c.ep_meta.copy_from_slice(&self.ep_meta);
        for (dst, src) in c.ep_out_ready.iter().zip(self.ep_out_ready.iter()) {
            dst.store(src.load(Ordering::SeqCst), Ordering::SeqCst);
        }
        c
    }

    pub fn core(&self) -> std::sync::MutexGuard<'_, CorigineUsb> {
        #[cfg(feature = "verbose-debug")]
        crate::println!("lock status: {}", if self.hw.try_lock().is_err() { "locked " } else { "unlocked" });
        self.hw.lock().unwrap()
    }
}

#[cfg(feature = "std")]
impl UsbBus for CorigineWrapper {
    /// Indicates that `set_device_address` must be called before accepting the corresponding
    /// control transfer, not after.
    ///
    /// The default value for this constant is `false`, which corresponds to the USB 2.0 spec, 9.4.6
    const QUIRK_SET_ADDRESS_BEFORE_STATUS: bool = true;

    /// Allocates an endpoint and specified endpoint parameters. This method is called by the device
    /// and class implementations to allocate endpoints, and can only be called before
    /// [`enable`](UsbBus::enable) is called.
    ///
    /// This allocator cannot handle split allocations of EP where the address is `None` but successive
    /// allocations are not for the same EP. i.e. it is assumed that devices are always allocated in
    /// IN/OUT pairs. Single IN or OUT EPs are not supported, and it's also not supported to allocate
    /// all the IN and then all the OUT.
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
        _interval: u8,
    ) -> Result<EndpointAddress> {
        // #[cfg(feature = "verbose-debug")]
        crate::println!("alloc_ep {:?} size: {} dir: {:?}", ep_addr, max_packet_size, ep_dir);

        let allocated_ep = if let Some(addr) = ep_addr {
            addr.index()
        } else {
            let ep = self.free_pei / 2;
            self.free_pei += 1;
            ep
        };
        if allocated_ep > (CRG_EP_NUM / 2) + 1 {
            return Err(UsbError::EndpointOverflow);
        }

        let dir = match ep_dir {
            UsbDirection::Out => CRG_OUT,
            UsbDirection::In => CRG_IN,
        };
        let pei = CorigineUsb::pei(allocated_ep as u8, dir);

        let hw_ep_type = match ep_type {
            EndpointType::Control => EpType::ControlOrInvalid,
            EndpointType::Interrupt => {
                if ep_dir == UsbDirection::Out {
                    EpType::IntrOutbound
                } else {
                    EpType::IntrInbound
                }
            }
            EndpointType::Bulk => {
                if ep_dir == UsbDirection::Out {
                    EpType::BulkOutbound
                } else {
                    EpType::BulkInbound
                }
            }
            EndpointType::Isochronous => {
                if ep_dir == UsbDirection::Out {
                    EpType::IsochOutbound
                } else {
                    EpType::IsochInbound
                }
            }
        };

        self.core().max_packet_size[pei] = Some(max_packet_size as usize);
        if allocated_ep != 0 {
            // also record metadata for non-0 EPs
            self.ep_meta[pei - 2] = Some((hw_ep_type, max_packet_size as usize));
        }

        Ok(EndpointAddress::from_parts(allocated_ep, ep_dir))
    }

    /// Enables and initializes the USB peripheral. Soon after enabling the device will be reset, so
    /// there is no need to perform a USB reset in this method.
    fn enable(&mut self) {
        #[cfg(feature = "verbose-debug")]
        crate::println!(" ******** enable");
    }

    /// Called when the host resets the device. This will be soon called after
    /// [`poll`](crate::device::UsbDevice::poll) returns [`PollResult::Reset`]. This method should
    /// reset the state of all endpoints and peripheral flags back to a state suitable for
    /// enumeration, as well as ensure that all endpoints previously allocated with alloc_ep are
    /// initialized as specified.
    fn reset(&self) {
        self.address_is_set.store(false, Ordering::SeqCst);
        crate::println!(" ******** reset");
        let irq_csr = {
            let mut hw = self.core();
            // disable IRQs
            hw.irq_csr.wo(utralib::utra::irqarray1::EV_ENABLE, 0);
            hw.reset();
            hw.init();
            hw.start();
            hw.update_current_speed();
            // IRQ enable must happen without dependency on the hardware lock
            hw.irq_csr.clone()
        };
        // the lock is released, now we can enable irqs
        irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xffff_ffff); // blanket clear
        irq_csr.wo(utralib::utra::irqarray1::EV_ENABLE, 3); // FIXME: hard coded value that enables CORIGINE_IRQ_MASK | SW_IRQ_MASK

        // TODO -- figure out what this means
        // self.force_reset().ok();
    }

    /// Sets the device USB address to `addr`.
    fn set_device_address(&self, addr: u8) {
        crate::println!("set address {}", addr);
        #[cfg(feature = "verbose-debug")]
        crate::println!(" ******** set address");
        self.core().set_addr(addr, CRG_INT_TARGET);
        self.address_is_set.store(true, Ordering::SeqCst);

        // this core has a quirk that you can't actually enable the endpoints until *after* the address
        // has been set. :-/
        for (index, &maybe_ep) in self.ep_meta.iter().enumerate() {
            if let Some((hw_ep_type, max_packet_size)) = maybe_ep {
                let pei = index + 2;
                let dir = CorigineUsb::pei_to_dir(pei);
                crate::println!("enabling pei {}, dir {:?}", pei, dir);
                self.core().ep_enable(CorigineUsb::pei_to_ep(pei), dir, max_packet_size as u16, hw_ep_type);

                // If the end point is an OUT, set up a standing transfer to receive the incoming packet
                /*
                if dir == CRG_OUT {
                    crate::println!("setting up standing transfers");
                    let addr = self
                        .core()
                        .get_app_buf_ptr(CorigineUsb::pei_to_ep(pei), dir)
                        .expect("should always be a buffer available at set_address");
                    crate::println!("app pointer at {:x}", addr);
                    match hw_ep_type {
                        // NOTE: we kind of need to know how big of a transfer we expect for this
                        // to work, otherwise, the transfer may not complete. But we won't know
                        // this until "read" is called by the USB stack implementation, so...how to
                        // break this dependency??
                        //
                        // For now the code just sets the size to the whole buffer size but this should
                        // cause the stack to not respond to packets that fail to meet the total expected
                        // length.
                        EpType::BulkOutbound => {
                            crate::println!("enabling bulk out");
                            self.core().bulk_xfer(
                                CorigineUsb::pei_to_ep(pei),
                                dir,
                                addr,
                                CRG_UDC_APP_BUF_LEN,
                                CRG_INT_TARGET,
                                CRG_XFER_SET_CHAIN,
                            );
                        }
                        EpType::BulkInbound => {
                            crate::println!("bulk in UNREACHABLE");
                            unreachable!("Bulk inbound not reachable for OUT endpoints")
                        }
                        _ => {
                            crate::println!("enabling interrupt pei {}", pei);
                            self.core().ep_xfer(
                                CorigineUsb::pei_to_ep(pei),
                                dir,
                                addr,
                                CRG_UDC_APP_BUF_LEN,
                                CRG_INT_TARGET,
                                false,
                                false,
                                false,
                            );
                        }
                    }
                }
                */
            } else {
                let pei = index + 2;
                let dir = CorigineUsb::pei_to_dir(pei);
                crate::println!("disabling unused pei {} dir {:?}", pei, dir);
                self.core().ep_disable(CorigineUsb::pei_to_ep(pei), dir);
            }
            crate::println!("done with index {}", index);
        }
        crate::println!("enabled EPs: {:b}", self.core().csr.r(EPENABLE));
        crate::println!("running EPs: {:b}", self.core().csr.r(EPRUNNING));
    }

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
    fn write(&self, ep_addr: EndpointAddress, buf: &[u8]) -> Result<usize> {
        if ep_addr.index() == 0 {
            // #[cfg(feature = "verbose-debug")]
            crate::println!(" ******* EP0 WRITE ({}): {:x?}", buf.len(), &buf[..8.min(buf.len())]);
            let ep0_buf_addr = self.core().ep0_buf.load(Ordering::SeqCst) as usize;
            let ep0_buf =
                unsafe { core::slice::from_raw_parts_mut(ep0_buf_addr as *mut u8, CRG_UDC_EP0_REQBUFSIZE) };
            ep0_buf[..buf.len()].copy_from_slice(buf);
            if buf.len() != 64 {
                self.core().ep0_send(ep0_buf_addr, buf.len(), 0);
            } else {
                // a 64-length packet cannot be the last packet sent, there must be a short packet afterwards.
                // so, enqueue() the packet (which differs from send() in that it leaves the pointers in a
                // state expecting a "next" packet).
                self.core().ep0_enqueue(ep0_buf_addr, buf.len(), 0);
            }
            Ok(buf.len())
        } else {
            // TODO: resolve ep_addr to an ep_type, so we can handle both bulk and intr. But for now, let's
            // just do intr since that's all we're using for testing.

            // #[cfg(feature = "verbose-debug")]
            crate::println!(
                " ******** WRITE: {:?}({}): {:x?}",
                ep_addr.index(),
                buf.len(),
                &buf[..8.min(buf.len())]
            );
            let addr = if let Some(addr) = self.core().get_app_buf_ptr(ep_addr.index() as u8, USB_RECV) {
                addr
            } else {
                #[cfg(feature = "verbose-debug")]
                crate::println!("would block");
                return Err(UsbError::WouldBlock);
            };
            let hw_buf = unsafe { core::slice::from_raw_parts_mut(addr as *mut u8, CRG_UDC_APP_BUF_LEN) };
            assert!(buf.len() < CRG_UDC_APP_BUF_LEN, "write buffer size exceeded");
            hw_buf[..buf.len()].copy_from_slice(&buf);
            self.core().ep_xfer(
                ep_addr.index() as u8,
                CRG_IN,
                addr,
                buf.len(),
                CRG_INT_TARGET,
                false,
                false,
                false,
            );
            #[cfg(feature = "verbose-debug")]
            crate::println!("ep{} initiated {}", ep_addr.index(), buf.len());
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
    fn read(&self, ep_addr: EndpointAddress, buf: &mut [u8]) -> Result<usize> {
        if ep_addr.index() == 0 {
            // #[cfg(feature = "verbose-debug")]
            crate::println!(" ******** EP0 READ");
            // setup packet
            if let Some(setup) = self.core().setup.take() {
                buf[..setup.len()].copy_from_slice(&setup);
                // #[cfg(feature = "verbose-debug")]
                crate::println!("   {:x?}", &buf[..setup.len()]);
                Ok(setup.len())
            } else {
                crate::println!("   empty setup");
                Ok(0)
            }
        } else {
            if self.address_is_set.load(Ordering::SeqCst) {
                #[cfg(feature = "verbose-debug")]
                crate::println!(" ******** READ {}", ep_addr.index());
                let pending_ep = self.core().pending_ep();
                let ret = if let Some(pending_ep) = pending_ep {
                    if pending_ep as usize == ep_addr.index() {
                        let app_ptr = self.core().app_ptr.take().expect("inconistent app_ptr state");
                        let ptr = app_ptr.addr;
                        let len = app_ptr.len;
                        self.ep_out_ready[ep_addr.index()].store(false, Ordering::SeqCst);
                        let app_buf = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
                        if buf.len() < app_buf.len() {
                            crate::println!(
                                "overflow: app_buf.len() {} >= buf.len() {}",
                                app_buf.len(),
                                buf.len()
                            );
                            Err(UsbError::BufferOverflow)
                        } else {
                            #[cfg(feature = "verbose-debug")]
                            crate::println!("copy into len {} from len {}", buf.len(), app_buf.len());
                            buf[..app_buf.len()].copy_from_slice(app_buf);
                            #[cfg(feature = "verbose-debug")]
                            crate::println!("   {:x?}", &buf[..app_buf.len().min(8)]);
                            Ok(app_buf.len())
                        }
                    } else {
                        // crate::println!("read would block");
                        Err(UsbError::WouldBlock)
                    }
                } else {
                    Err(UsbError::WouldBlock)
                };

                if self.ep_out_ready[ep_addr.index()].swap(true, Ordering::SeqCst) == false {
                    // crate::println!("get address");
                    // release lock on core after getting the address
                    let addr = {
                        self.core()
                            .get_app_buf_ptr(ep_addr.index() as u8, CRG_OUT)
                            .expect("should always be a buffer available at set_address")
                    };
                    // crate::println!("address {:x}", addr);
                    if let Some((hw_ep_type, max_packet_size)) = self.ep_meta[ep_addr.index()] {
                        match hw_ep_type {
                            // NOTE: we kind of need to know how big of a transfer we expect for this
                            // to work, otherwise, the transfer may not complete. But we won't know
                            // this until "read" is called by the USB stack implementation, so...how to
                            // break this dependency??
                            //
                            // For now the code just sets the size to the whole buffer size but this should
                            // cause the stack to not respond to packets that fail to meet the total expected
                            // length.
                            EpType::BulkOutbound => {
                                self.core().bulk_xfer(
                                    ep_addr.index() as u8,
                                    CRG_OUT,
                                    addr,
                                    CRG_UDC_APP_BUF_LEN.min(buf.len()).min(max_packet_size),
                                    CRG_INT_TARGET,
                                    CRG_XFER_SET_CHAIN,
                                );
                            }
                            EpType::BulkInbound => {
                                crate::println!("bulk in UNREACHABLE");
                                unreachable!("Bulk inbound not reachable for OUT endpoints");
                            }
                            _ => {
                                // crate::println!("ep_type {:?}", hw_ep_type);
                                self.core().ep_xfer(
                                    ep_addr.index() as u8,
                                    CRG_OUT,
                                    addr,
                                    CRG_UDC_APP_BUF_LEN.min(buf.len()).min(max_packet_size),
                                    CRG_INT_TARGET,
                                    false,
                                    false,
                                    false,
                                );
                            }
                        }
                    } else {
                        crate::println!("EP{} has no metadata!", ep_addr.index());
                    }
                }
                ret
            } else {
                Err(UsbError::WouldBlock)
            }
        }
    }

    /// Sets or clears the STALL condition for an endpoint. If the endpoint is an OUT endpoint, it
    /// should be prepared to receive data again.
    fn set_stalled(&self, _ep_addr: EndpointAddress, _stalled: bool) {
        /*
        // #[cfg(feature = "verbose-debug")]
        crate::println!(
            " ******* set stalled {:?}<-{:?}, {}",
            ep_addr.index(),
            stalled,
            if ep_addr.is_in() { "OUT" } else { "IN" }
        );
        self.core().handle_set_stalled(ep_addr.index() as u8, ep_addr.is_in(), stalled);
        */
    }

    /// Gets whether the STALL condition is set for an endpoint.
    fn is_stalled(&self, ep_addr: EndpointAddress) -> bool {
        crate::println!(" ******* is_stalled");
        let pei = CorigineUsb::pei(ep_addr.index() as u8, ep_addr.is_in());
        self.core().udc_ep[pei].ep_state == EpState::Halted
    }

    /// Instruct EP0 to configure itself with an OUT descriptor, so that it may receive a STATUS
    /// update during configuration. This is for devices that support an EP0 which can only either
    /// be IN or OUT, but not both at the same time. Devices with both IN/OUT may leave this as
    /// an empty stub.
    fn set_ep0_out(&self) {
        let addr = self.core().ep0_buf.load(Ordering::SeqCst) as usize;
        self.core().ep0_receive(addr, 64, 0);
    }

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
        match self.core().event_inner.take() {
            Some(e) => {
                match e {
                    CrgEvent::None => PollResult::None,
                    CrgEvent::Connect => {
                        /*
                        let mut hw = self.hw.lock().unwrap();
                        hw.reset();
                        hw.init();
                        hw.start(); */
                        PollResult::Reset
                    }
                    CrgEvent::Data(ep_out, ep_in_complete, ep_setup) => {
                        PollResult::Data { ep_out, ep_in_complete, ep_setup }
                    }
                    CrgEvent::Error => {
                        crate::println!("Error detected in poll, issuing reset");
                        PollResult::Reset
                    }
                }
            }
            None => PollResult::None,
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
        crate::println!(" ******* force_reset");

        self.address_is_set.store(false, Ordering::SeqCst);
        for eor in self.ep_out_ready.iter() {
            eor.store(false, Ordering::SeqCst);
        }
        crate::println!(" ******** reset");
        let irq_csr = {
            let mut hw = self.core();
            // disable IRQs
            hw.irq_csr.wo(utralib::utra::irqarray1::EV_ENABLE, 0);
            hw.reset();
            hw.init();
            hw.start();
            hw.update_current_speed();
            // IRQ enable must happen without dependency on the hardware lock
            hw.irq_csr.clone()
        };
        // the lock is released, now we can enable irqs
        irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xffff_ffff); // blanket clear
        irq_csr.wo(utralib::utra::irqarray1::EV_ENABLE, 3); // FIXME: hard coded value that enables CORIGINE_IRQ_MASK | SW_IRQ_MASK

        Ok(())
    }
}

pub fn handle_event(this: &mut CorigineUsb, event_trb: &mut EventTrbS) -> CrgEvent {
    #[cfg(feature = "verbose-debug")]
    crate::println!("handle_event: {:x?}", event_trb);
    let pei = event_trb.get_endpoint_id();
    let ep_num = pei >> 1;
    let udc_ep = &mut this.udc_ep[pei as usize];
    let mut ret = CrgEvent::None;
    match event_trb.get_trb_type() {
        TrbType::EventPortStatusChange => {
            let portsc_val = this.csr.r(PORTSC);
            this.csr.wo(PORTSC, portsc_val);
            // this.print_status(portsc_val);

            let portsc = PortSc(portsc_val);
            #[cfg(feature = "verbose-debug")]
            crate::println!("{:?}", portsc);

            if portsc.prc() && !portsc.pr() {
                #[cfg(feature = "std")]
                crate::println!("update_current_speed() - reset done");
                this.update_current_speed();
            }
            if portsc.csc() && portsc.ppc() && portsc.pp() && portsc.ccs() {
                #[cfg(feature = "std")]
                crate::println!("update_current_speed() - cable connect");
                this.update_current_speed();
            }
            /*
            let cs = (portsc_val & this.csr.ms(PORTSC_CCS, 1)) != 0;
            let pp = (portsc_val & this.csr.ms(PORTSC_PP, 1)) != 0;
            #[cfg(feature = "std")]
            crate::println!("  {:x} {:x?} PORT_STATUS_CHANGE", portsc_val, event_trb.dw3);

            if portsc_val & this.csr.ms(PORTSC_CSC, 1) != 0 {
                if cs {
                    #[cfg(not(feature = "std"))]
                    println!("  Port connection");
                    #[cfg(feature = "std")]
                    crate::println!("  Port connection");
                    // ret = CrgEvent::Connect;
                } else {
                    #[cfg(not(feature = "std"))]
                    println!("  Port disconnection");
                    #[cfg(feature = "std")]
                    crate::println!("  Port disconnection");
                }
            }

            if portsc_val & this.csr.ms(PORTSC_PPC, 1) != 0 {
                if pp {
                    #[cfg(not(feature = "std"))]
                    println!("  Power present");
                    #[cfg(feature = "std")]
                    crate::println!("  Power present");
                    // ret = CrgEvent::None;
                } else {
                    #[cfg(not(feature = "std"))]
                    println!("  Power not present");
                    #[cfg(feature = "std")]
                    crate::println!("  Power not present");
                }
            }

            if (portsc_val & this.csr.ms(PORTSC_CSC, 1) != 0)
                && (portsc_val & this.csr.ms(PORTSC_PPC, 1) != 0)
            {
                if cs && pp {
                    #[cfg(not(feature = "std"))]
                    println!("  Cable connect and power present");
                    #[cfg(feature = "std")]
                    crate::println!("  Cable connect and power present");
                    this.update_current_speed();
                    // ret = CrgEvent::None;
                }
            }

            if (portsc_val & this.csr.ms(PORTSC_PRC, 1)) != 0 {
                if portsc_val & this.csr.ms(PORTSC_PR, 1) != 0 {
                    #[cfg(not(feature = "std"))]
                    println!("  In port reset process");
                    #[cfg(feature = "std")]
                    crate::println!("  In port reset process");
                } else {
                    #[cfg(not(feature = "std"))]
                    println!("  Port reset done");
                    #[cfg(feature = "std")]
                    crate::println!("  Port reset done");
                    this.update_current_speed();
                    ret = CrgEvent::Connect;
                }
            }

            if (portsc_val & this.csr.ms(PORTSC_PLC, 1)) != 0 {
                #[cfg(not(feature = "std"))]
                println!("  Port link state change: {:?}", PortLinkState::from_portsc(portsc_val));
                #[cfg(feature = "std")]
                crate::println!("  Port link state change: {:?}", PortLinkState::from_portsc(portsc_val));
            }

            if !cs && !pp {
                #[cfg(not(feature = "std"))]
                println!("  cable disconnect and power not present");
                #[cfg(feature = "std")]
                crate::println!("  cable disconnect and power not present");
            }
            */
            this.csr.rmwf(EVENTCONFIG_SETUP_ENABLE, 1);
        }
        TrbType::EventTransfer => {
            let comp_code =
                CompletionCode::try_from(event_trb.dw2.compl_code()).expect("Invalid completion code");

            // update the dequeue pointer
            #[cfg(feature = "verbose-debug")]
            crate::println!("event_transfer {:x?}", event_trb);
            let deq_pt =
                unsafe { (event_trb.dw0 as *mut TransferTrbS).add(1).as_mut().expect("Couldn't deref ptr") };
            if deq_pt.get_trb_type() == TrbType::Link {
                udc_ep.deq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
            } else {
                udc_ep.deq_pt = AtomicPtr::new(deq_pt as *mut TransferTrbS);
            }
            #[cfg(feature = "verbose-debug")]
            crate::println!("EventTransfer: comp_code {:?}, PEI {}", comp_code, pei);

            let dir = (pei & 1) != 0;
            if pei == 0 {
                if comp_code == CompletionCode::Success {
                    // ep0_xfer_complete
                    if dir == USB_SEND {
                        ret = CrgEvent::Data(0, 1, 0); // FIXME: this ordering contradicts the `dir` bit, but seems necessary to trigger the next packet send
                    } else {
                        ret = CrgEvent::Data(1, 0, 0); // FIXME: this ordering contradicts the `dir` bit, but seems necessary to trigger the next packet send
                    }
                } else {
                    #[cfg(feature = "verbose-debug")]
                    crate::println!("EP0 unhandled comp_code: {:?}", comp_code);
                    ret = CrgEvent::None;
                }
            } else if pei >= 2 {
                if comp_code == CompletionCode::Success || comp_code == CompletionCode::ShortPacket {
                    #[cfg(feature = "verbose-debug")]
                    crate::println!("EP{} xfer event, dir {}", ep_num, if dir { "OUT" } else { "IN" });
                    // xfer_complete
                    if dir == CRG_OUT {
                        let addr = this.retire_app_buf_ptr(ep_num, dir);
                        let mps =
                            this.max_packet_size[pei as usize].expect("max packet size was not initialized!");
                        let hw_buf = unsafe { core::slice::from_raw_parts_mut(addr as *mut u8, mps) };
                        // copy the whole hardware buffer contents -- even if it's bogus
                        let mut storage = [0u8; CRG_UDC_APP_BUF_LEN];
                        storage[..mps].copy_from_slice(hw_buf);
                        this.readout[ep_num as usize - 1] = Some(storage);
                        // re-enqueue the listener
                        let addr =
                            this.get_app_buf_ptr(ep_num, dir).expect("retire should have opened an entry");
                        this.ep_xfer(
                            ep_num,
                            dir,
                            addr,
                            CRG_UDC_APP_BUF_LEN,
                            CRG_INT_TARGET,
                            false,
                            false,
                            false,
                        );
                        ret = CrgEvent::Data(ep_num as u16, 0, 0);
                    } else {
                        this.retire_app_buf_ptr(ep_num, dir);
                        ret = CrgEvent::Data(0, ep_num as u16, 0);
                    }
                } else if comp_code == CompletionCode::MissedServiceError {
                    #[cfg(feature = "std")]
                    crate::println!("MissedServiceError");
                } else {
                    #[cfg(feature = "std")]
                    crate::println!("EventTransfer {:?} event not handled", comp_code);
                }
            }
        }
        TrbType::SetupPkt => {
            #[cfg(feature = "verbose-debug")]
            crate::println!("  handle_setup_pkt");
            let mut setup_storage = [0u8; 8];
            setup_storage.copy_from_slice(&event_trb.get_raw_setup());
            this.setup = Some(setup_storage);
            this.setup_tag = event_trb.get_setup_tag();

            // demo of setup packets working in loader
            #[cfg(not(feature = "std"))]
            {
                let _request_type = setup_storage[0];
                let request = setup_storage[1];
                let value = u16::from_le_bytes(setup_storage[2..4].try_into().unwrap());
                let _index = u16::from_le_bytes(setup_storage[4..6].try_into().unwrap());
                let _length = u16::from_le_bytes(setup_storage[6..].try_into().unwrap());

                const SET_ADDRESS: u8 = 5;
                const GET_DESCRIPTOR: u8 = 6;

                match request {
                    SET_ADDRESS => {
                        this.set_addr(value as u8, 0);
                        println!("address set");
                    }
                    GET_DESCRIPTOR => {
                        let base_ptr = crate::usb::driver::CRG_UDC_MEMBASE + CRG_UDC_EP0_BUF_OFFSET;
                        let ep0_buf = base_ptr as *mut u8;
                        let desc = [
                            0x12u8, 0x1, 0x10, 0x2, 0, 0, 0, 0x8, // pkt 0
                            0x9, 0x12, 0x13, 0x36, 0x10, 0, 0x1, 0x2, // pkt 1
                            0x3, 0x1, // pkt 2
                        ];
                        // [12, 1, 10, 2, 0, 0, 0, 8]
                        println!("ep0 send {}", desc.len());
                        let mut enq_pt = unsafe {
                            this.udc_ep[0]
                                .enq_pt
                                .load(Ordering::SeqCst)
                                .as_mut()
                                .expect("couldn't deref pointer")
                        };
                        let mut pcs = this.udc_ep[0].pcs;
                        let tag = this.setup_tag;
                        for (j, chunk) in desc.chunks(8).enumerate() {
                            for (i, &d) in chunk.iter().enumerate() {
                                unsafe { ep0_buf.add(i + j * 8).write_volatile(d) };
                            }
                            // this.ep0_send(base_ptr, chunk.len(), 0);
                            enq_pt.control_data_trb(
                                unsafe { ep0_buf.add(j * 8) } as u32,
                                pcs,
                                1,
                                chunk.len() as u32,
                                0,
                                true,
                                false,
                                false,
                                tag,
                                CRG_INT_TARGET,
                            );
                            (enq_pt, pcs) = this.udc_ep[0].increment_enq_pt();
                            // this.knock_doorbell(0);
                            compiler_fence(Ordering::SeqCst);
                            this.csr.wfo(DOORBELL_TARGET, 0);
                        }
                        enq_pt.control_status_trb(pcs, false, false, tag, CRG_INT_TARGET, USB_RECV);
                        let (_enq_pt, _pcs) = this.udc_ep[0].increment_enq_pt();
                        this.knock_doorbell(0);
                        println!("ep0 sent");
                    }
                    _ => {
                        println!("A request was not handled {:x}", request);
                    }
                }
            }

            ret = CrgEvent::Data(0, 0, 1);
        }
        TrbType::DataStage => {
            panic!("data stage needs handling");
        }
        _ => {
            println!("Unexpected trb_type {:?}", event_trb.get_trb_type());
        }
    }
    ret
}

pub fn handle_event_inner(this: &mut CorigineUsb, event_trb: &mut EventTrbS) {
    #[cfg(feature = "verbose-debug")]
    crate::println!("handle_event: {:x?}", event_trb);
    let pei = event_trb.get_endpoint_id();
    let udc_ep = &mut this.udc_ep[pei as usize];
    let mut ret = CrgEvent::None;
    match event_trb.get_trb_type() {
        TrbType::EventPortStatusChange => {
            let portsc_val = this.csr.r(PORTSC);
            this.csr.wo(PORTSC, portsc_val);
            // this.print_status(portsc_val);

            let portsc = PortSc(portsc_val);
            #[cfg(feature = "verbose-debug")]
            crate::println!("{:?}", portsc);

            if portsc.prc() && !portsc.pr() {
                #[cfg(feature = "std")]
                crate::println!("update_current_speed() - reset done");
                this.update_current_speed();
            }
            if portsc.csc() && portsc.ppc() && portsc.pp() && portsc.ccs() {
                #[cfg(feature = "std")]
                crate::println!("update_current_speed() - cable connect");
                this.update_current_speed();
            }
            this.csr.rmwf(EVENTCONFIG_SETUP_ENABLE, 1);
        }
        TrbType::EventTransfer => {
            let comp_code =
                CompletionCode::try_from(event_trb.dw2.compl_code()).expect("Invalid completion code");

            // update the dequeue pointer
            #[cfg(feature = "verbose-debug")]
            crate::println!("event_transfer {:x?}", event_trb);
            let deq_pt =
                unsafe { (event_trb.dw0 as *mut TransferTrbS).add(1).as_mut().expect("Couldn't deref ptr") };
            if deq_pt.get_trb_type() == TrbType::Link {
                udc_ep.deq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
            } else {
                udc_ep.deq_pt = AtomicPtr::new(deq_pt as *mut TransferTrbS);
            }
            #[cfg(feature = "verbose-debug")]
            crate::println!("EventTransfer: comp_code {:?}, PEI {}", comp_code, pei);

            let dir = (pei & 1) != 0;
            if pei == 0 {
                if comp_code == CompletionCode::Success {
                    // ep0_xfer_complete
                    if dir == USB_SEND {
                        // (out, in_complete, setup)
                        ret = CrgEvent::Data(0, 1, 0); // FIXME: this ordering contradicts the `dir` bit, but seems necessary to trigger the next packet send
                    } else {
                        ret = CrgEvent::Data(1, 0, 0); // FIXME: this ordering contradicts the `dir` bit, but seems necessary to trigger the next packet send
                    }
                } else {
                    #[cfg(feature = "verbose-debug")]
                    crate::println!("EP0 unhandled comp_code: {:?}", comp_code);
                    ret = CrgEvent::None;
                }
            } else if pei >= 2 {
                if comp_code == CompletionCode::Success || comp_code == CompletionCode::ShortPacket {
                    let ep = pei as u16 / 2;
                    let ep_onehot = 1u16 << ep;
                    crate::println!(
                        "EP{}[{:x}] xfer event, dir {}",
                        ep,
                        ep_onehot,
                        if dir { "OUT" } else { "IN" }
                    );
                    // xfer_complete

                    // so unsafe. so unsafe. We're counting on the hardware to hand us a raw pointer
                    // that isn't corrupted.
                    let p_trb = unsafe { &*(event_trb.dw0 as *const TransferTrbS) };
                    this.app_ptr = Some(AppPtr {
                        addr: p_trb.dplo as usize,
                        len: (p_trb.dw2.0 & 0xffff) as usize,
                        ep: ep as u8,
                    });
                    if dir {
                        // out
                        ret = CrgEvent::Data(ep_onehot, 0, 0)
                    } else {
                        // in
                        ret = CrgEvent::Data(0, ep_onehot, 0)
                    }
                } else if comp_code == CompletionCode::MissedServiceError {
                    #[cfg(feature = "std")]
                    crate::println!("MissedServiceError");
                } else {
                    #[cfg(feature = "std")]
                    crate::println!("EventTransfer {:?} event not handled", comp_code);
                }
            }
        }
        TrbType::SetupPkt => {
            #[cfg(feature = "verbose-debug")]
            crate::println!("  handle_setup_pkt");
            let mut setup_storage = [0u8; 8];
            setup_storage.copy_from_slice(&event_trb.get_raw_setup());
            this.setup = Some(setup_storage);
            this.setup_tag = event_trb.get_setup_tag();
            ret = CrgEvent::Data(0, 0, 1);
        }
        TrbType::DataStage => {
            panic!("data stage needs handling");
        }
        _ => {
            println!("Unexpected trb_type {:?}", event_trb.get_trb_type());
        }
    }
    #[cfg(feature = "verbose-debug")]
    crate::println!("handle_event_inner: {:?}", ret);
    this.event_inner = Some(ret)
}
