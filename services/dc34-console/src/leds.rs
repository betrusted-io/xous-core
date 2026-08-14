use dc34_api::*;

pub fn start_leds() {
    std::thread::spawn(move || {
        leds();
    });
}

fn leds() {
    let xns = xous_names::XousNames::new().unwrap();

    let sid = xns.register_name(dc34_api::LED_SERVER, None).unwrap();

    #[cfg(not(feature = "uber"))]
    const LED_COUNT: u8 = 10;
    #[cfg(feature = "uber")]
    const LED_COUNT: u8 = 18;

    let mut lightgenes =
        crate::bio::lightgenes::Lightgenes::new(arbitrary_int::u5::new(15), LED_COUNT, None).unwrap();

    let mut rate_param: u8 = 1;
    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let opcode = {
            let msg = msg_opt.as_mut().unwrap();
            num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(LedManagerOp::Invalid)
        };
        match opcode {
            LedManagerOp::Autogamy => {
                lightgenes.autogamy();
                lightgenes.express();
            }
            LedManagerOp::SetTestRate => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    rate_param = scalar.arg1.min(255) as u8;
                }
            }
            LedManagerOp::Syngamy => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    if let Some(sperm) = Haploid::deserialize_u32(&[
                        scalar.arg1 as u32,
                        scalar.arg2 as u32,
                        scalar.arg3 as u32,
                        scalar.arg4 as u32,
                    ]) {
                        // rewritten to match exactly what's happening in the production loop
                        let mut egg = lightgenes.meiosis().unwrap();
                        let rate = MutationRate::from_param(rate_param);
                        log::info!("mutate at rate {:?}: {:?}", rate, egg);
                        mutate(&mut egg, rate);
                        log::info!("Mutated {:?}", egg);
                        lightgenes.gene = Some(Diploid([egg, sperm]));
                        lightgenes.express();
                    } else {
                        log::warn!("Couldn't deserialize gene in call to Syngamy, ignoring")
                    }
                }
            }
            LedManagerOp::Force => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    if let Some(phenotype) = Haploid::deserialize_u32(&[
                        scalar.arg1 as u32,
                        scalar.arg2 as u32,
                        scalar.arg3 as u32,
                        scalar.arg4 as u32,
                    ]) {
                        lightgenes.force(phenotype);
                    } else {
                        log::warn!("Couldn't deserialize gene in call to Force, ignoring")
                    }
                }
            }
            LedManagerOp::GeneTest => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    let badge_type = BadgeType::try_from(scalar.arg1 as u8).unwrap_or(BadgeType::None);
                    lightgenes
                        .gene
                        .replace(Diploid([Haploid::from_type(&badge_type), Haploid::from_type(&badge_type)]));
                    log::info!("Init to {:?}: gene {:?}", badge_type, lightgenes.gene);
                    lightgenes.express();
                }
            }
            LedManagerOp::SetGene => {
                if let Some(mem) = msg_opt.as_ref().unwrap().body.memory_message() {
                    lightgenes.gene = Some(Diploid::receive(mem));
                    log::info!("Received gene {:?}", lightgenes.gene);
                    lightgenes.express();
                }
            }
            LedManagerOp::JackEyes => {
                if let Some(scalar) = msg_opt.as_ref().unwrap().body.scalar_message() {
                    lightgenes.jack_eyes(scalar.arg1 != 0);
                }
            }
            LedManagerOp::Pause => {
                if let Some(scalar) = msg_opt.as_mut().unwrap().body.scalar_message_mut() {
                    lightgenes.pause_rendering(scalar.arg1 != 0);
                }
            }
            LedManagerOp::Invalid => {
                log::error!("Invalid LED manager operation: {:?}", opcode);
            }
        };
    }
}
