use corigine_usb::CORIGINE_USB_BASE;

pub fn usb_test() {
    let mut csr = utralib::CSR::new(CORIGINE_USB_BASE as *mut u32);

    crate::println!("devcap: {:x}", csr.r(corigine_usb::DEVCAP));
    crate::println!("max speed: {:x}", csr.rf(corigine_usb::DEVCONFIG_MAX_SPEED));
    crate::println!("usb3 disable: {:x}", csr.rf(corigine_usb::DEVCONFIG_USB3_DISABLE_COUNT));
}

#[allow(dead_code)]
pub mod corigine_usb {
    use utralib::{Field, Register};

    pub const DEVCAP: Register = Register::new(0, 0xffffffff);
    pub const DEVCAP_VESION: Field = Field::new(8, 0, DEVCAP);
    pub const DEVCAP_EP_IN: Field = Field::new(4, 8, DEVCAP);
    pub const DEVCAP_EP_OUT: Field = Field::new(4, 12, DEVCAP);
    pub const DEVCAP_MAX_INTS: Field = Field::new(10, 16, DEVCAP);
    pub const DEVCAP_GEN1: Field = Field::new(1, 27, DEVCAP);
    pub const DEVCAP_GEN2: Field = Field::new(1, 28, DEVCAP);
    pub const DEVCAP_ISOCH: Field = Field::new(1, 29, DEVCAP);

    pub const DEVCONFIG: Register = Register::new(0x10 / 4, 0xFF);
    pub const DEVCONFIG_MAX_SPEED: Field = Field::new(4, 0, DEVCONFIG);
    pub const DEVCONFIG_USB3_DISABLE_COUNT: Field = Field::new(4, 4, DEVCONFIG);

    pub const CORIGINE_USB_BASE: usize = 0x5020_2400;
}
