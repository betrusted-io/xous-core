#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

const EXTRA_KEY: usize = 42;

fn sleep_loop_4(sleep_ms: usize, conn: usize, pid: usize, extra: usize) {
    let tid = xous::current_tid().unwrap();
    let conn = conn as _;
    // let pid = xous::current_pid().unwrap().get();
    log::info!("My thread number is {}, sleeping 0x{:08x} ({}) ms and EXTRA: {}", tid, sleep_ms, sleep_ms, extra);
    assert_eq!(extra, EXTRA_KEY, "extra key in arg 4 didn't match -- was {}, not {}", extra, EXTRA_KEY);

    let mut loop_count = 0;
    loop {
        // log::info!(
        //     "TEST THREAD {}: {}ms Number of times slept: {}",
        //     tid,
        //     sleep_ms,
        //     loop_count
        // );
        let start_time = ticktimer_server::elapsed_ms(conn).unwrap();
        ticktimer_server::sleep_ms(conn, sleep_ms).unwrap();
        let end_time = ticktimer_server::elapsed_ms(conn).unwrap();
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

fn sleep_loop_3(sleep_ms: usize, conn: usize, pid: usize) {
    sleep_loop_4(sleep_ms, conn, pid, EXTRA_KEY);
}

fn sleep_loop_2(sleep_ms: usize, conn: usize) {
    sleep_loop_3(sleep_ms, conn, xous::current_pid().unwrap().get() as _);
}

fn sleep_loop_1(sleep_ms: usize) {
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();
    sleep_loop_2(sleep_ms, ticktimer_conn as _);
}

fn sleep_loop_0() {
    let sleep_ms = (xous::current_pid().unwrap().get() as usize) * (xous::current_tid().unwrap() as usize);
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();
    sleep_loop_2(sleep_ms, ticktimer_conn as _);
}

#[xous::xous_main]
fn test_main() -> ! {
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    log_server::init_wait().unwrap();
    let pid = xous::current_pid().unwrap().get() as usize;

    xous::create_thread_0(sleep_loop_0).unwrap();
    xous::create_thread_1(sleep_loop_1, 4 * pid).unwrap();
    xous::create_thread_2(sleep_loop_2, 10 * pid, ticktimer_conn as _).unwrap();
    xous::create_thread_3(sleep_loop_3, 42 * pid, ticktimer_conn as _, pid).unwrap();
    xous::create_thread_4(sleep_loop_4, 180 * pid, ticktimer_conn as _, pid, EXTRA_KEY).unwrap();

    loop {
        xous::wait_event();
    }
}
