// Constants that define pin locations, RAM offsets, etc. for the BaoSec board
use crate::iox;
use crate::iox::*;
use crate::iox::{IoIrq, IoSetup};

pub const I2C_AXP2101_ADR: u8 = 0x34;
pub const I2C_TUSB320_ADR: u8 = 0x47;
pub const I2C_BQ27427_ADR: u8 = 0x55;

// console uart buffer
pub const UART_DMA_TX_BUF_PHYS: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4096;

// RAM needs two buffers of 1k + 16 bytes = 2048 + 16 = 2064 bytes; round up to one page
pub const SPIM_RAM_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 2 * 4096;

// app uart buffer
pub const APP_UART_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 3 * 4096;

// display buffer: 1 page for double-buffering, rounded up to 1 page for commands
pub const DISPLAY_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 5 * 4096;

// Flash needs 4096 bytes for Rx, and 0 bytes for Tx + 16 bytes for cmd for 2 pages total. This is released
// after boot.
pub const SPIM_FLASH_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 7 * 4096;

// one page for the I2C driver
pub const I2C_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 8 * 4096;

// memory for camera driver - note this is mutually exclusive with USB, can't use both at the same time
pub const CAM_IFRAM_LEN_PAGES: usize = 30;
pub const CAM_IFRAM_ADDR: usize =
    utralib::HW_IFRAM1_MEM + utralib::HW_IFRAM1_MEM_LEN - CAM_IFRAM_LEN_PAGES * 4096;

// USB pages - USB subsystem is a hog, needs a lot of pages, note this is mutually exclusive with camera,
// can't use both at once
pub const CRG_IFRAM_PAGES: usize = 23; // +1 for extended application buffer by 4k
pub const CRG_UDC_MEMBASE: usize =
    utralib::HW_IFRAM1_MEM + utralib::HW_IFRAM1_MEM_LEN - CRG_IFRAM_PAGES * 0x1000;

// MANUALLY SYNCED TO ALLOCATIONS ABOVE
// inclusive numbering - we allocate pages from the top-down, so the last number should generally be 31
pub const IFRAM0_RESERVED_PAGE_RANGE: [usize; 2] = [31 - 9, 31];
pub const IFRAM1_RESERVED_PAGE_RANGE: [usize; 2] = [31 - CAM_IFRAM_LEN_PAGES, 31];

