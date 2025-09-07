mod driver;
pub mod glue;
mod mass_storage;
mod sha512_digest;
mod slice_cursor;

pub use driver::*;
pub use mass_storage::*;
pub use slice_cursor::*;

use crate::irq::*;

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
#[allow(dead_code)]
const USB_TYPE_VENDOR: u8 = 0x02 << 5;
#[allow(dead_code)]
const USB_TYPE_RESERVED: u8 = 0x03 << 5;
/*
 * USB recipients, the third of three bRequestType fields
 */
#[allow(dead_code)]
const USB_RECIP_MASK: u8 = 0x1f;
const USB_RECIP_DEVICE: u8 = 0x00;
const USB_RECIP_INTERFACE: u8 = 0x01;
const USB_RECIP_ENDPOINT: u8 = 0x02;
#[allow(dead_code)]
const USB_RECIP_OTHER: u8 = 0x03;

const USB_REQ_GET_STATUS: u8 = 0x00;
const USB_REQ_CLEAR_FEATURE: u8 = 0x01;
const USB_REQ_SET_FEATURE: u8 = 0x03;
const USB_REQ_SET_ADDRESS: u8 = 0x05;
const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;
#[allow(dead_code)]
const USB_REQ_SET_DESCRIPTOR: u8 = 0x07;
const USB_REQ_GET_CONFIGURATION: u8 = 0x08;
const USB_REQ_SET_CONFIGURATION: u8 = 0x09;
const USB_REQ_GET_INTERFACE: u8 = 0x0A;
const USB_REQ_SET_INTERFACE: u8 = 0x0B;
#[allow(dead_code)]
const USB_REQ_SYNCH_FRAME: u8 = 0x0C;
const USB_REQ_SET_SEL: u8 = 0x30;
const USB_REQ_SET_ISOCH_DELAY: u8 = 0x31;

const USB_DT_DEVICE: u8 = 0x01;
const USB_DT_CONFIG: u8 = 0x02;
const USB_DT_STRING: u8 = 0x03;
#[allow(dead_code)]
const USB_DT_INTERFACE: u8 = 0x04;
const USB_DT_ENDPOINT: u8 = 0x05;
const USB_DT_DEVICE_QUALIFIER: u8 = 0x06;
const USB_DT_OTHER_SPEED_CONFIG: u8 = 0x07;
#[allow(dead_code)]
const USB_DT_INTERFACE_POWER: u8 = 0x08;

/* these are from a minor usb 2.0 revision (ECN) */
#[allow(dead_code)]
const USB_DT_OTG: u8 = 0x09;
#[allow(dead_code)]
const USB_DT_DEVICE_CAPABILITY: u8 = 0x10;
#[allow(dead_code)]
const USB_DT_DEBUG: u8 = 0x0a;
#[allow(dead_code)]
const USB_DT_INTERFACE_ASSOCIATION: u8 = 0x0b;
#[allow(dead_code)]
const USB_DT_BOS: u8 = 0x0f;

/* From the T10 UAS specification */
#[allow(dead_code)]
const USB_DT_PIPE_USAGE: u8 = 0x24;
/* From the USB 3.0 spec */
#[allow(dead_code)]
const USB_DT_SS_ENDPOINT_COMP: u8 = 0x30;
/* From the USB 3.1 spec */
#[allow(dead_code)]
const USB_DT_SSP_ISOC_ENDPOINT_COMP: u8 = 0x31;

const USB_CAP_TYPE_EXT: u8 = 0x2;

#[allow(dead_code)]
#[repr(C, packed)]
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
#[allow(dead_code)]
#[repr(C, packed)]
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

#[allow(dead_code)]
#[repr(C, packed)]
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

#[allow(dead_code)]
#[repr(C, packed)]
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
            b_descriptor_type: USB_DT_INTERFACE,
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
#[allow(dead_code)]
#[repr(C, packed)]
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

#[repr(C, packed)]
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

#[allow(dead_code)]
#[repr(C, packed)]
struct BosDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub w_total_length: u16,
    pub b_num_device_caps: u8,
}
impl BosDescriptor {
    pub fn default_mass_storage(total_length: u16, num_caps: u8) -> Self {
        BosDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_BOS,
            w_total_length: total_length,
            b_num_device_caps: num_caps,
        }
    }
}

impl AsRef<[u8]> for BosDescriptor {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const BosDescriptor as *const u8,
                core::mem::size_of::<BosDescriptor>(),
            ) as &[u8]
        }
    }
}

#[allow(dead_code)]
#[repr(C, packed)]
struct ExtCapDescriptor {
    pub b_length: u8,
    pub b_descriptor_type: u8,
    pub b_dev_capability_type: u8,
    pub b_mattributes: u32,
}
impl ExtCapDescriptor {
    pub fn default_mass_storage(attributes: u32) -> Self {
        ExtCapDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_DEVICE_CAPABILITY,
            b_dev_capability_type: USB_CAP_TYPE_EXT,
            b_mattributes: attributes,
        }
    }
}

