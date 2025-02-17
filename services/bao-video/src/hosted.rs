pub fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name("bao video subsystem", None).expect("can't register server");

    let tt = ticktimer::Ticktimer::new().unwrap();

    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode = num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(Opcode::InvalidCall);
        log::debug!("{:?}", opcode);
        match opcode {
            Opcode::CamIrq => {
                unimplemented!()
            }
            Opcode::InvalidCall => {
                log::error!("Invalid call to bao video server: {:?}", msg);
            }
        }
    }
}
