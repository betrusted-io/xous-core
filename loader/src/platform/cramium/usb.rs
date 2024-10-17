use core::convert::TryFrom;
use core::sync::atomic::{AtomicPtr, Ordering};

use cramium_hal::usb::compat::AtomicCsr;
use cramium_hal::usb::driver::CorigineUsb;
use cramium_hal::usb::driver::*;
use cramium_hal::usb::utra::*;
use riscv::register::{mcause, mie, mstatus, vexriscv::mim, vexriscv::mip};

// locate the "disk" at 1MiB -- middle of the SRAM region. This can be
// fine-tuned later.
const RAMDISK_ADDRESS: usize = utralib::HW_SRAM_MEM + 1024 * 1024;
const RAMDISK_LEN: usize = 512 * 1024; // 512k of RAM allocated to "disk"
const SECTOR_SIZE: u16 = 512;
// Note that the trap handler is just placed one page below this, and it
// needs to be manually updated in the assembly because we can't refer to
// consts in that snippet of assembly. That handler also needs a default
// stack area, which is right below that spare page.
const SCRATCH_PAGE: usize = RAMDISK_ADDRESS - 4096;
#[allow(dead_code)] // this reminds us there are two places this has to be changed in assembly-land
const EXCEPTION_STACK: usize = SCRATCH_PAGE;

const USB_TYPE_MASK: u8 = 0x03 << 5;
const USB_TYPE_STANDARD: u8 = 0x00 << 5;
const USB_TYPE_CLASS: u8 = 0x01 << 5;
const USB_TYPE_VENDOR: u8 = 0x02 << 5;
const USB_TYPE_RESERVED: u8 = 0x03 << 5;
/*
 * USB recipients, the third of three bRequestType fields
 */
const USB_RECIP_MASK: u8 = 0x1f;
const USB_RECIP_DEVICE: u8 = 0x00;
const USB_RECIP_INTERFACE: u8 = 0x01;
const USB_RECIP_ENDPOINT: u8 = 0x02;
const USB_RECIP_OTHER: u8 = 0x03;

const USB_REQ_GET_STATUS: u8 = 0x00;
const USB_REQ_CLEAR_FEATURE: u8 = 0x01;
const USB_REQ_SET_FEATURE: u8 = 0x03;
const USB_REQ_SET_ADDRESS: u8 = 0x05;
const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;
const USB_REQ_SET_DESCRIPTOR: u8 = 0x07;
const USB_REQ_GET_CONFIGURATION: u8 = 0x08;
const USB_REQ_SET_CONFIGURATION: u8 = 0x09;
const USB_REQ_GET_INTERFACE: u8 = 0x0A;
const USB_REQ_SET_INTERFACE: u8 = 0x0B;
const USB_REQ_SYNCH_FRAME: u8 = 0x0C;
const USB_REQ_SET_SEL: u8 = 0x30;
const USB_REQ_SET_ISOCH_DELAY: u8 = 0x31;

const USB_DT_DEVICE: u8 = 0x01;
const USB_DT_CONFIG: u8 = 0x02;
const USB_DT_STRING: u8 = 0x03;
const USB_DT_INTERFACE: u8 = 0x04;
const USB_DT_ENDPOINT: u8 = 0x05;
const USB_DT_DEVICE_QUALIFIER: u8 = 0x06;
const USB_DT_OTHER_SPEED_CONFIG: u8 = 0x07;
const USB_DT_INTERFACE_POWER: u8 = 0x08;

/* these are from a minor usb 2.0 revision (ECN) */
const USB_DT_OTG: u8 = 0x09;
const USB_DT_DEVICE_CAPABILITY: u8 = 0x10;
const USB_DT_DEBUG: u8 = 0x0a;
const USB_DT_INTERFACE_ASSOCIATION: u8 = 0x0b;
const USB_DT_BOS: u8 = 0x0f;

/* From the T10 UAS specification */
const USB_DT_PIPE_USAGE: u8 = 0x24;
/* From the USB 3.0 spec */
const USB_DT_SS_ENDPOINT_COMP: u8 = 0x30;
/* From the USB 3.1 spec */
const USB_DT_SSP_ISOC_ENDPOINT_COMP: u8 = 0x31;

#[repr(i32)]
enum UmsState {
    // Thesea ren't used
    CommandPhase = -10,
    DataPhase,
    StatusPhase,
    Idle = 0,
    AbortBulkOut,
    Reset,
    InterfaceChange,
    ConfigChange,
    Disconnect,
    Exit,
    Terminated,
}

#[repr(packed)]
struct DeviceDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub b_cd_usb: u16,
    pub b_device_class: u8,
    pub b_device_sub_class: u8,
    pub b_device_protocol: u8,
    pub b_max_packet_size0: u8,
    pub id_vendor: u16,
    pub id_product: u16,
    pub b_cd_device: u16,
    pub i_manufacturer: u8,
    pub i_product: u8,
    pub i_serial_number: u8,
    pub b_num_configurations: u8,
}
const VENDOR_ID: u16 = 0x1209;
const PRODUCT_ID: u16 = 0x3613; // this needs to change! this is the Precursor product ID.
const MANUFACTURER: &'static str = "Bao Semi";
const PRODUCT: &'static str = "SecuriBao";
// no seriously, do this
const SERIAL: &'static str = "TODO";

