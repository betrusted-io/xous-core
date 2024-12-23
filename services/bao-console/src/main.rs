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

    loop {
        // just sleep as this is the parent thread
        tt.sleep_ms(120_000).ok();
    }
}
