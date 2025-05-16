mod cmds;
mod repl;
mod shell;

use cmds::*;

fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let tt = ticktimer::Ticktimer::new().unwrap();
    shell::start_shell();

    log::info!("start spi flash map test");
    let spimap = xous::map_memory(
        None,
        xous::MemoryAddress::new(xous::arch::MMAP_VIRT_BASE),
        cramium_hal::board::SPINOR_LEN as usize,
        xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::VIRT,
    )
    .expect("couldn't map spi range");
    log::info!("spimap: {:x?}", spimap);
    // we have the mapping, now try to dereference it and read something to test it
    let spislice: &[u32] = unsafe { spimap.as_slice() };
    // this should trigger the page fault and thus the handler
    log::info!("spislice: {:x?}", &spislice[..32]);

    // marks modified pages as dirty.
    xous_swapper::mark_dirty(&spislice[..32]);

    // Calls sync to explicitly flush the dirty pages now
    xous_swapper::sync(Some(&spislice[..32]));
    // xous_swapper::sync::<u8>(None);

    log::info!("end spi flash map test");

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
    #[cfg(not(feature = "hosted-baosec"))]
    {
        use cramium_api::I2cApi;
        let mut i2c = cram_hal_service::I2c::new();
        use cramium_hal::axp2101::*;
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
    #[cfg(feature = "hosted-baosec")]
    loop {
        tt.sleep_ms(2_000).ok();
    }
}
