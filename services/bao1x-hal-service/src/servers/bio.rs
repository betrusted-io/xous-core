use bao1x_api::bio::*;
use bao1x_hal::bio_hw;

pub fn start_bio_service(clk_freq: u32) {
    std::thread::spawn(move || {
        bio_service(clk_freq);
    });
}

fn bio_service(clk_freq: u32) {
    let xns = xous_names::XousNames::new().unwrap();
    // claim the server name
    let sid = xns.register_name(BIO_SERVER_NAME, None).unwrap();

    let mut bio_ss = bio_hw::BioSharedState::new(clk_freq);
    let mut handles_used: [bool; 4] = [false; 4];
    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let opcode = {
            let msg = msg_opt.as_mut().unwrap();
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(BioOp::InvalidCall)
        };
        log::debug!("{:?}", opcode);
        match opcode {
            BioOp::InitCore => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg_opt.as_mut().unwrap().body.memory_message_mut().unwrap(),
                    )
                };
                let mut config = buf.to_original::<CoreInitRkyv, _>().unwrap();
                match bio_ss.init_core(config.core, &config.code, config.offset, config.config) {
                    Ok(freq) => {
                        config.actual_freq = freq;
                        config.result = BioError::None;
                    }
                    Err(e) => {
                        config.result = e;
                        config.actual_freq = None;
                    }
                }
                buf.replace(config).unwrap();
            }
            BioOp::DeInitCore => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let core = scalar.arg1;
                    bio_ss.de_init_core(core.into()).unwrap();
                }
            }
            BioOp::GetCoreHandle => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let index = scalar.arg1;
                    if !handles_used[index] {
                        handles_used[index] = true;
                        scalar.arg1 = 1; // set valid bit
                    } else {
                        scalar.arg1 = 0; // set invalid - handle already in use
                    }
                }
            }
            BioOp::ReleaseCoreHandle => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    // caller should have *already* de-allocated the handle on their side to avoid a race
                    // condition
                    let core = scalar.arg1;
                    handles_used[core] = false;
                    // that's it - all the bookkeeping is now done.
                }
            }
            BioOp::UpdateBioFreq => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let new_freq = scalar.arg1;
                    // returns the old freq
                    scalar.arg1 = bio_ss.update_bio_freq(new_freq as u32) as usize;
                }
            }
            BioOp::GetBioFreq => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    scalar.arg1 = bio_ss.get_bio_freq() as usize;
                }
            }
            BioOp::GetCoreFreq => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let bio_core: BioCore = scalar.arg1.into();
                    let result = bio_ss.get_core_freq(bio_core);
                    if let Some(freq) = result {
                        scalar.arg1 = freq as usize;
                        scalar.arg2 = 1;
                    } else {
                        scalar.arg2 = 0;
                    }
                }
            }
            BioOp::GetVersion => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    scalar.arg1 = bio_ss.get_version() as usize;
                }
            }
            BioOp::CoreState => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let which =
                        [scalar.arg1.into(), scalar.arg2.into(), scalar.arg3.into(), scalar.arg4.into()];
                    log::debug!("setting: {:?}", which);
                    bio_ss.set_core_state(which).unwrap();
                    log::debug!("core state: {:x}", bio_ss.bio.r(utralib::utra::bio_bdma::SFR_CTRL));
                }
            }
            BioOp::DmaWindows => {
                let buf = unsafe {
                    xous_ipc::Buffer::from_memory_message(
                        msg_opt.as_mut().unwrap().body.memory_message().unwrap(),
                    )
                };
                let windows = buf.to_original::<DmaFilterWindows, _>().unwrap();
                bio_ss.setup_dma_windows(windows).unwrap();
            }
            BioOp::FifoEventTriggers => {
                let buf = unsafe {
                    xous_ipc::Buffer::from_memory_message(
                        msg_opt.as_mut().unwrap().body.memory_message().unwrap(),
                    )
                };
                let config = buf.to_original::<FifoEventConfig, _>().unwrap();
                bio_ss.setup_fifo_event_triggers(config).unwrap();
            }
            BioOp::IoConfig => {
                let buf = unsafe {
                    xous_ipc::Buffer::from_memory_message(
                        msg_opt.as_mut().unwrap().body.memory_message().unwrap(),
                    )
                };
                let config = buf.to_original::<IoConfig, _>().unwrap();
                bio_ss.setup_io_config(config).unwrap();
            }
            BioOp::IrqConfig => {
                let buf = unsafe {
                    xous_ipc::Buffer::from_memory_message(
                        msg_opt.as_mut().unwrap().body.memory_message().unwrap(),
                    )
                };
                let config = buf.to_original::<IrqConfig, _>().unwrap();
                bio_ss.setup_irq_config(config).unwrap();
            }
            BioOp::InvalidCall => panic!("Invalid BioOp"),
        }
    }
}