impl DeviceDescriptor {
    pub fn default_mass_storage() -> Self {
        Self {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_DEVICE,
            b_cd_usb: 0x0200,
            b_device_class: 0,
            b_device_sub_class: 0,
            b_device_protocol: 0,
            b_max_packet_size0: 0x40,
            id_vendor: VENDOR_ID,
            id_product: PRODUCT_ID,
            b_cd_device: 0x0101,
            i_manufacturer: 0x01,  // string index for manufacturer
            i_product: 0x02,       // string index for product
            i_serial_number: 0x03, // string index for serial number
            b_num_configurations: 1,
        }
    }
}
impl AsRef<[u8]> for DeviceDescriptor {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const DeviceDescriptor as *const u8,
                core::mem::size_of::<DeviceDescriptor>(),
            ) as &[u8]
        }
    }
}

/* USB_DT_DEVICE_QUALIFIER: Device Qualifier descriptor */
#[repr(packed)]
struct QualifierDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub b_cd_usb: u16,
    pub b_device_class: u8,
    pub b_device_sub_class: u8,
    pub b_device_protocol: u8,
    pub b_max_packet_size0: u8,
    pub b_num_configurations: u8,
    pub b_reserved: u8,
}
impl QualifierDescriptor {
    pub fn default_mass_storage() -> Self {
        Self {
            b_length: 0xA,
            b_descriptor_type: 0x6,
            b_cd_usb: 0x200,
            b_device_class: 0x0,
            b_device_sub_class: 0x0,
            b_device_protocol: 0x0,
            b_max_packet_size0: 0x40,
            b_num_configurations: 0x1,
            b_reserved: 0x0,
        }
    }
}
impl AsRef<[u8]> for QualifierDescriptor {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const QualifierDescriptor as *const u8,
                core::mem::size_of::<QualifierDescriptor>(),
            ) as &[u8]
        }
    }
}

#[repr(packed)]
struct ConfigDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub w_total_length: u16,
    pub b_num_interfaces: u8,
    pub b_configuration_value: u8,
    pub i_configuration: u8,
    pub bm_attributes: u8,
    pub b_max_power: u8,
}
impl ConfigDescriptor {
    pub fn default_mass_storage(total_length: u16) -> Self {
        ConfigDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_CONFIG,
            w_total_length: total_length,
            b_num_interfaces: 1,
            b_configuration_value: 1,
            i_configuration: 0x0,
            bm_attributes: 0xC0,
            b_max_power: 250,
        }
    }
}
impl AsRef<[u8]> for ConfigDescriptor {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const ConfigDescriptor as *const u8,
                core::mem::size_of::<ConfigDescriptor>(),
            ) as &[u8]
        }
    }
}

#[repr(packed)]
struct InterfaceDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub b_interface_number: u8,
    pub b_alternate_setting: u8,
    pub b_num_endpoints: u8,
    pub b_interface_class: u8,
    pub b_interface_sub_class: u8,
    pub b_interface_protocol: u8,
    pub i_interface: u8,
}
impl InterfaceDescriptor {
    pub fn default_mass_storage() -> Self {
        InterfaceDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_CONFIG,
            b_interface_number: 0,
            b_alternate_setting: 0,
            b_num_endpoints: 2,
            b_interface_class: 0x08, // mass storage class
            b_interface_sub_class: 0x06,
            b_interface_protocol: 0x50,
            i_interface: 0x0,
        }
    }
}
impl AsRef<[u8]> for InterfaceDescriptor {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const InterfaceDescriptor as *const u8,
                core::mem::size_of::<InterfaceDescriptor>(),
            ) as &[u8]
        }
    }
}
#[repr(packed)]
struct EndpointDescriptor {
    b_length: u8,
    b_descriptor_type: u8,
    b_endpoint_address: u8,
    b_m_attributes: u8,
    w_max_packet_size: u16,
    b_interval: u8,
}
impl EndpointDescriptor {
    pub fn default_mass_storage(addr: u8, max_packet_size: u16) -> Self {
        EndpointDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_ENDPOINT,
            b_endpoint_address: addr,
            b_m_attributes: 0x02,
            w_max_packet_size: max_packet_size,
            b_interval: 0x0,
        }
    }
}
const MASS_STORAGE_EPADDR_IN: u8 = 0x81;
const MASS_STORAGE_EPADDR_OUT: u8 = 0x01;
impl AsRef<[u8]> for EndpointDescriptor {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const EndpointDescriptor as *const u8,
                core::mem::size_of::<EndpointDescriptor>(),
            ) as &[u8]
        }
    }
}

static mut USB: Option<CorigineUsb> = None;
// todo: figure out how to make this less gross
static mut UMS_STATE: UmsState = UmsState::Idle;

// MBR template
// 0x0b~0x0C 2 bytes means block size, default 0x200 bytes
// 0x20~0x23 4 bytes means block number, default 0x400 block
#[rustfmt::skip] // keep this in 16-byte width
const MBR_TEMPLATE: [u8; 512] = [
    0xEB, 0x3C, 0x90, 0x4D, 0x53, 0x44, 0x4F, 0x53, 0x35, 0x2E, 0x30, 0x00, 0x02, 0x20, 0x01, 0x00,
    0x02, 0x00, 0x02, 0x00, 0x00, 0xF8, 0x00, 0x01, 0x3f, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x04, 0x00, 0x00, 0x80, 0x00, 0x29, 0x72, 0x1a, 0x65, 0xA4, 0x4E, 0x4F, 0x20, 0x4E, 0x41,
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
    0x72, 0x74, 0x0D, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xAC, 0xCB, 0xD8, 0x55, 0xAA
];

