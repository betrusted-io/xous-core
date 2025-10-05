mod driver;
mod fat32_base;
pub mod glue;
mod handlers;
pub use driver::*;
pub use handlers::*;
mod page_defrag;

use crate::irq::*;

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

// ===== Constants for CDC/IAD and endpoints =====
const USB_DT_CS_INTERFACE: u8 = 0x24;
#[allow(dead_code)]
const USB_DT_CS_ENDPOINT: u8 = 0x25;

// CDC functional descriptor subtypes
const CDC_FD_HEADER: u8 = 0x00;
const CDC_FD_CALL_MANAGEMENT: u8 = 0x01;
const CDC_FD_ACM: u8 = 0x02;
const CDC_FD_UNION: u8 = 0x06;

// CDC class codes
const CDC_COMM_CLASS: u8 = 0x02;
const CDC_COMM_SUBCLASS_ACM: u8 = 0x02;
const CDC_COMM_PROTOCOL_AT: u8 = 0x01; // common ACM

const CDC_DATA_CLASS: u8 = 0x0A;

// Endpoint addresses. Keep MSD on EP1 as you already use.
// CDC notification: EP2 IN
// CDC data: EP3 OUT, EP3 IN
const CDC_NOTIF_EP_IN: u8 = 0x82;
const CDC_DATA_EP_OUT: u8 = 0x03;
const CDC_DATA_EP_IN: u8 = 0x83;

// Packet sizes
const HS_BULK_MPS: u16 = 512;
const FS_BULK_MPS: u16 = 64;
const HS_INT_MPS: u16 = 16; // common value for CDC notif
const FS_INT_MPS: u16 = 8;

// Interrupt intervals
// HS interval is in microframes. 9 gives 2^9 = 512 uframes ≈ 64 ms.
const HS_INT_INTERVAL: u8 = 9;
// FS interval in frames. 10 ≈ 10 ms
const FS_INT_INTERVAL: u8 = 10;

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
const MANUFACTURER: &'static str = "Baochip";
const PRODUCT: &'static str = "Dabao";
// no seriously, do this
const SERIAL: &'static str = "TODO";

impl DeviceDescriptor {
    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn default_composite() -> Self {
        Self {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_DEVICE,
            b_cd_usb: 0x0200,
            b_device_class: 0, // composite via per-interface classing
            b_device_sub_class: 0,
            b_device_protocol: 0,
            b_max_packet_size0: 0x40,
            id_vendor: VENDOR_ID,
            id_product: PRODUCT_ID,
            b_cd_device: 0x0101,
            i_manufacturer: 0x01,
            i_product: 0x02,
            i_serial_number: 0x03,
            b_num_configurations: 1,
        }
    }

    // Windows-friendly composite: EF/02/01 indicates IAD-present composite
    pub fn composite_with_iad() -> Self {
        Self {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_DEVICE,
            b_cd_usb: 0x0200,
            b_device_class: 0xEF,     // Miscellaneous
            b_device_sub_class: 0x02, // Common Class
            b_device_protocol: 0x01,  // Interface Association Descriptor (IAD)
            b_max_packet_size0: 0x40,
            id_vendor: VENDOR_ID,
            id_product: PRODUCT_ID,
            b_cd_device: 0x0101,
            i_manufacturer: 0x01,
            i_product: 0x02,
            i_serial_number: 0x03,
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
    #[allow(dead_code)]
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

    pub fn mass_storage(if_num: u8) -> Self {
        let mut d = InterfaceDescriptor::default_mass_storage();
        d.b_interface_number = if_num;
        d
    }

    pub fn cdc_comm(if_num: u8) -> Self {
        Self {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_INTERFACE,
            b_interface_number: if_num,
            b_alternate_setting: 0,
            b_num_endpoints: 1, // interrupt IN only
            b_interface_class: CDC_COMM_CLASS,
            b_interface_sub_class: CDC_COMM_SUBCLASS_ACM,
            b_interface_protocol: CDC_COMM_PROTOCOL_AT,
            i_interface: 0,
        }
    }

    pub fn cdc_data(if_num: u8) -> Self {
        Self {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_INTERFACE,
            b_interface_number: if_num,
            b_alternate_setting: 0,
            b_num_endpoints: 2, // bulk IN + bulk OUT
            b_interface_class: CDC_DATA_CLASS,
            b_interface_sub_class: 0,
            b_interface_protocol: 0,
            i_interface: 0,
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

    fn cdc_notif_ep_hs() -> Self {
        EndpointDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_ENDPOINT,
            b_endpoint_address: CDC_NOTIF_EP_IN,
            b_m_attributes: 0x03, // Interrupt
            w_max_packet_size: HS_INT_MPS,
            b_interval: HS_INT_INTERVAL,
        }
    }

    fn cdc_notif_ep_fs() -> Self {
        EndpointDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_ENDPOINT,
            b_endpoint_address: CDC_NOTIF_EP_IN,
            b_m_attributes: 0x03,
            w_max_packet_size: FS_INT_MPS,
            b_interval: FS_INT_INTERVAL,
        }
    }

    fn cdc_data_in_hs() -> Self {
        EndpointDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_ENDPOINT,
            b_endpoint_address: CDC_DATA_EP_IN,
            b_m_attributes: 0x02, // Bulk
            w_max_packet_size: HS_BULK_MPS,
            b_interval: 0,
        }
    }

    fn cdc_data_out_hs() -> Self {
        EndpointDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_ENDPOINT,
            b_endpoint_address: CDC_DATA_EP_OUT,
            b_m_attributes: 0x02,
            w_max_packet_size: HS_BULK_MPS,
            b_interval: 0,
        }
    }

