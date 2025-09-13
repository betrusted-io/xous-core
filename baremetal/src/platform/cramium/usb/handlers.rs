use core::sync::atomic::Ordering;

use cramium_hal::usb::driver::CorigineUsb;
use cramium_hal::usb::driver::*;

use super::*;

pub static TX_IDLE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

// Locate the "disk"
pub(crate) const RAMDISK_ADDRESS: usize = crate::platform::HEAP_START + crate::platform::HEAP_LEN;
pub(crate) const STACK_SIZE: usize = 128 * 1024; // 128k for stack is enough? maybe? if the ramdisk overflows, it smashes stack - dangerous!
pub(crate) const RAMDISK_LEN: usize =
    crate::platform::RAM_SIZE - (RAMDISK_ADDRESS - crate::platform::RAM_BASE) - STACK_SIZE;
pub(crate) const SECTOR_SIZE: u16 = 512;

// MBR template
// 0x0b~0x0C 2 bytes means block size, default 0x200 bytes
// 0x20~0x23 4 bytes means block number, default 0x400 block
#[rustfmt::skip] // keep this in 16-byte width
pub(crate) const MBR_TEMPLATE: [u8; SECTOR_SIZE as usize] = [
    0xEB, 0x3C, 0x90, 0x4D, 0x53, 0x44, 0x4F, 0x53, 0x35, 0x2E, 0x30, 0x00, 0x02, 0x20, 0x06, 0x00,
    0x02, 0x00, 0x02, 0x00, 0x0C, 0xF8, 0x01, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x29, 0xBB, 0xBF, 0xB7, 0xB0, 0x4E, 0x4F, 0x20, 0x4E, 0x41,
    0x4D, 0x45, 0x20, 0x20, 0x20, 0x20, 0x46, 0x41, 0x54, 0x31, 0x32, 0x20, 0x20, 0x20, 0x33, 0xC9,
    0x8E, 0xD1, 0xBC, 0xF0, 0x7B, 0x8E, 0xD9, 0xB8, 0x00, 0x20, 0x8E, 0xC0, 0xFC, 0xBD, 0x00, 0x7C,
    0x38, 0x4E, 0x24, 0x7D, 0x24, 0x8B, 0xC1, 0x99, 0xE8, 0x3C, 0x01, 0x72, 0x1C, 0x83, 0xEB, 0x3A,
    0x66, 0xA1, 0x1C, 0x7C, 0x26, 0x66, 0x3B, 0x07, 0x26, 0x8A, 0x57, 0xFC, 0x75, 0x06, 0x80, 0xCA,
    0x02, 0x88, 0x56, 0x02, 0x80, 0xC3, 0x10, 0x73, 0xEB, 0x33, 0xC9, 0x8A, 0x46, 0x10, 0x98, 0xF7,
    0x66, 0x16, 0x03, 0x46, 0x1C, 0x13, 0x56, 0x1E, 0x03, 0x46, 0x0E, 0x13, 0xD1, 0x8B, 0x76, 0x11,
    0x60, 0x89, 0x46, 0xFC, 0x89, 0x56, 0xFE, 0xB8, 0x20, 0x00, 0xF7, 0xE6, 0x8B, 0x5E, 0x0B, 0x03,
    0xC3, 0x48, 0xF7, 0xF3, 0x01, 0x46, 0xFC, 0x11, 0x4E, 0xFE, 0x61, 0xBF, 0x00, 0x00, 0xE8, 0xE6,
    0x00, 0x72, 0x39, 0x26, 0x38, 0x2D, 0x74, 0x17, 0x60, 0xB1, 0x0B, 0xBE, 0xA1, 0x7D, 0xF3, 0xA6,
    0x61, 0x74, 0x32, 0x4E, 0x74, 0x09, 0x83, 0xC7, 0x20, 0x3B, 0xFB, 0x72, 0xE6, 0xEB, 0xDC, 0xA0,
    0xFB, 0x7D, 0xB4, 0x7D, 0x8B, 0xF0, 0xAC, 0x98, 0x40, 0x74, 0x0C, 0x48, 0x74, 0x13, 0xB4, 0x0E,
    0xBB, 0x07, 0x00, 0xCD, 0x10, 0xEB, 0xEF, 0xA0, 0xFD, 0x7D, 0xEB, 0xE6, 0xA0, 0xFC, 0x7D, 0xEB,
    0xE1, 0xCD, 0x16, 0xCD, 0x19, 0x26, 0x8B, 0x55, 0x1A, 0x52, 0xB0, 0x01, 0xBB, 0x00, 0x00, 0xE8,
    0x3B, 0x00, 0x72, 0xE8, 0x5B, 0x8A, 0x56, 0x24, 0xBE, 0x0B, 0x7C, 0x8B, 0xFC, 0xC7, 0x46, 0xF0,
    0x3D, 0x7D, 0xC7, 0x46, 0xF4, 0x29, 0x7D, 0x8C, 0xD9, 0x89, 0x4E, 0xF2, 0x89, 0x4E, 0xF6, 0xC6,
    0x06, 0x96, 0x7D, 0xCB, 0xEA, 0x03, 0x00, 0x00, 0x20, 0x0F, 0xB6, 0xC8, 0x66, 0x8B, 0x46, 0xF8,
    0x66, 0x03, 0x46, 0x1C, 0x66, 0x8B, 0xD0, 0x66, 0xC1, 0xEA, 0x10, 0xEB, 0x5E, 0x0F, 0xB6, 0xC8,
    0x4A, 0x4A, 0x8A, 0x46, 0x0D, 0x32, 0xE4, 0xF7, 0xE2, 0x03, 0x46, 0xFC, 0x13, 0x56, 0xFE, 0xEB,
    0x4A, 0x52, 0x50, 0x06, 0x53, 0x6A, 0x01, 0x6A, 0x10, 0x91, 0x8B, 0x46, 0x18, 0x96, 0x92, 0x33,
    0xD2, 0xF7, 0xF6, 0x91, 0xF7, 0xF6, 0x42, 0x87, 0xCA, 0xF7, 0x76, 0x1A, 0x8A, 0xF2, 0x8A, 0xE8,
    0xC0, 0xCC, 0x02, 0x0A, 0xCC, 0xB8, 0x01, 0x02, 0x80, 0x7E, 0x02, 0x0E, 0x75, 0x04, 0xB4, 0x42,
    0x8B, 0xF4, 0x8A, 0x56, 0x24, 0xCD, 0x13, 0x61, 0x61, 0x72, 0x0B, 0x40, 0x75, 0x01, 0x42, 0x03,
    0x5E, 0x0B, 0x49, 0x75, 0x06, 0xF8, 0xC3, 0x41, 0xBB, 0x00, 0x00, 0x60, 0x66, 0x6A, 0x00, 0xEB,
    0xB0, 0x42, 0x4F, 0x4F, 0x54, 0x4D, 0x47, 0x52, 0x20, 0x20, 0x20, 0x20, 0x0D, 0x0A, 0x52, 0x65,
    0x6D, 0x6F, 0x76, 0x65, 0x20, 0x64, 0x69, 0x73, 0x6B, 0x73, 0x20, 0x6F, 0x72, 0x20, 0x6F, 0x74,
    0x68, 0x65, 0x72, 0x20, 0x6D, 0x65, 0x64, 0x69, 0x61, 0x2E, 0xFF, 0x0D, 0x0A, 0x44, 0x69, 0x73,
    0x6B, 0x20, 0x65, 0x72, 0x72, 0x6F, 0x72, 0xFF, 0x0D, 0x0A, 0x50, 0x72, 0x65, 0x73, 0x73, 0x20,
    0x61, 0x6E, 0x79, 0x20, 0x6B, 0x65, 0x79, 0x20, 0x74, 0x6F, 0x20, 0x72, 0x65, 0x73, 0x74, 0x61,
    0x72, 0x74, 0x0D, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xAC, 0xCB, 0xD8, 0x55, 0xAA,
];