fn delay(quantum: usize) {
    use utralib::{CSR, utra};
    // abuse the d11ctime timer to create some time-out like thing
    let mut d11c = CSR::new(utra::d11ctime::HW_D11CTIME_BASE as *mut u32);
    d11c.wfo(utra::d11ctime::CONTROL_COUNT, 100_000_000); // 1.0ms per interval
    let mut polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
    for _ in 0..quantum {
        while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
    }
    // we have to split this because we don't know where we caught the previous interval
    polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
    for _ in 0..quantum {
        while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
    }
}

fn enable_irq(irq_no: usize) {
    // Note that the vexriscv "IRQ Mask" register is inverse-logic --
    // that is, setting a bit in the "mask" register unmasks (i.e. enables) it.
    mim::write(mim::read() | (1 << irq_no));
}

fn irq_setup() {
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            // stop delegating
            "li          t0, 0x0",
            "csrw        mideleg, t0",
            "csrw        medeleg, t0",
            // Set trap handler, which will be called
            // on interrupts and cpu faults
            "la   t0, _start_trap", // this first one forces the nop sled symbol to be generated
            "la   t0, _start_trap_aligned", // this is the actual target
            "csrw mtvec, t0",
        );
    }
    // enable IRQ handling
    mim::write(0x0); // first make sure everything is disabled, so we aren't OR'ing in garbage
    // must enable external interrupts on the CPU for any of the above to matter
    unsafe { mie::set_mext() };
    unsafe { mstatus::set_mie() };
}

pub fn init_usb() {
    let mut usb = unsafe {
        cramium_hal::usb::driver::CorigineUsb::new(
            0,
            0,
            cramium_hal::board::CRG_UDC_MEMBASE,
            AtomicCsr::new(cramium_hal::usb::utra::CORIGINE_USB_BASE as *mut u32),
            AtomicCsr::new(utralib::utra::irqarray1::HW_IRQARRAY1_BASE as *mut u32),
        )
    };
    usb.assign_handler(handle_event);

    // initialize the "disk" area
    let disk = unsafe { core::slice::from_raw_parts_mut(RAMDISK_ADDRESS as *mut u8, RAMDISK_LEN) };
    disk.fill(0);
    disk[..MBR_TEMPLATE.len()].copy_from_slice(&MBR_TEMPLATE);
    //set block size
    disk[0xb..0xd].copy_from_slice(&SECTOR_SIZE.to_le_bytes());
    //set storage size
    disk[0x20..0x24].copy_from_slice(&(RAMDISK_LEN as u32).to_le_bytes());

    // install the interrupt handler
    // setup the stack & controller
    irq_setup();
    enable_irq(utralib::utra::irqarray1::IRQARRAY1_IRQ);
    // for testing
    // enable_irq(utralib::utra::irqarray19::IRQARRAY19_IRQ);
    // let mut irqarray19 = utralib::CSR::new(utralib::utra::irqarray19::HW_IRQARRAY19_BASE as *mut u32);
    // irqarray19.wo(utralib::utra::irqarray19::EV_ENABLE, 0x80);

    unsafe {
        USB = Some(usb);
    }
}

pub unsafe fn test_usb() {
    if let Some(ref mut usb_ref) = USB {
        let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
        usb.reset();
        let mut poweron = 0;
        loop {
            usb.udc_handle_interrupt();
            if usb.pp() {
                poweron += 1; // .pp() is a sham. MPW has no way to tell if power is applied. This needs to be fixed for NTO.
            }
            delay(100);
            if poweron >= 4 {
                break;
            }
        }
        usb.reset();
        usb.init();
        usb.start();
        usb.update_current_speed();

        crate::println!("hw started...");
        /*
        let mut vbus_on = false;
        let mut vbus_on_count = 0;
        let mut in_u0 = false;
        loop {
            if vbus_on == false && vbus_on_count == 4 {
                crate::println!("vbus on");
                usb.init();
                usb.start();
                vbus_on = true;
                in_u0 = false;
            } else if usb.pp() == true && vbus_on == false {
                vbus_on_count += 1;
                delay(100);
            } else if usb.pp() == false && vbus_on == true {
                crate::println!("20230802 vbus off during while");
                usb.stop();
                usb.reset();
                vbus_on_count = 0;
                vbus_on = false;
                in_u0 = false;
            } else if in_u0 == true && vbus_on == true {
                crate::println!("USB stack started");
                break;
                // crate::println!("Would be uvc_bulk_thread()");
                // uvc_bulk_thread();
            } else if usb.ccs() == true && vbus_on == true {
                crate::println!("enter U0");
                in_u0 = true;
            }
        }
        */
        let mut i = 0;
        loop {
            // wait for interrupt handler to do something
            delay(1000);
            crate::println!("{}", i);
            i += 1;
            // for testing interrupt handler
            // let mut irqarray19 = utralib::CSR::new(utralib::utra::irqarray19::HW_IRQARRAY19_BASE as *mut
            // u32); irqarray19.wfo(utralib::utra::irqarray19::EV_SOFT_TRIGGER, 0x80);
        }
    } else {
        crate::println!("USB core not allocated, skipping USB test");
    }
}

