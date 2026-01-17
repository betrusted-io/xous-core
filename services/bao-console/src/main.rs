mod cmds;
#[cfg(feature = "ctap-bringup")]
mod ctap;
mod repl;
mod shell;
use cmds::*;

fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());
    #[cfg(feature = "duart-debug-hal")]
    bao1x_hal::claim_duart();

    let tt = ticktimer::Ticktimer::new().unwrap();
    shell::start_shell();

    tt.sleep_ms(500).ok(); // pause for the system to startup
    let usb = usb_bao1x::UsbHid::new();
    usb.serial_console_input_injection();

    tt.sleep_ms(4000).ok();

    #[cfg(all(feature = "board-baosec", feature = "test-flash"))]
    {
        use xous_swapper::FlashPage;
        log::info!("start spi flash map test");
        let swapper = xous_swapper::Swapper::new().unwrap();
        log::info!("got handle on swapper object");
        let mut spimap = xous::map_memory(
            None,
            xous::MemoryAddress::new(xous::arch::MMAP_VIRT_BASE),
            bao1x_hal::board::SPINOR_LEN as usize,
            xous::MemoryFlags::R | xous::MemoryFlags::VIRT,
        )
        .expect("couldn't map spi range");
        log::info!("spimap: {:x?}", spimap);
        // we have the mapping, now try to dereference it and read something to test it
        let spislice: &mut [u8] = unsafe { spimap.as_slice_mut() };
        // this should trigger the page fault and thus the handler
        log::info!("spislice read 0..64: {:x?}", &spislice[..64]);
        log::info!("spislice read 256..264: {:x?}", &spislice[256..264]);
        let test_start = 8192 * 1024 + 4096;
        log::info!("spislice read 8M: {:x?}", &spislice[test_start..test_start + 16]);

        let mut fp = FlashPage::new();
        fp.data.copy_from_slice(&spislice[test_start..test_start + 4096]);
        for (i, d) in fp.data[..32].iter_mut().enumerate() {
            *d = 128u8 - i as u8;
        }
        swapper
            .write_page(spislice[test_start..test_start + 32].as_ptr() as usize, &fp)
            .expect("couldn't write page");
        log::info!("spislice read 8M (again): {:x?}", &spislice[test_start..test_start + 16]);

        log::info!("end spi flash map test");
    }

    #[cfg(feature = "test-scrollbars")]
    {
        let mut sl_binding = ux_api::widgets::ScrollableList::default();
        let mut sl = &mut sl_binding;

        log::info!("setup list");
        for col in 0..2 {
            for row in 0..12 {
                sl = sl.add_item(col, &format!("c{}r{}", row, col));
            }
        }
        sl.select_index = (1, 3);
        log::info!("initial draw");
        sl.draw(0);
        tt.sleep_ms(500).ok();
        use ux_api::widgets::Direction;

        log::info!("scroll down test");
        for _ in 0..14 {
            sl.move_scroll_offset(Direction::Down);
            sl.draw(0);
            tt.sleep_ms(500).ok();
        }
        log::info!("scroll right then up");
        sl.move_scroll_offset(Direction::Right);
        for _ in 0..10 {
            sl.move_scroll_offset(Direction::Up);
            sl.draw(0);
            tt.sleep_ms(500).ok();
        }

        sl.set_scroll_offset(0, 0);
        sl.set_selected(0, 0);
        for _ in 0..14 {
            sl.move_selection(Direction::Down);
            sl.draw(0);
            tt.sleep_ms(500).ok();
        }
        for _ in 0..8 {
            sl.move_selection(Direction::Up);
            sl.draw(0);
            tt.sleep_ms(500).ok();
        }
        for _ in 0..4 {
            sl.move_selection(Direction::Right);
            sl.draw(0);
            tt.sleep_ms(500).ok();
        }
        sl.move_selection(Direction::Left);
        sl.draw(0);
        tt.sleep_ms(500).ok();
    }

    #[cfg(feature = "modal-testing")]
    {
        log::set_max_level(log::LevelFilter::Debug);
        modals::tests::spawn_test();
    }

    #[cfg(feature = "battery-readout")]
    {
        use bao1x_api::I2cApi;
        let mut i2c = bao1x_hal_service::I2c::new();
        use bao1x_hal::axp2101::*;
        let measurements = [("VBAT", REG_VBAT_H), ("VBUS", REG_VBUS_H), ("VSYS", REG_VSYS_H)];
        let mut buf = [0u8, 0u8];
        loop {
            tt.sleep_ms(2_000).ok();
            for (name, offset) in measurements {
                i2c.i2c_read(AXP2101_DEV, offset, &mut buf, false).unwrap();
                let v: u32 = (((buf[0] as u32) & 0x3F) << 8) | buf[1] as u32;
                log::info!("{}: {:0.3}V", name, v as f32 / 1000.0);
                i2c.i2c_read(AXP2101_DEV, REG_SOC, &mut buf[0..1], false).unwrap();
                log::info!("SOC: {}%", buf[0]);
            }
        }
    }

    #[cfg(feature = "ctap-bringup")]
    crate::ctap::ctap_test();

    #[cfg(any(feature = "hosted-baosec", not(feature = "battery-readout")))]
    loop {
        tt.sleep_ms(2_000).ok();
    }
}