pub(crate) const FAT_TABLE: [u8; 0x08] = [0xF8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
pub(crate) const TABLE_OFFSET_FAT1: usize = 0xC00;
pub(crate) const TABLE_OFFSET_FAT2: usize = 0xE00;

#[rustfmt::skip] // keep this in 16-byte width
pub (crate) const ROOT_DIR: [u8; 0x80] = [
    0x42, 0x41, 0x4F, 0x53, 0x45, 0x43, 0x20, 0x20, 0x20, 0x20, 0x20, 0x08, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xCD, 0x78, 0x74, 0x59, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x42, 0x20, 0x00, 0x49, 0x00, 0x6E, 0x00, 0x66, 0x00, 0x6F, 0x00, 0x0F, 0x00, 0x72, 0x72, 0x00,
    0x6D, 0x00, 0x61, 0x00, 0x74, 0x00, 0x69, 0x00, 0x6F, 0x00, 0x00, 0x00, 0x6E, 0x00, 0x00, 0x00,
    0x01, 0x53, 0x00, 0x79, 0x00, 0x73, 0x00, 0x74, 0x00, 0x65, 0x00, 0x0F, 0x00, 0x72, 0x6D, 0x00,
    0x20, 0x00, 0x56, 0x00, 0x6F, 0x00, 0x6C, 0x00, 0x75, 0x00, 0x00, 0x00, 0x6D, 0x00, 0x65, 0x00,
    0x53, 0x59, 0x53, 0x54, 0x45, 0x4D, 0x7E, 0x31, 0x20, 0x20, 0x20, 0x16, 0x00, 0x36, 0xCF, 0x78,
    0x74, 0x59, 0x74, 0x59, 0x00, 0x00, 0xD0, 0x78, 0x74, 0x59, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
];
pub(crate) const ROOT_DIR_OFFSET: usize = 0x1000;

#[rustfmt::skip] // keep this in 16-byte width
pub (crate) const FAT_DATA1: [u8; 0x100] = [
    0x2E, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x10, 0x00, 0x36, 0xCF, 0x78,
    0x74, 0x59, 0x74, 0x59, 0x00, 0x00, 0xD0, 0x78, 0x74, 0x59, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x2E, 0x2E, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x10, 0x00, 0x36, 0xCF, 0x78,
    0x74, 0x59, 0x74, 0x59, 0x00, 0x00, 0xD0, 0x78, 0x74, 0x59, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x42, 0x47, 0x00, 0x75, 0x00, 0x69, 0x00, 0x64, 0x00, 0x00, 0x00, 0x0F, 0x00, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
    0x01, 0x49, 0x00, 0x6E, 0x00, 0x64, 0x00, 0x65, 0x00, 0x78, 0x00, 0x0F, 0x00, 0xFF, 0x65, 0x00,
    0x72, 0x00, 0x56, 0x00, 0x6F, 0x00, 0x6C, 0x00, 0x75, 0x00, 0x00, 0x00, 0x6D, 0x00, 0x65, 0x00,
    0x49, 0x4E, 0x44, 0x45, 0x58, 0x45, 0x7E, 0x31, 0x20, 0x20, 0x20, 0x20, 0x00, 0x4E, 0xCF, 0x78,
    0x74, 0x59, 0x74, 0x59, 0x00, 0x00, 0xD0, 0x78, 0x74, 0x59, 0x03, 0x00, 0x4C, 0x00, 0x00, 0x00,
    0x42, 0x74, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F, 0x00, 0xCE, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
    0x01, 0x57, 0x00, 0x50, 0x00, 0x53, 0x00, 0x65, 0x00, 0x74, 0x00, 0x0F, 0x00, 0xCE, 0x74, 0x00,
    0x69, 0x00, 0x6E, 0x00, 0x67, 0x00, 0x73, 0x00, 0x2E, 0x00, 0x00, 0x00, 0x64, 0x00, 0x61, 0x00,
    0x57, 0x50, 0x53, 0x45, 0x54, 0x54, 0x7E, 0x31, 0x44, 0x41, 0x54, 0x20, 0x00, 0xB5, 0xD8, 0x78,
    0x74, 0x59, 0x74, 0x59, 0x00, 0x00, 0xD9, 0x78, 0x74, 0x59, 0x04, 0x00, 0x0C, 0x00, 0x00, 0x00,
];
pub(crate) const FAT_DATA1_OFFSET: usize = 0x5000;

#[rustfmt::skip] // keep this in 16-byte width
pub(crate) const FAT_DATA2: [u8; 0x50] = [
    0x7B, 0x00, 0x34, 0x00, 0x33, 0x00, 0x39, 0x00, 0x44, 0x00, 0x41, 0x00, 0x31, 0x00, 0x32, 0x00,
    0x44, 0x00, 0x2D, 0x00, 0x37, 0x00, 0x33, 0x00, 0x42, 0x00, 0x39, 0x00, 0x2D, 0x00, 0x34, 0x00,
    0x43, 0x00, 0x41, 0x00, 0x34, 0x00, 0x2D, 0x00, 0x38, 0x00, 0x38, 0x00, 0x46, 0x00, 0x39, 0x00,
    0x2D, 0x00, 0x44, 0x00, 0x41, 0x00, 0x32, 0x00, 0x43, 0x00, 0x46, 0x00, 0x33, 0x00, 0x31, 0x00,
    0x38, 0x00, 0x45, 0x00, 0x32, 0x00, 0x37, 0x00, 0x35, 0x00, 0x7D, 0x00, 0x00, 0x00, 0x00, 0x00,
];
pub(crate) const FAT_DATA2_OFFSET: usize = 0x9000;

#[rustfmt::skip] // keep this in 16-byte width
pub(crate) const FAT_DATA3: [u8; 0x10] = [
    0x0C, 0x00, 0x00, 0x00, 0x28, 0x34, 0x07, 0x9C, 0x7C, 0xBD, 0x65, 0xF9, 0x00, 0x00, 0x00, 0x00,
];
pub(crate) const FAT_DATA3_OFFSET: usize = 0xD000;

pub(crate) const CSW_ADDR: usize = CRG_UDC_MEMBASE + CRG_UDC_APP_BUFOFFSET + CRG_UDC_APP_BUF_LEN;
pub(crate) const CBW_ADDR: usize = CRG_UDC_MEMBASE + CRG_UDC_APP_BUFOFFSET;
pub(crate) const EP1_IN_BUF: usize = CRG_UDC_MEMBASE + CRG_UDC_APP_BUFOFFSET + 1024;
pub(crate) const EP1_IN_BUF_LEN: usize = 1024;
#[allow(dead_code)]
pub(crate) const EP1_OUT_BUF: usize = CRG_UDC_MEMBASE + CRG_UDC_APP_BUFOFFSET + 2048;
#[allow(dead_code)]
pub(crate) const EP1_OUT_BUF_LEN: usize = 1024;

pub(crate) const MASS_STORAGE_EPADDR_IN: u8 = 0x81;
pub(crate) const MASS_STORAGE_EPADDR_OUT: u8 = 0x01;

pub const FS_MAX_PKT_SIZE: usize = 64;
pub const HS_MAX_PKT_SIZE: usize = 512;

pub(crate) fn enable_mass_storage_eps(this: &mut CorigineUsb, ep_num: u8) {
    this.ep_enable(ep_num, USB_RECV, HS_MAX_PKT_SIZE as _, EpType::BulkOutbound);
    this.ep_enable(ep_num, USB_SEND, HS_MAX_PKT_SIZE as _, EpType::BulkInbound);
}

// Call from USB_REQ_SET_CONFIGURATION after setting device state to Configured
pub(crate) fn enable_composite_eps(this: &mut CorigineUsb) {
    // MSD endpoints
    enable_mass_storage_eps(this, 1);

    // CDC notification IN (interrupt)
    this.ep_enable(2, USB_SEND, HS_INT_MPS as _, EpType::IntrInbound);

    // CDC data bulk OUT and IN
    this.ep_enable(3, USB_RECV, HS_BULK_MPS as _, EpType::BulkOutbound);
    this.ep_enable(3, USB_SEND, HS_BULK_MPS as _, EpType::BulkInbound);
}

pub fn get_descriptor_request(this: &mut CorigineUsb, value: u16, _index: usize, length: usize) {
    let ep0_buf = unsafe {
        core::slice::from_raw_parts_mut(
            this.ep0_buf.load(Ordering::SeqCst) as *mut u8,
            CRG_UDC_EP0_REQBUFSIZE,
        )
    };

    match (value >> 8) as u8 {
        USB_DT_DEVICE => {
            let mut dd = DeviceDescriptor::composite_with_iad();
            dd.b_max_packet_size0 = 64;
            let len = length.min(core::mem::size_of::<DeviceDescriptor>());
            ep0_buf[..len].copy_from_slice(&dd.as_ref()[..len]);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, len, 0);
        }
        USB_DT_DEVICE_QUALIFIER => {
            let mut q = QualifierDescriptor::default_mass_storage();
            // For composite the fields still apply. Keep class 0.
            q.b_num_configurations = 1;
            let len = length.min(core::mem::size_of::<QualifierDescriptor>());
            ep0_buf[..len].copy_from_slice(&q.as_ref()[..len]);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, len, 0);
        }
        USB_DT_CONFIG => {
            let wrote = write_config_hs(ep0_buf);
            let buffsize = wrote.min(length);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        USB_DT_OTHER_SPEED_CONFIG => {
            let wrote = write_config_fs(ep0_buf);
            let buffsize = wrote.min(length);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        USB_DT_STRING => {
            let id = (value & 0xFF) as u8;
            let len = if id == 0 {
                ep0_buf[..4].copy_from_slice(&[4, USB_DT_STRING, 9, 4]);
                4
            } else {
                let s = match id {
                    1 => MANUFACTURER,
                    2 => PRODUCT,
                    _ => SERIAL,
                };
                let slen = 2 + s.len() * 2;
                ep0_buf[0] = slen as u8;
                ep0_buf[1] = USB_DT_STRING;
                for (dst, &src) in ep0_buf[2..].chunks_exact_mut(2).zip(s.as_bytes()) {
                    dst.copy_from_slice(&(src as u16).to_le_bytes());
                }
                slen
            };
            let buffsize = length.min(len);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        USB_DT_BOS => {
            let total_length =
                core::mem::size_of::<BosDescriptor>() + core::mem::size_of::<ExtCapDescriptor>();
            let bos = BosDescriptor::default_mass_storage(total_length as u16, 1);
            let ext = ExtCapDescriptor::default_mass_storage((0xfa << 8) | (0x3 << 3));
            let response: [&[u8]; 2] = [bos.as_ref(), ext.as_ref()];
            let mut idx = 0;
            for part in response {
                ep0_buf[idx..idx + part.len()].copy_from_slice(part);
                idx += part.len();
            }
            let buffsize = total_length.min(length);
            this.ep0_send(this.ep0_buf.load(Ordering::SeqCst) as usize, buffsize, 0);
        }
        _ => {
            this.ep_halt(0, USB_RECV);
        }
    }
}

pub fn usb_ep1_bulk_out_complete(
    this: &mut CorigineUsb,
    buf_addr: usize,
    info: u32,
    _error: u8,
    _residual: u16,
) {
    let length = info & 0xFFFF;
    let buf = unsafe { core::slice::from_raw_parts(buf_addr as *const u8, info as usize & 0xFFFF) };
    let mut cbw = Cbw::default();
    cbw.as_mut().copy_from_slice(&buf[..size_of::<Cbw>()]);
    let mut csw = Csw::derive();

    if UmsState::CommandPhase == this.ms_state && (length == 31) {
        // CBW
        if cbw.signature == BULK_CBW_SIG {
            csw.signature = BULK_CSW_SIG;
            csw.tag = cbw.tag;
            csw.update_hw();
            process_mass_storage_command(this, cbw);
            // invalid_cbw = 0;
        } else {
            crate::println!("Invalid CBW, HALT");
            this.ep_halt(1, USB_SEND);
            this.ep_halt(1, USB_RECV);
            // invalid_cbw = 1;
        }
    } else if UmsState::CommandPhase == this.ms_state && (length != 31) {
        crate::println!("invalid command");
        this.ep_halt(1, USB_SEND);
        this.ep_halt(1, USB_RECV);
        // invalid_cbw = 1;
    } else if UmsState::DataPhase == this.ms_state {
        crate::println!("data");
        //DATA
        if let Some((write_offset, len)) = this.callback_wr.take() {
            let app_buf = conjure_app_buf();
            let disk = conjure_disk();
            // update the received data to the disk
            disk[write_offset..write_offset + len].copy_from_slice(&app_buf[..len]);
            if let Some((offset, remaining_len)) = this.remaining_wr.take() {
                this.setup_big_write(app_buf_addr(), app_buf_len(), offset, remaining_len);
                this.ms_state = UmsState::DataPhase;
                csw.update_hw();
            } else {
                csw.residue = 0;
                csw.status = 0;
                csw.send(this);
            }
        } else {
            crate::println!("Data completion reached without destination for data copy! data dropped.");
        }
    } else {
        crate::println!("uhhh wtf");
    }
}

pub fn usb_ep1_bulk_in_complete(
    this: &mut CorigineUsb,
    _buf_addr: usize,
    info: u32,
    _error: u8,
    _residual: u16,
) {
    // crate::println!("bulk IN handler");
    let length = info & 0xFFFF;
    if UmsState::DataPhase == this.ms_state {
        //DATA
        if let Some((offset, len)) = this.remaining_rd.take() {
            let app_buf = conjure_app_buf();
            let disk = conjure_disk();
            this.setup_big_read(app_buf, disk, offset, len);
            this.ms_state = UmsState::DataPhase;
        } else {
            this.bulk_xfer(1, USB_SEND, CSW_ADDR, 13, 0, 0);
            this.ms_state = UmsState::StatusPhase;
        }
    } else if UmsState::StatusPhase == this.ms_state && length == 13 {
        //CSW
        this.bulk_xfer(1, USB_RECV, CBW_ADDR, 31, 0, 0);
        this.ms_state = UmsState::CommandPhase;
    }
}

// ===== CDC Bulk OUT (EP3 OUT) =====
pub fn usb_ep3_bulk_out_complete(
    this: &mut CorigineUsb,
    buf_addr: usize,
    _info: u32,
    _error: u8,
    residual: u16,
) {
    // crate::println!("EP3 OUT: {:x} {:x}", info, _error);

    let actual = CRG_UDC_APP_BUF_LEN - residual as usize;

    if actual == 0 {
        return; // zero-length packet, ignore
    }

    // Slice of received data
    let buf = unsafe { core::slice::from_raw_parts(buf_addr as *const u8, actual as usize) };

    // For now: just print, or push into a ring buffer for your "virtual terminal"
    // crate::println!("CDC OUT received {} bytes: {:?}", actual, &buf);

    critical_section::with(|cs| {
        let mut queue = crate::USB_RX.borrow(cs).borrow_mut();
        for &d in buf {
            queue.push_back(d);
        }
    });

    // Re-arm OUT transfer so host can send more
    let acm_buf = this.cdc_acm_rx_slice();
    this.bulk_xfer(3, USB_RECV, acm_buf.as_ptr() as usize, acm_buf.len(), 0, 0);
}

pub fn flush_tx(this: &mut CorigineUsb) {
    let mut written = 0;

    while !crate::platform::usb::TX_IDLE.swap(false, core::sync::atomic::Ordering::SeqCst) {
        // wait for tx to go idle
    }

    let tx_buf = this.cdc_acm_tx_slice();
    critical_section::with(|cs| {
        let mut queue = crate::USB_TX.borrow(cs).borrow_mut();
        let to_copy = queue.len().min(CRG_UDC_APP_BUF_LEN);

        let (a, b) = queue.as_slices();
        if to_copy <= a.len() {
            tx_buf[..to_copy].copy_from_slice(&a[..to_copy]);
            queue.drain(..to_copy);
            written = to_copy;
        } else {
            let first = a.len();
            let second = to_copy - first;
            tx_buf[..first].copy_from_slice(a);
            tx_buf[first..to_copy].copy_from_slice(&b[..second]);
            queue.drain(..to_copy);
            written = to_copy;
        }
    });

    if written > 0 {
        this.bulk_xfer(3, USB_SEND, tx_buf.as_ptr() as usize, written, 0, 0);
    } else {
        // release the lock
        TX_IDLE.store(true, Ordering::SeqCst);
    }
}

// ===== CDC Bulk IN (EP3 IN) =====
pub fn usb_ep3_bulk_in_complete(
    this: &mut CorigineUsb,
    _buf_addr: usize,
    _info: u32,
    _error: u8,
    _residual: u16,
) {
    // crate::println!("EP3 IN");
    // let length = CRG_UDC_APP_BUF_LEN - residual as usize;
    // crate::println!("CDC IN transfer complete, {} bytes sent", length);

    // signal that more stuff can be put into the pipe
    TX_IDLE.store(true, Ordering::SeqCst);

    // this may or may not initiate a new connection, depending on how full the Tx buffer is
    flush_tx(this);
}

// ===== CDC Notification IN (EP2 IN) =====
pub fn usb_ep2_int_in_complete(
    _this: &mut CorigineUsb,
    _buf_addr: usize,
    _info: u32,
    _error: u8,
    _residual: u16,
) {
    // crate::println!("EP2 INT");
    // let length = CRG_UDC_APP_BUF_LEN - residual as usize;
    // crate::println!("CDC notification sent, {} bytes", length);

    // Typically sends SERIAL_STATE bitmap (carrier detect etc).
    // Ignoring - this is a virtual terminal
}

fn process_mass_storage_command(this: &mut CorigineUsb, cbw: Cbw) {
    match cbw.cdb[0] {
      0x00 => { /* Request the device to report if it is ready */
        process_test_unit_ready(this, cbw);
      }
      0x03 => { /* Transfer status sense data to the host */
        process_request_sense(this, cbw);
      }
      0x12 => { /* Inquity command. Get device information */

        process_inquiry_command(this, cbw);
      }
      0x1E => { /* Prevent or allow the removal of media from a removable
                 ** media device
                 */
        process_prevent_allow_medium_removal(this, cbw);
      }
      0x25 => { /* Report current media capacity */
        process_report_capacity(this, cbw);
      }
      0x9e => {
        process_read_capacity_16(this, cbw);
      }
      0x28 => { /* Read (10) Transfer binary data from media to the host */
        process_read_command(this, cbw);
      }
      0x2A => { /* Write (10) Transfer binary data from the host to the
                 ** media
                 */
        process_write_command(this, cbw);
      }
      0xAA => { /* Write (12) Transfer binary data from the host to the
                 ** media
                 */
         process_write12_command(this, cbw);
      }
      0x01 | /* Position a head of the drive to zero track */
      0x04 | /* Format unformatted media */
      0x1A |
      0x1B | /* Request a request a removable-media device to load or
                 ** unload its media
                 */
      0x1D | /* Perform a hard reset and execute diagnostics */
      0x23 | /* Read Format Capacities. Report current media capacity and
                 ** formattable capacities supported by media
                 */
      0x2B | /* Seek the device to a specified address */
      0x2E | /* Transfer binary data from the host to the media and
                 ** verify data
                 */
      0x2F | /* Verify data on the media */
      0x55 | /* Allow the host to set parameters in a peripheral */
      0x5A | /* Report parameters to the host */
      0xA8 /* Read (12) Transfer binary data from the media to the host */ => {
        process_unsupported_command(this, cbw);
      }
      _ => {
        process_unsupported_command(this, cbw);
     }
    }
}

fn process_test_unit_ready(this: &mut CorigineUsb, _cbw: Cbw) {
    let mut csw = Csw::derive();
    csw.residue = 0;
    csw.status = 0;

    csw.send(this);
}

fn process_request_sense(this: &mut CorigineUsb, cbw: Cbw) {
    let mut csw = Csw::derive();
    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 0;
        csw.send(this);
        // crate::println!("UMS_STATE_STATUS_PHASE\r\n");
    } else if cbw.flags & 0x80 != 0 {
        if cbw.data_transfer_length < 18 {
            let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
            ep1_in[..18].fill(0);
            this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, cbw.data_transfer_length as usize, 0, 0);
            this.ms_state = UmsState::DataPhase;
            csw.residue = 0;
            csw.status = 0;
        } else if cbw.data_transfer_length >= 18 {
            let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
            ep1_in[..18].fill(0);
            this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, 18, 0, 0);
            this.ms_state = UmsState::DataPhase;
            csw.residue = cbw.data_transfer_length - 18;
            csw.status = 0;
        }
    }
    csw.update_hw();
}

