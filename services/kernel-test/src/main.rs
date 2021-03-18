#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

struct LoopArg {
    msec: usize,
}
// fn sleep_10_loop(_ign: usize) {
//     let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
//     let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

//     let mut ms_count = 1;
//     loop {
//         log::info!("10ms Loop count {}", ms_count);
//         sleep_ms(ticktimer_conn, 10).unwrap();
//         ms_count += 1;
//     }
// }

// fn sleep_5_loop(_ign: usize) {
//     let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
//     let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

//     let mut ms_count = 1;
//     loop {
//         log::info!("5ms Loop count {}", ms_count);
//         sleep_ms(ticktimer_conn, 5).unwrap();
//         ms_count += 1;
//     }
// }

fn sleep_loop(_arg: usize) {
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();
    let tid = xous::current_tid().unwrap();
    let sleep_ms = match tid {
        2 => 2,
        3 => 5,
        4 => 10,
        _ => panic!("Unknown TID"),
    };
    log::info!("My thread number is {}, sleeping {} ms", tid, sleep_ms);

    let mut ms_count = 1;
    loop {
        log::info!("{}ms Loop count {}", sleep_ms, ms_count);
        ticktimer_server::sleep_ms(ticktimer_conn, sleep_ms).unwrap();
        ms_count += 1;
    }
}

#[xous::xous_main]
fn test_main() -> ! {
    log_server::init_wait().unwrap();
    // let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    // let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    xous::create_thread_simple(sleep_loop, 10).unwrap();
    xous::create_thread_simple(sleep_loop, 5).unwrap();
    xous::create_thread_simple(sleep_loop, 2).unwrap();

    // let mut ms_count = 1;
    loop {
        xous::wait_event();
        // log::info!("Loop {}", ms_count);
        // sleep_ms(ticktimer_conn, 1).unwrap();
        // ms_count += 1;
    }
}
