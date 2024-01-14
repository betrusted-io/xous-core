#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

const EXTRA_KEY: usize = 42;

fn sleep_loop_4(main_conn: usize, sleep_ms: usize, ticktimer_conn: usize, pid: usize) {
    let tid = xous::current_tid().unwrap();
    let ticktimer_conn = ticktimer_conn as _;
    // let pid = xous::current_pid().unwrap().get();
    log::info!(
        "My thread number is {}, sleeping 0x{:08x} ({}) ms and main_conn: {}",
        tid,
        sleep_ms,
        sleep_ms,
        main_conn
    );
    let main_conn = main_conn as xous::CID;

    let mut loop_count = 0;
    loop {
        // log::info!(
        //     "TEST THREAD {}: {}ms Number of times slept: {}",
        //     tid,
        //     sleep_ms,
        //     loop_count
        // );
        let start_time = ticktimer.elapsed_ms();
        ticktimer.sleep_ms(ticktimer_conn, sleep_ms).unwrap();
        let end_time = ticktimer.elapsed_ms();
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
        xous::send_message(
            main_conn,
            xous::Message::Scalar(xous::ScalarMessage::from_usize(
                pid as _,
                tid as _,
                sleep_ms as _,
                loop_count as _,
                end_time as _,
            )),
        )
        .unwrap();
    }
}

fn sleep_loop_3(main_conn: usize, sleep_ms: usize, ticktimer_conn: usize) {
    sleep_loop_4(main_conn, sleep_ms, ticktimer_conn, xous::current_pid().unwrap().get() as _);
}

fn sleep_loop_2(main_conn: usize, sleep_ms: usize) {
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
    sleep_loop_3(main_conn, sleep_ms, ticktimer_conn);
}

fn sleep_loop_1(main_conn: usize) {
    let sleep_ms = (xous::current_pid().unwrap().get() as usize) * (xous::current_tid().unwrap() as usize);
    sleep_loop_2(main_conn, sleep_ms);
}

static mut MAIN_CONN: xous::CID = 0;
fn sleep_loop_0() { sleep_loop_1(unsafe { MAIN_CONN } as _); }

fn main() -> ! {
    log_server::init_wait().unwrap();

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    let main_server = xous::create_server().unwrap();
    let server_conn = xous::connect(main_server).unwrap();
    unsafe { MAIN_CONN = server_conn };

    let pid = xous::current_pid().unwrap().get() as usize;

    xous::create_thread_0(sleep_loop_0).unwrap();
    xous::create_thread_1(sleep_loop_1, 4 * pid).unwrap();
    xous::create_thread_2(sleep_loop_2, 10 * pid, ticktimer_conn as _).unwrap();
    xous::create_thread_3(sleep_loop_3, 42 * pid, ticktimer_conn as _, pid).unwrap();
    xous::create_thread_4(sleep_loop_4, 180 * pid, ticktimer_conn as _, pid, EXTRA_KEY).unwrap();

    loop {
        xous::receive_message(main_server).unwrap();
        log::info!("Received message from remote");
    }
}