#[export_name = "_start_trap"]
// #[repr(align(4))] // can't do this yet.
#[inline(never)]
pub unsafe extern "C" fn _start_trap() -> ! {
    loop {
        // install a NOP sled before _start_trap() until https://github.com/rust-lang/rust/issues/82232 is stable
        core::arch::asm!("nop", "nop", "nop", "nop");
        #[export_name = "_start_trap_aligned"]
        pub unsafe extern "C" fn _start_trap_aligned() {
            #[rustfmt::skip]
            core::arch::asm!(
                "csrw        mscratch, sp",
                "li          sp, 0x610FF000", // scratch page: one page below the disk start
                "sw       x1, 0*4(sp)",
                // Skip SP for now
                "sw       x3, 2*4(sp)",
                "sw       x4, 3*4(sp)",
                "sw       x5, 4*4(sp)",
                "sw       x6, 5*4(sp)",
                "sw       x7, 6*4(sp)",
                "sw       x8, 7*4(sp)",
                "sw       x9, 8*4(sp)",
                "sw       x10, 9*4(sp)",
                "sw       x11, 10*4(sp)",
                "sw       x12, 11*4(sp)",
                "sw       x13, 12*4(sp)",
                "sw       x14, 13*4(sp)",
                "sw       x15, 14*4(sp)",
                "sw       x16, 15*4(sp)",
                "sw       x17, 16*4(sp)",
                "sw       x18, 17*4(sp)",
                "sw       x19, 18*4(sp)",
                "sw       x20, 19*4(sp)",
                "sw       x21, 20*4(sp)",
                "sw       x22, 21*4(sp)",
                "sw       x23, 22*4(sp)",
                "sw       x24, 23*4(sp)",
                "sw       x25, 24*4(sp)",
                "sw       x26, 25*4(sp)",
                "sw       x27, 26*4(sp)",
                "sw       x28, 27*4(sp)",
                "sw       x29, 28*4(sp)",
                "sw       x30, 29*4(sp)",
                "sw       x31, 30*4(sp)",
                // Save MEPC
                "csrr        t0, mepc",
                "sw       t0, 31*4(sp)",

                // Finally, save SP
                "csrr        t0, mscratch",
                "sw          t0, 1*4(sp)",
                // Restore a default stack pointer
                "li          sp, 0x610FF000", /* builds down from scratch page */
                // Note that registers $a0-$a7 still contain the arguments
                "j           _start_trap_rust",
                // Note to self: trying to assign the scratch and default pages using in(reg) syntax
                // clobbers the `a0` register and places the initialization outside of the handler loop
                // and there seems to be no way to refer directly to a symbol? the `sym` directive wants
                // to refer to an address, not a constant.
            );
        }
        _start_trap_aligned();
        core::arch::asm!("nop", "nop", "nop", "nop");
    }
}

#[export_name = "_resume_context"]
#[inline(never)]
pub unsafe extern "C" fn _resume_context(registers: u32) -> ! {
    #[rustfmt::skip]
    core::arch::asm!(
        "move        sp, {registers}",

        "lw        x1, 0*4(sp)",
        // Skip SP for now
        "lw        x3, 2*4(sp)",
        "lw        x4, 3*4(sp)",
        "lw        x5, 4*4(sp)",
        "lw        x6, 5*4(sp)",
        "lw        x7, 6*4(sp)",
        "lw        x8, 7*4(sp)",
        "lw        x9, 8*4(sp)",
        "lw        x10, 9*4(sp)",
        "lw        x11, 10*4(sp)",
        "lw        x12, 11*4(sp)",
        "lw        x13, 12*4(sp)",
        "lw        x14, 13*4(sp)",
        "lw        x15, 14*4(sp)",
        "lw        x16, 15*4(sp)",
        "lw        x17, 16*4(sp)",
        "lw        x18, 17*4(sp)",
        "lw        x19, 18*4(sp)",
        "lw        x20, 19*4(sp)",
        "lw        x21, 20*4(sp)",
        "lw        x22, 21*4(sp)",
        "lw        x23, 22*4(sp)",
        "lw        x24, 23*4(sp)",
        "lw        x25, 24*4(sp)",
        "lw        x26, 25*4(sp)",
        "lw        x27, 26*4(sp)",
        "lw        x28, 27*4(sp)",
        "lw        x29, 28*4(sp)",
        "lw        x30, 29*4(sp)",
        "lw        x31, 30*4(sp)",

        // Restore SP
        "lw        x2, 1*4(sp)",
        "mret",
        registers = in(reg) registers,
    );
    loop {}
}