    fn cdc_data_in_fs() -> Self {
        EndpointDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_ENDPOINT,
            b_endpoint_address: CDC_DATA_EP_IN,
            b_m_attributes: 0x02,
            w_max_packet_size: FS_BULK_MPS,
            b_interval: 0,
        }
    }

    fn cdc_data_out_fs() -> Self {
        EndpointDescriptor {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_ENDPOINT,
            b_endpoint_address: CDC_DATA_EP_OUT,
            b_m_attributes: 0x02,
            w_max_packet_size: FS_BULK_MPS,
            b_interval: 0,
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
            core::slice::from_raw_parts(handlers::CSW_ADDR as *mut u8, size_of::<Csw>())
        });
        csw
    }

    fn update_hw(&self) {
        let csw_buf = unsafe { core::slice::from_raw_parts_mut(CSW_ADDR as *mut u8, size_of::<Csw>()) };
        csw_buf.copy_from_slice(self.as_ref());
    }

    fn send(&self, usb: &mut bao1x_hal::usb::driver::CorigineUsb) {
        let csw_buf = unsafe { core::slice::from_raw_parts_mut(CSW_ADDR as *mut u8, size_of::<Csw>()) };
        csw_buf.copy_from_slice(self.as_ref());
        usb.bulk_xfer(1, bao1x_hal::usb::driver::USB_SEND, CSW_ADDR, size_of::<Csw>(), 0, 0);
        usb.ms_state = bao1x_hal::usb::driver::UmsState::StatusPhase;
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

// ===== IAD descriptor to group the CDC function =====
#[repr(C, packed)]
struct IadDescriptor {
    b_length: u8,            // 8
    b_descriptor_type: u8,   // 0x0B
    b_first_interface: u8,   // first interface of the function
    b_interface_count: u8,   // number of interfaces in the function
    b_function_class: u8,    // 0x02 (CDC)
    b_function_subclass: u8, // 0x02 (ACM)
    b_function_protocol: u8, // 0x01 (AT)
    i_function: u8,          // string index, 0 if none
}
impl IadDescriptor {
    fn cdc(first_if: u8) -> Self {
        Self {
            b_length: core::mem::size_of::<Self>() as u8,
            b_descriptor_type: USB_DT_INTERFACE_ASSOCIATION,
            b_first_interface: first_if,
            b_interface_count: 2,
            b_function_class: CDC_COMM_CLASS,
            b_function_subclass: CDC_COMM_SUBCLASS_ACM,
            b_function_protocol: CDC_COMM_PROTOCOL_AT,
            i_function: 0,
        }
    }
}
impl AsRef<[u8]> for IadDescriptor {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const IadDescriptor as *const u8,
                core::mem::size_of::<IadDescriptor>(),
            )
        }
    }
}

// ===== CDC functional descriptors =====
#[repr(C, packed)]
struct CdcHeaderFuncDesc {
    b_function_length: u8,    // 5
    b_descriptor_type: u8,    // CS_INTERFACE
    b_descriptor_subtype: u8, // Header
    bcd_cdc: u16,             // 0x0110 or 0x011A; 1.10 is common
}
impl CdcHeaderFuncDesc {
    fn new() -> Self {
        Self {
            b_function_length: 5,
            b_descriptor_type: USB_DT_CS_INTERFACE,
            b_descriptor_subtype: CDC_FD_HEADER,
            bcd_cdc: 0x0110,
        }
    }
}
impl AsRef<[u8]> for CdcHeaderFuncDesc {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const CdcHeaderFuncDesc as *const u8,
                core::mem::size_of::<CdcHeaderFuncDesc>(),
            )
        }
    }
}

