use bao1x_hal::acram::OneWayCounter;
use utralib::*;
use ux_api::minigfx::FrameBuffer;

// build with `cargo xtask bao1x-baremetal-baosec --loader-feature factory-utils`
pub fn pddb_erase() {
    crate::println!("entering pddb_erase");
    let one_way = OneWayCounter::new();
    while one_way.get_decoded::<bao1x_api::BootWaitCoding>().expect("couldn't fetch flag")
        != bao1x_api::BootWaitCoding::Enable
    {
        one_way.inc_coded::<bao1x_api::BootWaitCoding>().unwrap();
    }

    // setup all the board pins to a known state
    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    bao1x_hal::board::setup_display_pins(&iox);
    bao1x_hal::board::setup_memory_pins(&iox);
    bao1x_hal::board::setup_oled_power_pin(&iox);

    // this routine is used to initialize baosec products - sets the board type and
    // erases the off-chip FLASH
    use bao1x_api::baosec::PDDB_ORIGIN;
    use bao1x_hal::{
        board::SPINOR_BULK_ERASE_SIZE,
        ifram::IframRange,
        iox::Iox,
        udma::{Spim, *},
    };
    let perclk = 100_000_000;
    let udma_global = GlobalConfig::new();

    // setup the I/O pins
    let channel = bao1x_hal::board::setup_memory_pins(&iox);
    udma_global.clock_on(bao1x_api::PeriphId::from(channel));
    // safety: this is safe because clocks have been set up
    let mut flash_spim = unsafe {
        Spim::new_with_ifram(
            channel,
            // has to be half the clock frequency reaching the block, but
            // run it as fast
            // as we can run perclk
            perclk / 4,
            perclk / 2,
            SpimClkPol::LeadingEdgeRise,
            SpimClkPha::CaptureOnLeading,
            SpimCs::Cs0,
            0,
            0,
            None,
            256 + 16, /* just enough space to send commands + programming
                       * page */
            4096,
            Some(6),
            Some(SpimMode::Standard), // guess Standard
            IframRange::from_raw_parts(
                bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                bao1x_hal::board::SPIM_FLASH_IFRAM_ADDR,
                4096 * 2,
            ),
        )
    };
    flash_spim.identify_flash_reset_qpi();
    let flash_id = flash_spim.mem_read_id_flash();
    crate::println!("flash ID (init): {:x}", flash_id);
    if !bao1x_api::SPI_FLASH_IDS.contains(&(flash_id & 0xFF_FF_FF)) {
        return;
    }

    let pddb_erase_len = SPINOR_BULK_ERASE_SIZE as usize * 2;
    for addr in (PDDB_ORIGIN..PDDB_ORIGIN + pddb_erase_len).step_by(SPINOR_BULK_ERASE_SIZE as usize) {
        crate::println!("  {:x}...", addr);
        flash_spim.flash_erase_block(addr, SPINOR_BULK_ERASE_SIZE as usize);
    }
    crate::println!("...done!");

    let mut iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    // redraw or show the boot logo - depending on how boot1 went, we may or may not have it already
    let mut udma_global = GlobalConfig::new();
    let mut sh1107 = bao1x_hal::sh1107::Oled128x128::new(
        bao1x_hal::sh1107::MainThreadToken::new(),
        perclk,
        &mut iox,
        &mut udma_global,
    );
    sh1107.init().ok();
    sh1107.buffer_mut().fill(0xFFFF_FFFF);
    sh1107.blit_screen(&crate::platform::bao1x::bitmaps::factory_mode::BITMAP);
    sh1107.draw().ok();
    crate::println!("leaving pddb_erase");
}
