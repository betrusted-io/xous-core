// Constants that define pin locations, RAM offsets, etc. for the BaoSec board
use bao1x_api::*;

pub const DEFAULT_FCLK_FREQUENCY: u32 = bao1x_api::offsets::baosec::DEFAULT_FCLK_FREQUENCY;
pub const DEFAULT_CPU_VOLTAGE_MV: u32 = 820;
pub const VDD85_SWITCH_MARGIN_MV: u32 = 20; // margin, in mV, for the transistor power switch

pub const I2C_AXP2101_ADR: u8 = 0x34;
pub const I2C_TUSB320_ADR: u8 = 0x47;
pub const I2C_BQ27427_ADR: u8 = 0x55;

// re-export these constants from the API crate
// the API crate has to list *all* offsets, not just those targeting the
// current build. This re-export allows us to have "generic" offsets
// independent of builds.
pub const SPINOR_PAGE_LEN: u32 = bao1x_api::offsets::baosec::SPINOR_PAGE_LEN;
pub const SPINOR_ERASE_SIZE: u32 = bao1x_api::offsets::baosec::SPINOR_ERASE_SIZE;
pub const SPINOR_BULK_ERASE_SIZE: u32 = bao1x_api::offsets::baosec::SPINOR_BULK_ERASE_SIZE;
pub const SPINOR_LEN: u32 = bao1x_api::offsets::baosec::SPI_FLASH_LEN as _;
pub const PDDB_LOC: u32 = bao1x_api::offsets::baosec::PDDB_ORIGIN as _;
pub const PDDB_LEN: u32 = bao1x_api::offsets::baosec::PDDB_LEN as _;

// Define the virtual region that memory-mapped FLASH should go to
// top 8 megs are reserved for staging updates, backups, etc.
pub const MMAP_VIRT_LEN: usize = SPINOR_LEN as usize;
pub const MMAP_VIRT_END: usize = xous::arch::MMAP_VIRT_BASE + SPINOR_LEN as usize;

// console uart buffer
pub const UART_DMA_TX_BUF_PHYS: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4096;

// RAM needs two buffers of 1k + 16 bytes = 2048 + 16 = 2064 bytes; round up to one page
pub const SPIM_RAM_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 2 * 4096;

// app uart buffer
pub const APP_UART_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 3 * 4096;

// display buffer: 1 page for double-buffering, rounded up to 1 page for commands
pub const DISPLAY_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 5 * 4096;

// Flash needs 4096 bytes for Rx, and 0 or 256 bytes for Tx + 16 bytes for cmd for 2 pages total.
pub const SPIM_FLASH_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 7 * 4096;

// one page for the I2C driver
pub const I2C_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 8 * 4096;

// USB pages
pub const CRG_IFRAM_PAGES: usize = 5;
pub const CRG_UDC_MEMBASE: usize = I2C_IFRAM_ADDR - CRG_IFRAM_PAGES * 0x1000;

// memory for camera driver
pub const CAM_IFRAM_LEN_PAGES: usize = 30;
pub const CAM_IFRAM_ADDR: usize =
    utralib::HW_IFRAM1_MEM + utralib::HW_IFRAM1_MEM_LEN - CAM_IFRAM_LEN_PAGES * 4096;

// MANUALLY SYNCED TO ALLOCATIONS ABOVE
// inclusive numbering - we allocate pages from the top-down, so the last number should generally be 31
pub const IFRAM0_RESERVED_PAGE_RANGE: [usize; 2] = [31 - 9 - CRG_IFRAM_PAGES, 31];
pub const IFRAM1_RESERVED_PAGE_RANGE: [usize; 2] = [31 - CAM_IFRAM_LEN_PAGES, 31];

// Re-export all of the offsets exposed in the API
pub use bao1x_api::offsets::baosec::*;
pub use bao1x_api::offsets::*;

// Display pins
const SPI_CS_PIN: u8 = 3;
const SPI_CLK_PIN: u8 = 0;
const SPI_DAT_PIN: u8 = 1;
const SPI_CD_PIN: u8 = 2;
const SPI_PORT: IoxPort = IoxPort::PC;

pub const SPI_MEM_CHANNEL: SpimChannel = SpimChannel::Channel1;

