fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    const HEAP_LARGER_LIMIT: usize = 4096 * 1024;
    let new_limit = HEAP_LARGER_LIMIT;
    let result =
        xous::rsyscall(xous::SysCall::AdjustProcessLimit(xous::Limits::HeapMaximum as usize, 0, new_limit));

    if let Ok(xous::Result::Scalar2(1, current_limit)) = result {
        xous::rsyscall(xous::SysCall::AdjustProcessLimit(
            xous::Limits::HeapMaximum as usize,
            current_limit,
            new_limit,
        ))
        .unwrap();
        log::info!("Heap limit increased to: {}", new_limit);
    } else {
        panic!("Unsupported syscall!");
    }

    let mut test_vec = Vec::new();
    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name("swaptest2", None).expect("can't register server");
    let mut msg_opt = None;

    const ALLOC_SIZE: usize = 1024 * 256;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        if let Some(scalar) = msg_opt.as_ref().unwrap().body.scalar_message() {
            log::info!("kicking off swaptest2 {}", scalar.arg1);
            for i in 0..ALLOC_SIZE {
                test_vec.push(i + scalar.arg1);
            }
            let mut sum = 0;
            for i in (0..ALLOC_SIZE).rev() {
                sum += test_vec[i];
            }
            log::info!("swaptest2: {}", sum);
            test_vec.clear();
        }
    }
}