fn process_inquiry_command(this: &mut CorigineUsb, cbw: Cbw) {
    let mut csw = Csw::derive();
    let inquiry_data = InquiryResponse {
        peripheral_device_type: 0,
        rmb: 0,
        version: 0,
        response_data_format: 1,
        additional_length: 31,
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
        vendor_identification: *b"Bao Semi",
        product_identification: *b"USB update vdisk",
        product_revision_level: *b"demo",
    };

    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 0;
        csw.send(this);
    } else if cbw.flags & 0x80 != 0 {
        if cbw.data_transfer_length < 36 {
            let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
            ep1_in[..36].fill(0);
            this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, cbw.data_transfer_length as usize, 0, 0);
            this.ms_state = UmsState::DataPhase;
            csw.residue = 0;
            csw.status = 0;
        } else if cbw.data_transfer_length as usize >= size_of::<InquiryResponse>() {
            let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
            ep1_in[..size_of::<InquiryResponse>()].copy_from_slice(inquiry_data.as_ref());

            this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, size_of::<InquiryResponse>(), 0, 0);
            this.ms_state = UmsState::DataPhase;
            csw.residue = cbw.data_transfer_length - size_of::<InquiryResponse>() as u32;
            csw.status = 0;
        }
    }
    csw.update_hw();
}