#[export_name = "_start_trap_rust"]
pub extern "C" fn trap_handler(
    _a0: usize,
    _a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) -> ! {
    crate::println!("it's a trap!");
    let mc: mcause::Mcause = mcause::read();
    // 2 is illegal instruction
    if mc.bits() == 2 {
        crate::abort();
    } else if mc.bits() == 0x8000_000B {
        // external interrupt. find out which ones triggered it, and clear the source.
        let irqs_pending = mip::read();

        if (irqs_pending & (1 << utralib::utra::irqarray1::IRQARRAY1_IRQ)) != 0 {
            // handle USB interrupt
            unsafe {
                if let Some(ref mut usb_ref) = USB {
                    let usb = &mut *core::ptr::addr_of_mut!(*usb_ref);
                    let pending = usb.irq_csr.r(utralib::utra::irqarray1::EV_PENDING);

                    let status = usb.csr.r(USBSTS);
                    // self.print_status(status);
                    if (status & usb.csr.ms(USBSTS_SYSTEM_ERR, 1)) != 0 {
                        crate::println!("System error");
                        usb.csr.wfo(USBSTS_SYSTEM_ERR, 1);
                        crate::println!("USBCMD: {:x}", usb.csr.r(USBCMD));
                    } else {
                        if (status & usb.csr.ms(USBSTS_EINT, 1)) != 0 {
                            // from udc_handle_interrupt
                            let mut ret = cramium_hal::usb::driver::CrgEvent::None;
                            let status = usb.csr.r(USBSTS);
                            // self.print_status(status);
                            let result = if (status & usb.csr.ms(USBSTS_SYSTEM_ERR, 1)) != 0 {
                                crate::println!("System error");
                                usb.csr.wfo(USBSTS_SYSTEM_ERR, 1);
                                crate::println!("USBCMD: {:x}", usb.csr.r(USBCMD));
                                cramium_hal::usb::driver::CrgEvent::Error
                            } else {
                                if (status & usb.csr.ms(USBSTS_EINT, 1)) != 0 {
                                    usb.csr.wfo(USBSTS_EINT, 1);
                                    // divert to the loader-based event ring handler
                                    ret = usb.process_event_ring(); // there is only one event ring
                                }
                                if usb.csr.rf(IMAN_IE) != 0 {
                                    usb.csr.rmwf(IMAN_IE, 1);
                                }
                                usb.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xFFFF_FFFF);
                                // re-enable interrupts
                                usb.irq_csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 1);
                                ret
                            };
                            crate::println!("Result: {:?}", result);
                        }
                        if usb.csr.rf(IMAN_IE) != 0 {
                            usb.csr.rmwf(IMAN_IE, 1);
                        }
                        usb.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, 0xFFFF_FFFF);
                        // re-enable interrupts
                        usb.irq_csr.wfo(utralib::utra::irqarray1::EV_ENABLE_USBC_DUPE, 1);
                    }
                    // clear pending
                    usb.irq_csr.wo(utralib::utra::irqarray1::EV_PENDING, pending);
                }
            }
        }
        if (irqs_pending & (1 << 19)) != 0 {
            // handle irq19 sw trigger test
            let mut irqarray19 = utralib::CSR::new(utralib::utra::irqarray19::HW_IRQARRAY19_BASE as *mut u32);
            let pending = irqarray19.r(utralib::utra::irqarray19::EV_PENDING);
            crate::println!("pending {:x}", (pending << 16 | 19)); // encode the irq bank number and bit number as [bit | bank]
            irqarray19.wo(utralib::utra::irqarray19::EV_PENDING, pending);
            // software interrupt should not require a 0-write to reset it
        }
    } else {
        crate::abort();
    }

    // re-enable interrupts
    let status: u32;
    unsafe {
        #[rustfmt::skip]
        core::arch::asm!(
            "csrr        t0, mstatus",
            "ori         t0, t0, 3",
            "csrw        mstatus, t0",
            "csrr        {status}, mstatus",
            status = out(reg) status,
        );
    }
    crate::println!("{}", status);

    unsafe { mie::set_mext() };
    unsafe { _resume_context(SCRATCH_PAGE as u32) };
}

#[repr(C, align(8))]
#[derive(Default)]
struct CtrlRequest {
    b_request_type: u8,
    b_request: u8,
    w_value: u16,
    w_index: u16,
    w_length: u16,
}
impl AsMut<[u8]> for CtrlRequest {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self as *const CtrlRequest as *mut u8,
                core::mem::size_of::<CtrlRequest>(),
            ) as &mut [u8]
        }
    }
}

fn get_status_request(this: &mut CorigineUsb, request_type: u8, index: u16) {
    let ep0_buf =
        unsafe { core::slice::from_raw_parts_mut(this.ep0_buf.as_ptr() as *mut u8, CRG_UDC_EP0_REQBUFSIZE) };

    let mut status_val: u32 = 0;
    let recipient = request_type & 0x1f;
    let ep_num = index & 0x7f;
    let ep_dir = if index & 0x80 != 0 { USB_SEND } else { USB_RECV };

    match recipient {
        USB_RECIP_DEVICE => {
            status_val |= 0x1;
            ep0_buf[0] = status_val as u8;
            this.ep0_send(ep0_buf.as_ptr() as usize, 2, 0);
        }
        USB_RECIP_INTERFACE => {
            ep0_buf[0] = 0;
            ep0_buf[1] = 0;
            this.ep0_send(ep0_buf.as_ptr() as usize, 2, 0);
        }
        USB_RECIP_ENDPOINT => {
            if this.is_halted(ep_num as u8, ep_dir) {
                ep0_buf[0] = 1;
                this.ep0_send(ep0_buf.as_ptr() as usize, 2, 0);
            } else {
                ep0_buf[0] = 0;
                this.ep0_send(ep0_buf.as_ptr() as usize, 2, 0);
            }
        }
        _ => {
            this.ep_halt(0, USB_RECV);
        }
    }
}

