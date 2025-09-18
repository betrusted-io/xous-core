// Constants that define pin locations, RAM offsets, etc. for the BaoSec board
use crate::iox;
use crate::iox::IoSetup;

// console uart buffer
pub const UART_DMA_TX_BUF_PHYS: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4096;

// RAM needs two buffers of 1k + 16 bytes = 2048 + 16 = 2064 bytes; round up to one page
pub const SPIM_RAM_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 2 * 4096;

// app uart buffer
pub const APP_UART_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 3 * 4096;

// Flash needs 4096 bytes for Rx, and 0 bytes for Tx + 16 bytes for cmd for 2 pages total. This is released
// after boot.
pub const SPIM_FLASH_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 5 * 4096;

// USB pages - USB subsystem is a hog, needs a lot of pages
pub const CRG_IFRAM_PAGES: usize = 22;
pub const CRG_UDC_MEMBASE: usize =
    utralib::HW_IFRAM1_MEM + utralib::HW_IFRAM1_MEM_LEN - CRG_IFRAM_PAGES * 0x1000;

// MANUALLY SYNCED TO ALLOCATIONS ABOVE
// inclusive numbering - we allocate pages from the top-down, so the last number should generally be 31
pub const IFRAM0_RESERVED_PAGE_RANGE: [usize; 2] = [31 - 5, 31];
pub const IFRAM1_RESERVED_PAGE_RANGE: [usize; 2] = [31 - CRG_IFRAM_PAGES, 31];

/// Setup pins for the baosor display (Precursor memory LCD target)
pub fn setup_display_pins(iox: &dyn IoSetup) -> crate::udma::SpimChannel {
    const SPI_CS_PIN: u8 = 5;
    const SPI_CLK_PIN: u8 = 4;
    const SPI_DAT_PIN: u8 = 0;
    const SPI_PORT: iox::IoxPort = iox::IoxPort::PD;

    // SPIM_CLK_A[0]
    iox.setup_pin(
        SPI_PORT,
        SPI_CLK_PIN,
        Some(iox::IoxDir::Output),
        Some(iox::IoxFunction::AF1),
        None,
        None,
        Some(iox::IoxEnable::Enable),
        Some(iox::IoxDriveStrength::Drive2mA),
    );
    // SPIM_SD0_A[0]
    iox.setup_pin(
        SPI_PORT,
        SPI_DAT_PIN,
        Some(iox::IoxDir::Output),
        Some(iox::IoxFunction::AF1),
        None,
        None,
        Some(iox::IoxEnable::Enable),
        Some(iox::IoxDriveStrength::Drive2mA),
    );
    // SPIM_CSN0_A[0]
    // chip select toggle by UDMA has ~6 cycles setup and 1 cycles hold time, which
    // meets the requirements for the display.
    iox.setup_pin(
        SPI_PORT,
        SPI_CS_PIN,
        Some(iox::IoxDir::Output),
        Some(iox::IoxFunction::AF1),
        None,
        Some(iox::IoxEnable::Enable),
        Some(iox::IoxEnable::Enable),
        Some(iox::IoxDriveStrength::Drive2mA),
    );
    // using bank SPIM_B[1]
    crate::udma::SpimChannel::Channel0
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
    // SPIM_CSN0_A[1]
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