fn process_prevent_allow_medium_removal(this: &mut CorigineUsb, _cbw: Cbw) {
    let mut csw = Csw::derive();
    csw.residue = 0;
    csw.status = 0;
    csw.send(this);
}

fn process_report_capacity(this: &mut CorigineUsb, cbw: Cbw) {
    crate::println!("REPORT CAPACITY");
    let mut csw = Csw::derive();
    let rc_lba = RAMDISK_LEN / SECTOR_SIZE as usize - 1;
    let rc_bl: u32 = SECTOR_SIZE as u32;
    let mut capacity = [0u8; 8];

    capacity[..4].copy_from_slice(&rc_lba.to_be_bytes());
    capacity[4..].copy_from_slice(&rc_bl.to_be_bytes());

    if cbw.flags & 0x80 != 0 {
        let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
        ep1_in[..8].fill(0);
        ep1_in[..8].copy_from_slice(&capacity);
        this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, 8, 0, 0);
        this.ms_state = UmsState::DataPhase;
    } else {
        csw.send(this);
    }
    csw.residue = 0;
    csw.status = 0;
    csw.update_hw();
}

fn process_read_capacity_16(this: &mut CorigineUsb, _cbw: Cbw) {
    let rc_lba = (RAMDISK_LEN / SECTOR_SIZE as usize - 1) as u64;
    let rc_bl: u32 = SECTOR_SIZE as u32;

    let mut response = [0u8; 32];
    response[..8].copy_from_slice(&rc_lba.to_be_bytes());
    response[8..12].copy_from_slice(&rc_bl.to_be_bytes());
    let ep1_in = unsafe { core::slice::from_raw_parts_mut(EP1_IN_BUF as *mut u8, EP1_IN_BUF_LEN) };
    ep1_in[..32].copy_from_slice(&response);
    this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, 32, 0, 0);
    this.ms_state = UmsState::DataPhase;
}