pub fn get_descriptor_request(this: &mut CorigineUsb, value: u16, _index: usize, length: usize) {
    let ep0_buf =
        unsafe { core::slice::from_raw_parts_mut(this.ep0_buf.as_ptr() as *mut u8, CRG_UDC_EP0_REQBUFSIZE) };

    match (value >> 8) as u8 {
        USB_DT_DEVICE => {
            crate::println!("USB_DT_DEVICE");
            let mut device_descriptor = DeviceDescriptor::default_mass_storage();
            device_descriptor.b_max_packet_size0 = 64;
            device_descriptor.b_cd_usb = 0x0210;

            let len = length.min(core::mem::size_of::<DeviceDescriptor>());
            ep0_buf[..len].copy_from_slice(&device_descriptor.as_ref()[..len]);
            crate::println!("ptr: {:x}, len: {}", this.ep0_buf.as_ptr() as usize, len);
            crate::println!("dd: {:x?}", device_descriptor.as_ref());
            crate::println!("buf: {:x?}", &ep0_buf[..len]);
            this.ep0_send(this.ep0_buf.as_ptr() as usize, len, 0);
        }
        USB_DT_DEVICE_QUALIFIER => {
            crate::println!("USB_DT_DEVICE_QUALIFIER");

            let qualifier_descriptor = QualifierDescriptor::default_mass_storage();
            let len = length.min(core::mem::size_of::<QualifierDescriptor>());
            ep0_buf[..len].copy_from_slice(&qualifier_descriptor.as_ref()[..len]);

            this.ep0_send(this.ep0_buf.as_ptr() as usize, len, 0);
        }
        USB_DT_OTHER_SPEED_CONFIG => {
            crate::println!("USB_DT_OTHER_SPEED_CONFIG\r\n");
            crate::println!("*** UNHANDLED ***");
        }
        USB_DT_CONFIG => {
            crate::println!("USB_DT_CONFIG\r\n");
            let total_length = size_of::<ConfigDescriptor>()
                + size_of::<InterfaceDescriptor>()
                + size_of::<EndpointDescriptor>() * 2;
            let config = ConfigDescriptor::default_mass_storage(total_length as u16);
            let interface = InterfaceDescriptor::default_mass_storage();
            let ep_in = EndpointDescriptor::default_mass_storage(MASS_STORAGE_EPADDR_IN, 64);
            let ep_out = EndpointDescriptor::default_mass_storage(MASS_STORAGE_EPADDR_OUT, 64);
            let response: [&[u8]; 4] = [config.as_ref(), interface.as_ref(), ep_in.as_ref(), ep_out.as_ref()];
            let flattened = response.iter().flat_map(|slice| slice.iter().copied());
            for (dst, src) in ep0_buf.iter_mut().zip(flattened) {
                *dst = src
            }
            let buffsize = total_length.min(length);
            this.ep0_send(this.ep0_buf.as_ptr() as usize, buffsize, 0);
        }
        USB_DT_STRING => {
            crate::println!("USB_DT_STRING\r\n");
            let id = (value & 0xFF) as u8;
            let len = if id == 0 {
                // index 0 is the language type. This is the hard-coded response for "English".
                ep0_buf[..4].copy_from_slice(&[4, USB_DT_STRING, 9, 4]);
                this.ep0_send(this.ep0_buf.as_ptr() as usize, 4, 0);
                4
            } else {
                let s = match id {
                    1 => MANUFACTURER,
                    2 => PRODUCT,
                    _ => SERIAL,
                };
                // strings are utf16-le encoded words. Manually pack them.
                let len = 2 + s.len() * 2; // 2 bytes for header + string data
                ep0_buf[0] = len as u8;
                ep0_buf[1] = USB_DT_STRING;
                // this code fails if the string isn't simple ASCII. Yes, we could
                // embed idk unicode emoji if I coded this better, but I ask you, WHY‽
                for (dst, &src) in ep0_buf[2..].chunks_exact_mut(2).zip(s.as_bytes()) {
                    dst.copy_from_slice(&(src as u16).to_le_bytes());
                }
                len
            };
            let buffsize = length.min(len);
            this.ep0_send(this.ep0_buf.as_ptr() as usize, buffsize, 0);
        }
        USB_DT_BOS => {
            crate::println!("USB_DT_BOS");
            crate::println!("Not supported, repsonding with stall");
            this.ep_halt(0, USB_RECV);
        }
        _ => {
            crate::println!("UNHANDLED SETUP: 0x{:x}", value >> 8);
            this.ep_halt(0, USB_RECV);
        }
    }
}