/// Returns just the pin mappings without setting anything up.
pub fn get_display_pins() -> (SpimChannel, IoxPort, u8, u8) {
    (SpimChannel::Channel2, SPI_PORT, SPI_CD_PIN, SPI_CS_PIN)
}
/// Setup pins for the baosec display
/// Returns a spi channel object and descriptor for the C/D + CS pins as a (port, c/d pin, cs pin) tuple
pub fn setup_display_pins(iox: &dyn IoSetup) -> (SpimChannel, IoxPort, u8, u8) {
    // SPIM_CLK_B[2]
    iox.setup_pin(
        SPI_PORT,
        SPI_CLK_PIN,
        Some(IoxDir::Output),
        Some(IoxFunction::AF2),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    // SPIM_SD0_B[2]
    iox.setup_pin(
        SPI_PORT,
        SPI_DAT_PIN,
        Some(IoxDir::Output),
        Some(IoxFunction::AF2),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    // SPIM_CSN0_B[2]
    iox.setup_pin(
        SPI_PORT,
        SPI_CS_PIN,
        Some(IoxDir::Output),
        Some(IoxFunction::AF2),
        None,
        Some(IoxEnable::Enable),
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    // C/D pin is a gpio direct-drive
    iox.setup_pin(
        SPI_PORT,
        SPI_CD_PIN,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    // using bank SPIM_B[2]
    get_display_pins()
}

pub fn setup_memory_pins(iox: &dyn IoSetup) -> SpimChannel {
    // JPC7_13
    // SPIM_CLK_A[1]
    iox.setup_pin(
        IoxPort::PC,
        11,
        Some(IoxDir::Output),
        Some(IoxFunction::AF1),
        None,
        None,
        Some(IoxEnable::Disable),
        Some(IoxDriveStrength::Drive12mA),
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
            Some(IoxEnable::Disable),
            Some(IoxDriveStrength::Drive8mA),
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
        Some(IoxEnable::Disable),
        Some(IoxDriveStrength::Drive8mA),
    );
    // SPIM_CSN1_A[1]
    iox.setup_pin(
        IoxPort::PC,
        13,
        Some(IoxDir::Output),
        Some(IoxFunction::AF1),
        None,
        None,
        Some(IoxEnable::Disable),
        Some(IoxDriveStrength::Drive8mA),
    );
    SPI_MEM_CHANNEL
}

/// This also sets up I2C-adjacent interrupt inputs as well
pub fn setup_i2c_pins(iox: &dyn IoSetup) -> I2cChannel {
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
    // PB15 -> PMIC IRQ
    iox.setup_pin(
        IoxPort::PB,
        15,
        Some(IoxDir::Input),
        Some(IoxFunction::Gpio),
        Some(IoxEnable::Enable),
        Some(IoxEnable::Enable),
        None,
        None,
    );
    I2cChannel::Channel0
}

/// returns the power-down port and pin number
pub fn setup_camera_pins<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
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
    // power-down pin - default to powered down
    iox.set_gpio_pin_value(IoxPort::PC, 14, IoxValue::High);
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

pub fn setup_periph_reset_pin<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
    let (port, pin) = (IoxPort::PC, 6);
    iox.setup_pin(
        port,
        pin,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        Some(IoxEnable::Enable),
        None,
        Some(IoxDriveStrength::Drive2mA),
    );
    (port, pin)
}

/// returns the USB SE0 port and pin number
pub fn setup_usb_pins<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
    const SE0_PIN: u8 = 5;
    const SE0_PORT: IoxPort = IoxPort::PF;
    iox.setup_pin(
        SE0_PORT,
        SE0_PIN,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    (SE0_PORT, SE0_PIN)
}

const KB_PORT: IoxPort = IoxPort::PF;
const R_PINS: [u8; 2] = [6, 7];
const C_PINS: [u8; 3] = [2, 3, 4];
pub fn setup_kb_pins<T: IoSetup + IoGpio>(iox: &T) -> ([(IoxPort, u8); 2], [(IoxPort, u8); 3]) {
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
    (
        [(KB_PORT, R_PINS[0]), (KB_PORT, R_PINS[1])],
        [(KB_PORT, C_PINS[0]), (KB_PORT, C_PINS[1]), (KB_PORT, C_PINS[2])],
    )
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyPress {
    Up,
    Down,
    Left,
    Right,
    Select,
    Home,
    Invalid,
    None,
}

pub fn scan_keyboard<T: IoSetup + IoGpio>(
    iox: &T,
    rows: &[(IoxPort, u8)],
    cols: &[(IoxPort, u8)],
) -> [KeyPress; 4] {
    let mut key_presses: [KeyPress; 4] = [KeyPress::None; 4];
    let mut key_press_index = 0; // no Vec in no_std, so we have to manually track it

    for (row, (port, pin)) in rows.iter().enumerate() {
        iox.set_gpio_pin_value(*port, *pin, IoxValue::Low);
        for (col, (col_port, col_pin)) in cols.iter().enumerate() {
            if iox.get_gpio_pin_value(*col_port, *col_pin) == IoxValue::Low {
                crate::println!("Key press at ({}, {})", row, col);
                if key_press_index < key_presses.len() {
                    key_presses[key_press_index] = match (row, col) {
                        (1, 3) => KeyPress::Left,
                        (1, 2) => KeyPress::Home,
                        (1, 0) => KeyPress::Right,
                        (0, 0) => KeyPress::Down,
                        (0, 2) => KeyPress::Up,
                        (0, 1) => KeyPress::Select,
                        _ => KeyPress::Invalid,
                    };
                    key_press_index += 1;
                }
            }
        }
        iox.set_gpio_pin_value(*port, *pin, IoxValue::High);
    }
    key_presses
}

pub fn setup_pmic_irq<T: IoIrq>(iox: &T, server: &str, opcode: usize) {
    iox.set_irq_pin(IoxPort::PB, 15, IoxValue::Low, server, opcode);
}

pub fn setup_oled_power_pin<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
    let (port, pin) = (IoxPort::PC, 4);
    iox.setup_pin(
        port,
        pin,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        Some(IoxEnable::Disable),
        None,
        Some(IoxDriveStrength::Drive2mA),
    );
    (port, pin)
}

pub fn setup_trng_power_pin<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
    let (port, pin) = (IoxPort::PC, 5);
    iox.setup_pin(
        port,
        pin,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        Some(IoxEnable::Disable),
        None,
        Some(IoxDriveStrength::Drive2mA),
    );
    (port, pin)
}

pub fn setup_trng_input_pin<T: IoSetup + IoGpio>(iox: &T) -> u8 {
    let (port, pin) = (IoxPort::PC, 15);
    iox.setup_pin(
        port,
        pin,
        Some(IoxDir::Input),
        Some(IoxFunction::Gpio),
        Some(IoxEnable::Enable), // enable the schmitt trigger on this pad
        Some(IoxEnable::Disable),
        None,
        Some(IoxDriveStrength::Drive2mA),
    );
    iox.set_bio_bit_from_port_and_pin(port, pin).expect("Couldn't allocate TRNG input pin")
}

pub fn setup_dcdc2_pin<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
    let (port, pin) = (IoxPort::PF, 0);
    iox.setup_pin(
        port,
        pin,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        Some(IoxEnable::Disable),
        None,
        Some(IoxDriveStrength::Drive2mA),
    );
    (port, pin)
}

pub fn setup_console_pins<T: IoSetup + IoGpio>(iox: &T) -> PeriphId {
    iox.setup_pin(
        IoxPort::PB,
        13,
        Some(IoxDir::Input),
        Some(IoxFunction::AF1),
        Some(IoxEnable::Enable),
        Some(IoxEnable::Enable),
        None,
        None,
    );
    iox.setup_pin(
        IoxPort::PB,
        14,
        Some(IoxDir::Output),
        Some(IoxFunction::AF1),
        None,
        None,
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive4mA),
    );
    PeriphId::Uart2
}

// sentinel used by test infrastructure to assist with parsing
// The format of any test infrastructure output to recover is as follows:
// _|TT|_<ident>,<data separated by commas>,_|TE|_
// where _|TT|_ and _|TE|_ are bookends around the data to be reported
// <ident> is a single-word identifier that routes the data to a given parser
// <data> is free-form data, which will be split at comma boundaries by the parser
pub const BOOKEND_START: &str = "_|TT|_";
pub const BOOKEND_END: &str = "_|TE|_";