fn process_unsupported_command(this: &mut CorigineUsb, cbw: Cbw) {
    let mut csw = Csw::derive();
    csw.residue = 0;
    csw.status = 0;

    if cbw.flags & 0x80 != 0 {
        this.bulk_xfer(1, USB_SEND, EP1_IN_BUF, 0, 0, 0);
        this.ms_state = UmsState::DataPhase;
    } else {
        csw.send(this);
    }
    csw.update_hw();
}

fn process_read_command(this: &mut CorigineUsb, cbw: Cbw) {
    let mut csw = Csw::derive();
    let mut lba;
    let mut length;

    lba = (cbw.cdb[4] as u32) << 8;
    lba |= cbw.cdb[5] as u32;
    length = (cbw.cdb[7] as u32) << 8;
    length |= cbw.cdb[8] as u32;

    length *= SECTOR_SIZE as u32;

    if cbw.flags & 0x80 == 0 {
        csw.residue = cbw.data_transfer_length;
        csw.status = 2;
        this.setup_big_write(
            app_buf_addr(),
            app_buf_len(),
            lba as usize * SECTOR_SIZE as usize,
            length as usize,
        );
        this.ms_state = UmsState::DataPhase;
        return;
    }
    crate::println!(
        "DISK READ address = 0x{:x}, length = 0x{:x}",
        RAMDISK_ADDRESS + lba as usize * SECTOR_SIZE as usize,
        length
    );
    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 2;

        csw.send(this);
    } else {
        csw.residue = 0;
        csw.status = 0;
        if cbw.data_transfer_length < length {
            length = cbw.data_transfer_length;
            csw.residue = cbw.data_transfer_length;
            csw.status = 1;
        } else if cbw.data_transfer_length > length {
            csw.residue = cbw.data_transfer_length - length;
            csw.status = 1;
        }
        let app_buf = conjure_app_buf();
        let disk = conjure_disk();
        this.setup_big_read(app_buf, disk, lba as usize * SECTOR_SIZE as usize, length as usize);
        this.ms_state = UmsState::DataPhase;
    }
}