#[repr(C, packed)]
struct CdcCallMgmtFuncDesc {
    b_function_length: u8,    // 5
    b_descriptor_type: u8,    // CS_INTERFACE
    b_descriptor_subtype: u8, // Call Management
    bm_capabilities: u8,      // 0x00 device does not handle call mgmt
    b_data_interface: u8,     // interface number of data interface
}
impl CdcCallMgmtFuncDesc {
    fn new(data_if: u8) -> Self {
        Self {
            b_function_length: 5,
            b_descriptor_type: USB_DT_CS_INTERFACE,
            b_descriptor_subtype: CDC_FD_CALL_MANAGEMENT,
            bm_capabilities: 0x00,
            b_data_interface: data_if,
        }
    }
}
impl AsRef<[u8]> for CdcCallMgmtFuncDesc {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const CdcCallMgmtFuncDesc as *const u8,
                core::mem::size_of::<CdcCallMgmtFuncDesc>(),
            )
        }
    }
}

#[repr(C, packed)]
struct CdcAcmFuncDesc {
    b_function_length: u8,    // 4
    b_descriptor_type: u8,    // CS_INTERFACE
    b_descriptor_subtype: u8, // ACM
    bm_capabilities: u8,      // 0x02 supports Set_Line_Coding, etc.
}
impl CdcAcmFuncDesc {
    fn new() -> Self {
        Self {
            b_function_length: 4,
            b_descriptor_type: USB_DT_CS_INTERFACE,
            b_descriptor_subtype: CDC_FD_ACM,
            bm_capabilities: 0x02,
        }
    }
}
impl AsRef<[u8]> for CdcAcmFuncDesc {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const CdcAcmFuncDesc as *const u8,
                core::mem::size_of::<CdcAcmFuncDesc>(),
            )
        }
    }
}

#[repr(C, packed)]
struct CdcUnionFuncDesc {
    b_function_length: u8,    // 5
    b_descriptor_type: u8,    // CS_INTERFACE
    b_descriptor_subtype: u8, // Union
    b_master_interface: u8,   // comm interface number
    b_slave_interface0: u8,   // data interface number
}
impl CdcUnionFuncDesc {
    fn new(master_if: u8, slave_if: u8) -> Self {
        Self {
            b_function_length: 5,
            b_descriptor_type: USB_DT_CS_INTERFACE,
            b_descriptor_subtype: CDC_FD_UNION,
            b_master_interface: master_if,
            b_slave_interface0: slave_if,
        }
    }
}
impl AsRef<[u8]> for CdcUnionFuncDesc {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const CdcUnionFuncDesc as *const u8,
                core::mem::size_of::<CdcUnionFuncDesc>(),
            )
        }
    }
}

// ===== Composite Config descriptors (HS and FS) =====
// Layout:
//   Config
//   IAD (CDC group)
//   IF0 CDC Comm
//     CDC Header, CallMgmt, ACM, Union
//     EP interrupt IN
//   IF1 CDC Data
//     EP bulk OUT, EP bulk IN
//   IF2 MSD
//     EP bulk IN, EP bulk OUT

fn config_total_len_hs() -> usize {
    core::mem::size_of::<ConfigDescriptor>()
    + core::mem::size_of::<IadDescriptor>()
    + core::mem::size_of::<InterfaceDescriptor>()               // CDC Comm
    + core::mem::size_of::<CdcHeaderFuncDesc>()
    + core::mem::size_of::<CdcCallMgmtFuncDesc>()
    + core::mem::size_of::<CdcAcmFuncDesc>()
    + core::mem::size_of::<CdcUnionFuncDesc>()
    + core::mem::size_of::<EndpointDescriptor>()                // CDC notif
    + core::mem::size_of::<InterfaceDescriptor>()               // CDC Data
    + core::mem::size_of::<EndpointDescriptor>() * 2            // CDC data bulk
    + core::mem::size_of::<InterfaceDescriptor>()               // MSD
    + core::mem::size_of::<EndpointDescriptor>() * 2 // MSD bulk
}

fn config_total_len_fs() -> usize {
    // Same structure as HS
    config_total_len_hs()
}