impl AsRef<[u8]> for ExtCapDescriptor {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const ExtCapDescriptor as *const u8,
                core::mem::size_of::<ExtCapDescriptor>(),
            ) as &[u8]
        }
    }
}

#[allow(dead_code)]
pub(crate) const USB_LPM_SUPPORT: u8 = 1 << 1; /* supports LPM */
#[allow(dead_code)]
pub(crate) const USB_BESL_SUPPORT: u8 = 1 << 2; /* supports BESL */
#[allow(dead_code)]
pub(crate) const USB_BESL_BASELINE_VALID: u8 = 1 << 3; /* Baseline BESL valid*/
#[allow(dead_code)]
pub(crate) const USB_BESL_DEEP_VALID: u8 = 1 << 4; /* Deep BESL valid */

#[repr(C, packed)]
#[derive(Default, Debug)]
struct Cbw {
    signature: u32,            // Contains 'USBC'
    tag: u32,                  // Unique per command id
    data_transfer_length: u32, // Size of the data
    flags: u8,                 // Direction in bit 7
    lun: u8,                   // LUN (normally 0)
    length: u8,                // Of the CDB, <= MAX_COMMAND_SIZE
    cdb: [u8; 16],             // Command Data Block
}
impl AsMut<[u8]> for Cbw {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut Cbw as *mut u8, core::mem::size_of::<Cbw>())
                as &mut [u8]
        }
    }
}
impl AsRef<[u8]> for Cbw {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const Cbw as *const u8, core::mem::size_of::<Cbw>())
                as &[u8]
        }
    }
}

const BULK_CBW_SIG: u32 = 0x43425355; /* Spells out USBC */

#[repr(C, packed)]
#[derive(Default)]
struct Csw {
    pub signature: u32, // Should = 'USBS'
    pub tag: u32,       // Same as original command
    pub residue: u32,   // Amount not transferred
    pub status: u8,     // See below
}
impl Csw {
    fn derive() -> Csw {
        let mut csw = Csw::default();
        csw.as_mut().copy_from_slice(unsafe {
            core::slice::from_raw_parts(mass_storage::CSW_ADDR as *mut u8, size_of::<Csw>())
        });
        csw
    }

    fn update_hw(&self) {
        let csw_buf = unsafe { core::slice::from_raw_parts_mut(CSW_ADDR as *mut u8, size_of::<Csw>()) };
        csw_buf.copy_from_slice(self.as_ref());
    }

    fn send(&self, usb: &mut cramium_hal::usb::driver::CorigineUsb) {
        let csw_buf = unsafe { core::slice::from_raw_parts_mut(CSW_ADDR as *mut u8, size_of::<Csw>()) };
        csw_buf.copy_from_slice(self.as_ref());
        usb.bulk_xfer(1, cramium_hal::usb::driver::USB_SEND, CSW_ADDR, size_of::<Csw>(), 0, 0);
        usb.ms_state = cramium_hal::usb::driver::UmsState::StatusPhase;
    }
}
const BULK_CSW_SIG: u32 = 0x53425355; /* Spells out 'USBS' */

impl AsRef<[u8]> for Csw {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const Csw as *const u8, core::mem::size_of::<Csw>())
                as &[u8]
        }
    }
}

impl AsMut<[u8]> for Csw {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self as *mut Csw as *mut u8, core::mem::size_of::<Csw>())
                as &mut [u8]
        }
    }
}

#[repr(C, packed)]
struct InquiryResponse {
    pub peripheral_device_type: u8,       // Byte 0: Peripheral Device Type (PDT)
    pub rmb: u8,                          // Byte 1: Removable Media Bit (RMB) and Device Type Modifier
    pub version: u8,                      // Byte 2: ISO/ECMA/ANSI Version
    pub response_data_format: u8,         // Byte 3: Response Data Format (RDF) and capabilities
    pub additional_length: u8,            // Byte 4: Additional Length (number of bytes after byte 7)
    pub reserved1: u8,                    // Byte 5: Reserved
    pub reserved2: u8,                    // Byte 6: Reserved
    pub reserved3: u8,                    // Byte 7: Reserved
    pub vendor_identification: [u8; 8],   // Byte 8-15: Vendor Identification (ASCII)
    pub product_identification: [u8; 16], // Byte 16-31: Product Identification (ASCII)
    pub product_revision_level: [u8; 4],  // Byte 32-35: Product Revision Level (ASCII)
}
impl AsRef<[u8]> for InquiryResponse {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const InquiryResponse as *const u8,
                core::mem::size_of::<InquiryResponse>(),
            ) as &[u8]
        }
    }
}