fn handle_event(this: &mut CorigineUsb, event_trb: &mut EventTrbS) -> CrgEvent {
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
            crate::println!("{:?}", portsc);

            if portsc.prc() && !portsc.pr() {
                crate::println!("update_current_speed() - reset done");
                this.update_current_speed();
            }
            if portsc.csc() && portsc.ppc() && portsc.pp() && portsc.ccs() {
                crate::println!("update_current_speed() - cable connect");
                this.update_current_speed();
            }

            this.csr.rmwf(EVENTCONFIG_SETUP_ENABLE, 1);
        }
        TrbType::EventTransfer => {
            let comp_code =
                CompletionCode::try_from(event_trb.dw2.compl_code()).expect("Invalid completion code");

            // update the dequeue pointer
            crate::println!("event_transfer {:x?}", event_trb);
            let deq_pt =
                unsafe { (event_trb.dw0 as *mut TransferTrbS).add(1).as_mut().expect("Couldn't deref ptr") };
            if deq_pt.get_trb_type() == TrbType::Link {
                udc_ep.deq_pt = AtomicPtr::new(udc_ep.first_trb.load(Ordering::SeqCst));
            } else {
                udc_ep.deq_pt = AtomicPtr::new(deq_pt as *mut TransferTrbS);
            }
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
                    crate::println!("EP0 unhandled comp_code: {:?}", comp_code);
                    ret = CrgEvent::None;
                }
            } else if pei >= 2 {
                if comp_code == CompletionCode::Success || comp_code == CompletionCode::ShortPacket {
                    crate::println!("EP{} xfer event, dir {}", ep_num, if dir { "OUT" } else { "IN" });
                    // xfer_complete
                    if let Some(f) = this.udc_ep[pei as usize].completion_handler {
                        // so unsafe. so unsafe. We're counting on the hardware to hand us a raw pointer
                        // that isn't corrupted.
                        let p_trb = unsafe { &*(event_trb.dw0 as *const TransferTrbS) };
                        f(this, p_trb.dplo as usize, p_trb.dw2.0, 0);
                    }
                } else if comp_code == CompletionCode::MissedServiceError {
                    crate::println!("MissedServiceError");
                } else {
                    crate::println!("EventTransfer {:?} event not handled", comp_code);
                }
            }
        }
        TrbType::SetupPkt => {
            crate::println!("  handle_setup_pkt");
            let mut setup_storage = [0u8; 8];
            setup_storage.copy_from_slice(&event_trb.get_raw_setup());
            this.setup = Some(setup_storage);
            this.setup_tag = event_trb.get_setup_tag();

            let mut setup_pkt = CtrlRequest::default();
            setup_pkt.as_mut().copy_from_slice(&setup_storage);

            let w_value = setup_pkt.w_value;
            let w_index = setup_pkt.w_index;
            let w_length = setup_pkt.w_length;

            crate::println!(
                "b_request={:x}, b_request_type={:x}, w_value={:04x}, w_index=0x{:x}, w_length={}",
                setup_pkt.b_request,
                setup_pkt.b_request_type,
                w_value,
                w_index,
                w_length
            );

            if (setup_pkt.b_request_type & USB_TYPE_MASK) == USB_TYPE_STANDARD {
                match setup_pkt.b_request {
                    USB_REQ_GET_STATUS => {
                        crate::println!("USB_REQ_GET_STATUS");
                        get_status_request(this, setup_pkt.b_request_type, w_index);
                    }
                    USB_REQ_SET_ADDRESS => {
                        crate::println!("USB_REQ_SET_ADDRESS");
                        this.set_addr(w_value as u8, 0);
                    }
                    USB_REQ_SET_SEL => {
                        crate::println!("USB_REQ_SET_SEL");
                        this.ep0_receive(this.ep0_buf.as_ptr() as usize, w_length as usize, 0);
                        delay(100);
                        /* do set sel */
                        crate::println!("SEL_VALUE NOT HANDLED");
                        /*
                        crg_udc->sel_value.u1_sel_value = *ep0_buf;
                        crg_udc->sel_value.u1_pel_value = *(ep0_buf+1);
                        crg_udc->sel_value.u2_sel_value = *(uint16_t*)(ep0_buf+2);
                        crg_udc->sel_value.u2_pel_value = *(uint16_t*)(ep0_buf+4);
                        */
                    }
                    USB_REQ_SET_ISOCH_DELAY => {
                        crate::println!("USB_REQ_SET_ISOCH_DELAY");
                        /* do set isoch delay */
                        this.ep0_send(0, 0, 0);
                    }
                    USB_REQ_CLEAR_FEATURE => {
                        crate::println!("USB_REQ_CLEAR_FEATURE");
                        crate::println!("*** UNSUPPORTED ***");
                        /* do clear feature */
                        // clear_feature_request(setup_pkt.b_request_type, w_index, w_value);
                    }
                    USB_REQ_SET_FEATURE => {
                        crate::println!("USB_REQ_SET_FEATURE\r\n");
                        crate::println!("*** UNSUPPORTED ***");
                        /* do set feature */
                        /*
                        if crg_udc_get_device_state() == USB_STATE_CONFIGURED {
                            set_feature_request(setup_pkt.b_request_type, w_index, w_value);
                        } else {
                            crg_udc_ep_halt(0, USB_RECV);
                        }
                        */
                    }
                    USB_REQ_SET_CONFIGURATION => {
                        crate::println!("USB_REQ_SET_CONFIGURATION");

                        let mut pass = false;
                        if w_value == 0 {
                            this.set_device_state(UsbDeviceState::Address);
                        } else if w_value == 1 {
                            this.set_device_state(UsbDeviceState::Configured);
                        } else {
                            this.ep_halt(0, USB_RECV);
                            pass = true;
                        }

                        if !pass {
                            this.assign_completion_handler(usb_ep1_bulk_in_complete, 1, USB_SEND);
                            this.assign_completion_handler(usb_ep1_bulk_out_complete, 1, USB_RECV);

                            this.ep_enable(1, USB_SEND, 64, EpType::BulkOutbound);
                            this.ep_enable(1, USB_RECV, 64, EpType::BulkInbound);

                            this.bulk_xfer(1, USB_RECV, this.cbw_ptr(), 31, 0, 0);
                            unsafe { UMS_STATE = UmsState::CommandPhase };
                            this.ep0_send(0, 0, 0);
                        }
                    }
                    USB_REQ_GET_DESCRIPTOR => {
                        crate::println!("USB_REQ_GET_DESCRIPTOR");
                        get_descriptor_request(this, w_value, w_index as usize, w_length as usize);
                    }
                    USB_REQ_GET_CONFIGURATION => {
                        crate::println!("USB_REQ_GET_CONFIGURATION");
                        let ep0_buf = unsafe {
                            core::slice::from_raw_parts_mut(
                                this.ep0_buf.as_ptr() as *mut u8,
                                CRG_UDC_EP0_REQBUFSIZE,
                            )
                        };
                        if this.get_device_state() != UsbDeviceState::Configured {
                            ep0_buf[0] = 0;
                        } else {
                            ep0_buf[0] = 1
                        }
                        this.ep0_send(ep0_buf.as_ptr() as usize, 1, 0);
                    }
                    USB_REQ_SET_INTERFACE => {
                        crate::println!("USB_REQ_SET_INTERFACE");
                        this.cur_interface_num = (w_value & 0xF) as u8;
                        crate::println!("USB_REQ_SET_INTERFACE altsetting {}", this.cur_interface_num);
                        this.ep0_send(0, 0, 0);
                    }
                    USB_REQ_GET_INTERFACE => {
                        crate::println!("USB_REQ_GET_INTERFACE");
                        let ep0_buf = unsafe {
                            core::slice::from_raw_parts_mut(
                                this.ep0_buf.as_ptr() as *mut u8,
                                CRG_UDC_EP0_REQBUFSIZE,
                            )
                        };
                        ep0_buf[0] = this.cur_interface_num;
                        this.ep0_send(ep0_buf.as_ptr() as usize, 1, 0);
                    }
                    _ => {
                        crate::println!(
                            "USB_REQ default b_request=0x{:x}, b_request_type=0x{:x}",
                            setup_pkt.b_request,
                            setup_pkt.b_request_type
                        );
                        this.ep_halt(0, USB_RECV);
                    }
                }
            } else if (setup_pkt.b_request_type & USB_TYPE_MASK) == USB_TYPE_CLASS {
                match setup_pkt.b_request {
                    0xff => {
                        crate::println!("Mass Storage Reset\r\n");
                        if (0 == w_value)
                            && (InterfaceDescriptor::default_mass_storage().b_interface_number
                                == w_index as u8)
                            && (0 == w_length)
                        {
                            this.ep_unhalt(1, USB_SEND);
                            this.ep_unhalt(1, USB_RECV);
                            this.ep_enable(1, USB_SEND, 64, EpType::BulkOutbound);
                            this.ep_enable(1, USB_RECV, 64, EpType::BulkInbound);

                            unsafe { UMS_STATE = UmsState::CommandPhase };
                            this.bulk_xfer(1, USB_RECV, this.cbw_ptr(), 31, 0, 0);
                            //crg_udc_ep0_status(false,0);
                            this.ep0_send(0, 0, 0);
                        } else {
                            this.ep_halt(0, USB_RECV);
                        }
                    }
                    0xfe => {
                        crate::println!("Get Max LUN");
                        if w_index != 0 || w_value != 0 || w_length != 1 {
                            this.ep_halt(0, USB_RECV);
                        } else {
                            let ep0_buf = unsafe {
                                core::slice::from_raw_parts_mut(
                                    this.ep0_buf.as_ptr() as *mut u8,
                                    CRG_UDC_EP0_REQBUFSIZE,
                                )
                            };
                            ep0_buf[0] = 0;
                            this.ep0_send(ep0_buf.as_ptr() as usize, 1, 0);
                        }
                    }
                    _ => {
                        crate::println!("Unhandled!");
                    }
                }
            } else {
                this.ep_halt(0, USB_RECV);
            }

            ret = CrgEvent::Data(0, 0, 1);
        }
        TrbType::DataStage => {
            panic!("data stage needs handling");
        }
        _ => {
            crate::println!("Unexpected trb_type {:?}", event_trb.get_trb_type());
        }
    }
    ret
}