// Writes HS config into ep0_buf and returns the byte count to send
fn write_config_hs(ep0_buf: &mut [u8]) -> usize {
    let if_cdc_comm: u8 = 0;
    let if_cdc_data: u8 = 1;
    let if_msd: u8 = 2;

    let total_len = config_total_len_hs() as u16;

    let config = ConfigDescriptor {
        b_length: core::mem::size_of::<ConfigDescriptor>() as u8,
        b_descriptor_type: USB_DT_CONFIG,
        w_total_length: total_len,
        b_num_interfaces: 3,
        b_configuration_value: 1,
        i_configuration: 0,
        bm_attributes: 0xC0, // self powered, no remote wakeup
        b_max_power: 250,
    };

    let iad = IadDescriptor::cdc(if_cdc_comm);
    let if_comm = InterfaceDescriptor::cdc_comm(if_cdc_comm);
    let fd_hdr = CdcHeaderFuncDesc::new();
    let fd_call = CdcCallMgmtFuncDesc::new(if_cdc_data);
    let fd_acm = CdcAcmFuncDesc::new();
    let fd_union = CdcUnionFuncDesc::new(if_cdc_comm, if_cdc_data);
    let ep_notif = EndpointDescriptor::cdc_notif_ep_hs();

    let if_data = InterfaceDescriptor::cdc_data(if_cdc_data);
    let ep_data_out = EndpointDescriptor::cdc_data_out_hs();
    let ep_data_in = EndpointDescriptor::cdc_data_in_hs();

    let if_msd_desc = InterfaceDescriptor::mass_storage(if_msd);
    let ep_msd_in = EndpointDescriptor::default_mass_storage(MASS_STORAGE_EPADDR_IN, HS_MAX_PKT_SIZE as _);
    let ep_msd_out = EndpointDescriptor::default_mass_storage(MASS_STORAGE_EPADDR_OUT, HS_MAX_PKT_SIZE as _);

    let parts: [&[u8]; 14] = [
        config.as_ref(),
        iad.as_ref(),
        if_comm.as_ref(),
        fd_hdr.as_ref(),
        fd_call.as_ref(),
        fd_acm.as_ref(),
        fd_union.as_ref(),
        ep_notif.as_ref(),
        if_data.as_ref(),
        ep_data_out.as_ref(),
        ep_data_in.as_ref(),
        if_msd_desc.as_ref(),
        ep_msd_in.as_ref(),
        ep_msd_out.as_ref(),
    ];

    let mut idx = 0;
    for p in parts.iter() {
        ep0_buf[idx..idx + p.len()].copy_from_slice(p);
        idx += p.len();
    }
    idx
}

// Writes FS other-speed config into ep0_buf and returns the byte count
fn write_config_fs(ep0_buf: &mut [u8]) -> usize {
    let if_cdc_comm: u8 = 0;
    let if_cdc_data: u8 = 1;
    let if_msd: u8 = 2;

    let total_len = config_total_len_fs() as u16;

    let config = ConfigDescriptor {
        b_length: core::mem::size_of::<ConfigDescriptor>() as u8,
        b_descriptor_type: USB_DT_CONFIG,
        w_total_length: total_len,
        b_num_interfaces: 3,
        b_configuration_value: 1,
        i_configuration: 0,
        bm_attributes: 0xC0,
        b_max_power: 250,
    };

    let iad = IadDescriptor::cdc(if_cdc_comm);
    let if_comm = InterfaceDescriptor::cdc_comm(if_cdc_comm);
    let fd_hdr = CdcHeaderFuncDesc::new();
    let fd_call = CdcCallMgmtFuncDesc::new(if_cdc_data);
    let fd_acm = CdcAcmFuncDesc::new();
    let fd_union = CdcUnionFuncDesc::new(if_cdc_comm, if_cdc_data);
    let ep_notif = EndpointDescriptor::cdc_notif_ep_fs();

    let if_data = InterfaceDescriptor::cdc_data(if_cdc_data);
    let ep_data_out = EndpointDescriptor::cdc_data_out_fs();
    let ep_data_in = EndpointDescriptor::cdc_data_in_fs();

    let if_msd_desc = InterfaceDescriptor::mass_storage(if_msd);
    let ep_msd_in = EndpointDescriptor::default_mass_storage(MASS_STORAGE_EPADDR_IN, FS_MAX_PKT_SIZE as _);
    let ep_msd_out = EndpointDescriptor::default_mass_storage(MASS_STORAGE_EPADDR_OUT, FS_MAX_PKT_SIZE as _);

    let parts: [&[u8]; 14] = [
        config.as_ref(),
        iad.as_ref(),
        if_comm.as_ref(),
        fd_hdr.as_ref(),
        fd_call.as_ref(),
        fd_acm.as_ref(),
        fd_union.as_ref(),
        ep_notif.as_ref(),
        if_data.as_ref(),
        ep_data_out.as_ref(),
        ep_data_in.as_ref(),
        if_msd_desc.as_ref(),
        ep_msd_in.as_ref(),
        ep_msd_out.as_ref(),
    ];

    let mut idx = 0;
    for p in parts.iter() {
        ep0_buf[idx..idx + p.len()].copy_from_slice(p);
        idx += p.len();
    }
    idx
}
