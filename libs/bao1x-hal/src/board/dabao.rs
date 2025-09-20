// Constants that define pin locations, RAM offsets, etc. for the DaBao basic breakout board

use bao1x_api::*;

// console uart buffer
pub const UART_DMA_TX_BUF_PHYS: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4096;

// RAM needs two buffers of 1k + 16 bytes = 2048 + 16 = 2064 bytes; round up to one page
pub const SPIM_RAM_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 2 * 4096;

// app uart buffer
pub const APP_UART_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 3 * 4096;

// one page for the I2C driver
pub const I2C_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4 * 4096;

// USB pages - USB subsystem is a hog, needs a lot of pages, note this is mutually exclusive with camera,
// can't use both at once
pub const CRG_IFRAM_PAGES: usize = 23; // +1 for extended application buffer by 4k
pub const CRG_UDC_MEMBASE: usize =
    utralib::HW_IFRAM1_MEM + utralib::HW_IFRAM1_MEM_LEN - CRG_IFRAM_PAGES * 0x1000;

// MANUALLY SYNCED TO ALLOCATIONS ABOVE
// inclusive numbering - we allocate pages from the top-down, so the last number should generally be 31
pub const IFRAM0_RESERVED_PAGE_RANGE: [usize; 2] = [31 - 4, 31];
pub const IFRAM1_RESERVED_PAGE_RANGE: [usize; 2] = [31 - 0, 31];

/// returns the USB SE0 port and pin number
const SE0_PIN: u8 = 14;
pub fn setup_usb_pins<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
    iox.setup_pin(
        IoxPort::PB,
        SE0_PIN,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    iox.set_gpio_pin_value(IoxPort::PB, SE0_PIN, IoxValue::Low);
    (IoxPort::PB, SE0_PIN)
}

pub fn setup_i2c_pins(iox: &dyn IoSetup) -> bao1x_api::I2cChannel {
    // I2C_SCL_B[0]
    iox.setup_pin(
        IoxPort::PB,
        11,
        Some(IoxDir::Output),
        Some(IoxFunction::AF1),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    // I2C_SDA_B[0]
    iox.setup_pin(
        IoxPort::PB,
        12,
        Some(IoxDir::Output),
        Some(IoxFunction::AF1),
        Some(IoxEnable::Enable),
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    bao1x_api::I2cChannel::Channel0
}

// these are a bodge to allow some UDMA imports to work - they need to be present even if
// there currently no SPINOR devices attached.
pub const SPINOR_PAGE_LEN: u32 = 0x100;
pub const SPINOR_ERASE_SIZE: u32 = 0x1000; // this is the smallest sector size.
pub const SPINOR_BULK_ERASE_SIZE: u32 = 0x1_0000; // this is the bulk erase size.
pub const SPINOR_LEN: u32 = 16384 * 1024;
