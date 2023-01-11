//!
//! # Glossary
//!
//! | Term   | Description | More Info |
//! |--------|-------------|-----------|
//! | ZLP    | Zero length packet. Used to terminate the current data transfer when the final packet is full and the total data length is less than the header specified | Section 5.5.3 [USB 2.0 Bus Spec][USB2Bus] |
//! | CBW    | Command block wrapper. Header that contains information about the data that is expected to be sent/received next | Section 5.1 [USB Bulk Only Transport Spec][USBBot] |
//! | CSW    | Command status wrapper. Status sent after data transfer to indicate success/failure and confirm length of data sent | Section 5.2 [USB Bulk Only Transport Spec][USBBot] |
//! | Data Residue | Data residue (bytes) is the difference in the length requested in the CBW and the actual amount of data sent/received | Section 5.2 [USB Bulk Only Transport Spec][USBBot] |
//!
//! [USB2Bus]: https://www.usb.org/document-library/usb-20-specification
//! [USBBot]: https://www.usb.org/document-library/mass-storage-bulk-only-10
//!

#![no_std]

mod msc;
mod interface_subclass;
mod interface_protocol;

pub use usb_device::{Result, UsbError};
pub use msc::*;
pub use interface_subclass::*;
pub use interface_protocol::*;

mod logging {
    pub use log::debug as trace_usb_control;
}