pub fn usb_ep1_bulk_out_complete(_this: &mut CorigineUsb, _buf_addr: usize, _info: u32, _error: u8) {
    crate::println!("bulk_out_complete callback TODO");
    /*
        let length = info & 0xFFFF;
        let buf = unsafe {core::slice::from_raw_parts(buf_addr as *const u8, info as usize & 0xFFFF)};
        let cbw = unsafe { core::slice::from_raw_parts_mut(this.cbw_ptr() as *mut u8, CRG_UDC_APP_BUF_LEN) };

        if unsafe{UmsState::CommandPhase == UMS_STATE} && (length == 31) { //CBW

            memcpy(cbw, buf, 31);
            if(cbw->Signature == BULK_CBW_SIG) {
                csw->Signature = BULK_CSW_SIG;
                csw->Tag = cbw->Tag;
                _process_mass_storage_command(cbw);
                invalid_cbw = 0;
            }
            else {
                crg_udc_ep_halt(1, USB_SEND);
                crg_udc_ep_halt(1, USB_RECV);
                invalid_cbw = 1;
            }
        }
        else if ((UMS_STATE_COMMAND_PHASE == ums_state) && (length != 31)) {
            crg_udc_ep_halt(1, USB_SEND);
            crg_udc_ep_halt(1, USB_RECV);
            invalid_cbw = 1;
        }
        else if(UMS_STATE_DATA_PHASE == ums_state) {  //DATA
            csw->Residue = 0;
            csw->Status = 0;

            crg_udc_bulk_xfer(1, USB_SEND, (uint8_t *)csw, 13, 0, 0);
            ums_state = UMS_STATE_STATUS_PHASE;
        }
    */
}

pub fn usb_ep1_bulk_in_complete(_this: &mut CorigineUsb, _buf_addr: usize, _info: u32, _error: u8) {
    crate::println!("bulk_in_complete callback TODO");
    /*
    uint32_t length = info & 0xFFFF;

    if(UMS_STATE_DATA_PHASE == ums_state) {  //DATA

        crg_udc_bulk_xfer(1, USB_SEND, (uint8_t *)csw, 13, 0, 0);
        ums_state = UMS_STATE_STATUS_PHASE;
    }
    else if(UMS_STATE_STATUS_PHASE == ums_state && length == 13)  //CSW
    {
        crg_udc_bulk_xfer(1, USB_RECV, (uint8_t *)cbw, 31, 0, 0);
        ums_state = UMS_STATE_COMMAND_PHASE;
    }
    */
}