fn process_write_command(this: &mut CorigineUsb, cbw: Cbw) {
    let mut csw = Csw::derive();

    let mut lba: u32;
    let mut length: u32;

    lba = (cbw.cdb[4] as u32) << 8;
    lba |= cbw.cdb[5] as u32;
    length = (cbw.cdb[7] as u32) << 8;
    length |= cbw.cdb[8] as u32;

    length *= SECTOR_SIZE as u32;

    if cbw.flags & 0x80 != 0 {
        csw.residue = cbw.data_transfer_length;
        csw.status = 2;
        let app_buf = conjure_app_buf();
        let disk = conjure_disk();
        this.setup_big_read(app_buf, disk, lba as usize * SECTOR_SIZE as usize, length as usize);
        this.ms_state = UmsState::DataPhase;
        csw.update_hw();
        return;
    }

    crate::println!(
        "Write address = 0x{:x}, length = 0x{:x}",
        RAMDISK_ADDRESS + lba as usize * SECTOR_SIZE as usize,
        length
    );

    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 2;
        csw.send(this);
    } else {
        csw.residue = 0;
        csw.status = 0;
        if cbw.data_transfer_length > length {
            csw.residue = cbw.data_transfer_length - length;
            length = cbw.data_transfer_length;
            csw.status = 1;
        } else if cbw.data_transfer_length < length {
            csw.residue = cbw.data_transfer_length;
            csw.status = 1;
            length = cbw.data_transfer_length;
        }
        this.setup_big_write(
            app_buf_addr(),
            app_buf_len(),
            lba as usize * SECTOR_SIZE as usize,
            length as usize,
        );
        this.ms_state = UmsState::DataPhase;
    }
    csw.update_hw();
}

