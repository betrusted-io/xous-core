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

    loop {
        // just sleep as this is the parent thread
        tt.sleep_ms(120_000).ok();
    }
}