/// Setup pins for the baosec display
/// Returns a spi channel object and descriptor for the C/D + CS pins as a (port, c/d pin, cs pin) tuple
pub fn setup_display_pins(iox: &dyn IoSetup) -> (crate::udma::SpimChannel, iox::IoxPort, u8, u8) {
    const SPI_CS_PIN: u8 = 3;
    const SPI_CLK_PIN: u8 = 0;
    const SPI_DAT_PIN: u8 = 1;
    const SPI_CD_PIN: u8 = 2;
    const SPI_PORT: iox::IoxPort = iox::IoxPort::PC;

    // SPIM_CLK_B[2]
    iox.setup_pin(
        SPI_PORT,
        SPI_CLK_PIN,
        Some(iox::IoxDir::Output),
        Some(iox::IoxFunction::AF2),
        None,
        None,
        Some(iox::IoxEnable::Enable),
        Some(iox::IoxDriveStrength::Drive2mA),
    );
    // SPIM_SD0_B[2]
    iox.setup_pin(
        SPI_PORT,
        SPI_DAT_PIN,
        Some(iox::IoxDir::Output),
        Some(iox::IoxFunction::AF2),
        None,
        None,
        Some(iox::IoxEnable::Enable),
        Some(iox::IoxDriveStrength::Drive2mA),
    );
    // SPIM_CSN0_B[2]
    iox.setup_pin(
        SPI_PORT,
        SPI_CS_PIN,
        Some(iox::IoxDir::Output),
        Some(iox::IoxFunction::AF2),
        None,
        Some(iox::IoxEnable::Enable),
        Some(iox::IoxEnable::Enable),
        Some(iox::IoxDriveStrength::Drive2mA),
    );
    // C/D pin is a gpio direct-drive
    iox.setup_pin(
        SPI_PORT,
        SPI_CD_PIN,
        Some(iox::IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(iox::IoxDriveStrength::Drive2mA),
    );
    // using bank SPIM_B[2]
    (crate::udma::SpimChannel::Channel2, SPI_PORT, SPI_CD_PIN, SPI_CS_PIN)
}

pub fn setup_memory_pins(iox: &dyn IoSetup) -> crate::udma::SpimChannel {
    // JPC7_13
    // SPIM_CLK_A[1]
    iox.setup_pin(
        IoxPort::PC,
        11,
        Some(IoxDir::Output),
        Some(IoxFunction::AF1),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive4mA),
    );
    // SPIM_SD[0-3]_A[1]
    for i in 7..11 {
        iox.setup_pin(
            IoxPort::PC,
            i,
            None,
            Some(IoxFunction::AF1),
            None,
            None,
            Some(IoxEnable::Enable),
            Some(IoxDriveStrength::Drive2mA),
        );
    }
    // SPIM_CSN0_A[1]
    iox.setup_pin(
        IoxPort::PC,
        12,
        Some(IoxDir::Output),
        Some(IoxFunction::AF1),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    // SPIM_CSN1_A[1]
    iox.setup_pin(
        IoxPort::PC,
        13,
        Some(IoxDir::Output),
        Some(IoxFunction::AF1),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    crate::udma::SpimChannel::Channel1
}

/// This also sets up I2C-adjacent interrupt inputs as well
pub fn setup_i2c_pins(iox: &dyn IoSetup) -> crate::udma::I2cChannel {
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
    // PB13 -> PMIC IRQ
    iox.setup_pin(
        IoxPort::PB,
        13,
        Some(IoxDir::Input),
        Some(IoxFunction::Gpio),
        Some(IoxEnable::Enable),
        Some(IoxEnable::Enable),
        None,
        None,
    );
    crate::udma::I2cChannel::Channel0
}

/// returns the power-down port and pin number
pub fn setup_ov2640_pins<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
    // power-down pin - default to powered down
    iox.set_gpio_pin_value(IoxPort::PC, 14, IoxValue::High);
    iox.setup_pin(
        IoxPort::PC,
        14,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    // camera interface proper
    for pin in 2..11 {
        iox.setup_pin(
            IoxPort::PB,
            pin,
            Some(IoxDir::Input),
            Some(IoxFunction::AF1),
            None,
            None,
            Some(IoxEnable::Enable),
            Some(IoxDriveStrength::Drive2mA),
        );
    }
    (IoxPort::PC, 14)
}

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

// These constants definitely change for NTO. These will only work on NTO.
const KB_PORT: IoxPort = IoxPort::PD;
const R_PINS: [u8; 3] = [0, 1, 4];
const C_PINS: [u8; 3] = [5, 6, 7];
pub fn setup_kb_pins<T: IoSetup + IoGpio>(iox: &T) -> ([(IoxPort, u8); 3], [(IoxPort, u8); 3]) {
    for r in R_PINS {
        iox.setup_pin(
            KB_PORT,
            r,
            Some(IoxDir::Output),
            Some(IoxFunction::Gpio),
            None,
            None,
            Some(IoxEnable::Enable),
            Some(IoxDriveStrength::Drive2mA),
        );
        iox.set_gpio_pin_value(KB_PORT, r, IoxValue::High);
    }

    for c in C_PINS {
        iox.setup_pin(
            KB_PORT,
            c,
            Some(IoxDir::Input),
            Some(IoxFunction::Gpio),
            Some(IoxEnable::Enable),
            None,
            Some(IoxEnable::Enable),
            Some(IoxDriveStrength::Drive2mA),
        );
    }
    ([(KB_PORT, R_PINS[0]), (KB_PORT, R_PINS[1]), (KB_PORT, R_PINS[2])], [
        (KB_PORT, C_PINS[0]),
        (KB_PORT, C_PINS[1]),
        (KB_PORT, C_PINS[2]),
    ])
}

pub fn setup_pmic_irq<T: IoIrq>(iox: &T, server: &str, opcode: usize) {
    iox.set_irq_pin(IoxPort::PB, 13, IoxValue::Low, server, opcode);
}
