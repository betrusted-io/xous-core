#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

fn sleep_loop(sleep_ms: usize) {
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();
    let tid = xous::current_tid().unwrap();
    let pid = xous::current_pid().unwrap().get();
    let sleep_ms = match (pid, tid) {
        (5, 2) => 10,
        (5, 3) => 20,
        (5, 4) => 50,
        (6, 2) => 3000,
        (6, 3) => 7000,
        (6, 4) => 12000,
        _ => panic!("Unknown TID/PID"),
    };
    // log::info!("My thread number is {}, sleeping 0x{:08x} ({}) ms", tid, sleep_ms, sleep_ms);

    let mut loop_count = 0;
    loop {
        // log::info!(
        //     "TEST THREAD {}: {}ms Number of times slept: {}",
        //     tid,
        //     sleep_ms,
        //     loop_count
        // );
        let start_time = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
        ticktimer_server::sleep_ms(ticktimer_conn, sleep_ms).unwrap();
        let end_time = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
        log::info!(
            "TEST THREAD {}:{}: target {}ms, {} loops: Sleep finished (uptime: {}, took {} ms)",
            pid,
            tid,
            sleep_ms,
            loop_count,
            end_time,
            end_time - start_time,
        );
        loop_count += 1;
    }
}

#[xous::xous_main]
fn test_main() -> ! {
    log_server::init_wait().unwrap();
    xous::create_thread_simple(sleep_loop, 10).unwrap();
    xous::create_thread_simple(sleep_loop, 5).unwrap();
    if xous::current_pid().unwrap().get() == 6 {
        xous::create_thread_simple(sleep_loop, 2).unwrap();
    }

    // let mut ms_count = 1;
    loop {
        xous::wait_event();
        // log::info!("Loop {}", ms_count);
        // sleep_ms(ticktimer_conn, 1).unwrap();
        // ms_count += 1;
    }
}
