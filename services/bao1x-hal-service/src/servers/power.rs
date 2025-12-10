use bao1x_api::PowerOp;

pub fn start_power_service(sid: xous::SID) {
    std::thread::spawn(move || {
        power_service(sid);
    });
}

fn power_service(sid: xous::SID) {
    let mut clk_mgr = bao1x_hal::clocks::ClockManager::new().unwrap();
    let measured = clk_mgr.measured_freqs();
    log::info!("computed frequencies:");
    log::info!("  vco: {}", clk_mgr.vco_freq);
    log::info!("  fclk: {}", clk_mgr.fclk);
    log::info!("  aclk: {}", clk_mgr.aclk);
    log::info!("  hclk: {}", clk_mgr.hclk);
    log::info!("  iclk: {}", clk_mgr.iclk);
    log::info!("  pclk: {}", clk_mgr.pclk);
    log::info!("  per: {}", clk_mgr.perclk);
    log::info!("measured frequencies:");
    for (name, freq) in measured {
        log::info!("  {}: {} MHz", name, freq);
    }

    let hal = bao1x_hal_service::Hal::new();

    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode: PowerOp =
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(PowerOp::Invalid);
        log::debug!("{:?}", opcode);
        match opcode {
            PowerOp::Wfi => {
                log::info!("triggering wfi...");
                clk_mgr.wfi();
                log::info!("recovered from wfi!");
                hal.set_preemption(true);
                log::info!("preemption turned on!");
            }
            PowerOp::Invalid => {
                log::error!("Unrecognized opcode: {:?}, ignoring!", opcode);
            }
        }
    }
}