fn process_write12_command(this: &mut CorigineUsb, cbw: Cbw) {
    let mut csw = Csw::derive();

    let mut lba;
    let mut length;

    lba = (cbw.cdb[2] as u32) << 24;
    lba |= (cbw.cdb[3] as u32) << 16;
    lba |= (cbw.cdb[4] as u32) << 8;
    lba |= (cbw.cdb[5] as u32) << 0;
    length = (cbw.cdb[6] as u32) << 24;
    length |= (cbw.cdb[7] as u32) << 16;
    length |= (cbw.cdb[8] as u32) << 8;
    length |= (cbw.cdb[9] as u32) << 0;

    length *= SECTOR_SIZE as u32;
    crate::println!("write12 of {} bytes", length);

    if cbw.flags & 0x80 != 0 {
        csw.residue = cbw.data_transfer_length;
        csw.status = 2;
        // note: zero-length transfer but we still update the buffer because that's what the reference driver
        // does
        let app_buf = conjure_app_buf();
        let disk = conjure_disk();
        this.setup_big_read(app_buf, disk, lba as usize * SECTOR_SIZE as usize, length as usize);
        this.ms_state = UmsState::DataPhase;
        csw.update_hw();
        return;
    }

    if cbw.data_transfer_length == 0 {
        csw.residue = 0;
        csw.status = 2;
        csw.send(this);
        return;
    } else {
        csw.residue = 0;
        csw.status = 0;
        if cbw.data_transfer_length > length {
            csw.residue = cbw.data_transfer_length - length;
            length = cbw.data_transfer_length;
        } else if cbw.data_transfer_length < length {
            csw.residue = cbw.data_transfer_length;
            csw.status = 2;
            length = cbw.data_transfer_length;
        }
        this.setup_big_write(
            app_buf_addr(),
            app_buf_len(),
            lba as usize * SECTOR_SIZE as usize,
            length as usize,
        );
        this.ms_state = UmsState::DataPhase;
    }
    csw.update_hw();
}

// the 1024 is reserved for the CSW/CBW records
pub(crate) fn app_buf_addr() -> usize { CRG_UDC_MEMBASE + CRG_UDC_APP_BUFOFFSET + 1024 }
// the length subtracts the reserved 1024, and adds one page as a hack for testing - TODO: fix that
pub(crate) fn app_buf_len() -> usize { CRG_UDC_APP_BUFSIZE - 1024 + 4096 }
pub(crate) fn conjure_app_buf() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(app_buf_addr() as *mut u8, app_buf_len()) } // added +1 page in CRG_UDC_MEMBASE
}

pub(crate) fn conjure_disk() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(RAMDISK_ADDRESS as *mut u8, RAMDISK_LEN) }
}
