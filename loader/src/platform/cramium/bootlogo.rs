use core::mem::size_of;

use cramium_hal::ifram::IframRange;
use cramium_hal::iox::{IoSetup, Iox};
use cramium_hal::udma::{GlobalConfig, PeriphId};
use cramium_hal::{iox, udma};

pub const FB_WIDTH_WORDS: usize = 11;
pub const FB_LINES: usize = 536;

pub fn show_logo(pclk_freq: u32, udma_global: &mut GlobalConfig, iox: &mut Iox) {
    let channel = {
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
        udma_global.clock_on(PeriphId::Spim0);
        udma::SpimChannel::Channel0
    };

    // safety: this is safe because we remembered to set up the clock config; and,
    // this binding should live for the lifetime of Xous so we don't have to worry about unmapping.
    let mut spim = unsafe {
        cramium_hal::udma::Spim::new_with_ifram(
            channel,
            2_000_000,
            pclk_freq,
            udma::SpimClkPol::LeadingEdgeRise,
            udma::SpimClkPha::CaptureOnLeading,
            udma::SpimCs::Cs0,
            3,
            2,
            None,
            // one extra line for handling the addressing setup
            (FB_LINES + 1) * FB_WIDTH_WORDS * size_of::<u32>(),
            0,
            None,
            None,
            IframRange::from_raw_parts(utralib::HW_IFRAM0_MEM, utralib::HW_IFRAM0_MEM, 24576),
        )
    };

    let mut next_free_line = 0;
    let hwfb = spim.tx_buf_mut();
    // safety: this is safe because `u32` has no invalid values
    // set the mode and address
    // the very first line is unused, except for the mode & address info
    // this is done just to keep the math easy for computing strides & alignments
    for src_line in 0..FB_LINES {
        hwfb[(next_free_line + 1) * FB_WIDTH_WORDS - 1] = (hwfb[(next_free_line + 1) * FB_WIDTH_WORDS - 1]
            & 0x0000_FFFF)
            | (((src_line as u32) << 6) | 0b001) << 16;
        // now copy the data
        hwfb[(next_free_line + 1) * FB_WIDTH_WORDS..(next_free_line + 2) * FB_WIDTH_WORDS].copy_from_slice(
            &crate::platform::cramium::poweron_bt::LOGO_MAP
                [src_line * FB_WIDTH_WORDS..(src_line + 1) * FB_WIDTH_WORDS],
        );

        if next_free_line < FB_LINES as usize {
            next_free_line += 1;
        }
    }

    // safety: this function is safe to call because:
    //   - `is_virtual` is `false` => data should be a physical buffer that is pre-populated with the transmit
    //     data this is done by `copy_line_to_dma()`
    //   - the `data` argument is a physical buffer slice, which is only used as a base/bounds argument
    unsafe {
        spim.tx_data_async_from_parts::<u16>(
            FB_WIDTH_WORDS * 2 - 1,
            // +1 for the trailing dummy bits
            next_free_line * FB_WIDTH_WORDS * 2 + 1,
            true,
            false,
        );
    }
}
