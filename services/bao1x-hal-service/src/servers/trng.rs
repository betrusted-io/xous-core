#[cfg(feature = "board-baosec")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "board-dabao")]
use bao1x_hal_service::trng::Trng;
use bao1x_hal_service::trng::api;
use flatipc::Ipc;
use num_traits::*;
#[cfg(feature = "board-baosec")]
use rand::RngCore;
use xous::CID;
#[cfg(feature = "board-baosec")]
use xous_bio_bdma::BioSharedState;
use xous_ipc::Buffer;

#[cfg(feature = "board-baosec")]
use crate::servers::baosec_hw::HwTrng;
#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
struct ScalarCallback {
    server_to_cb_cid: CID,
    cb_to_client_cid: CID,
    cb_to_client_id: u32,
}

#[cfg(feature = "board-baosec")]
pub fn start_trng_service(bio_ss: &Arc<Mutex<BioSharedState>>) {
    std::thread::spawn({
        let bio_ss = bio_ss.clone();
        move || {
            trng_service(bio_ss);
        }
    });
}
#[cfg(feature = "board-baosec")]
fn trng_service(bio_ss: Arc<Mutex<BioSharedState>>) -> ! {
    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed, anyone including less-trusted processes can get a random number
    let trng_sid = xns.register_name(api::SERVER_NAME_TRNG, None).expect("can't register server");

    let mut trng = Box::new(HwTrng::new(bio_ss));

    let mut error_cb_conns = Vec::<ScalarCallback>::new();

    loop {
        let mut msg = xous::receive_message(trng_sid).unwrap();
        let op: Option<api::Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", op);
        match op {
            Some(api::Opcode::GetTrng) => xous::msg_blocking_scalar_unpack!(msg, count, _, _, _, {
                if count == 1 {
                    xous::return_scalar2(msg.sender, trng.next_u32() as usize, 0xdead_beef)
                        .expect("couldn't return GetTrng request");
                } else {
                    xous::return_scalar2(msg.sender, trng.next_u32() as usize, trng.next_u32() as usize)
                        .expect("couldn't return GetTrng request");
                }
            }),
            Some(api::Opcode::ErrorSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<api::ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut error_cb_conns);
            }
            Some(api::Opcode::ErrorNotification) => {
                log::error!("Got a notification interrupt from the TRNG. Syndrome: {:?}", trng.get_errors());
                log::error!("Stats: {:?}", trng.get_tests());
                send_event(&error_cb_conns);
            }
            Some(api::Opcode::HealthStats) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                buffer.replace(trng.get_tests()).unwrap();
            }
            Some(api::Opcode::ErrorStats) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                buffer.replace(trng.get_errors()).unwrap();
            }
            Some(api::Opcode::FillTrng) => {
                let mm = msg.body.memory_message_mut().unwrap();
                let buffer =
                    api::IpcTrngBuf::from_memory_message_mut(mm).expect("couldn't unpack FillTrng buffer");
                let len = buffer.len as usize;
                trng.fill_buf(&mut buffer.data[..len]).expect("couldn't fill TRNG buffer");
            }
            None => {
                log::error!("couldn't convert opcode, ignoring");
            }
        }
    }
}

#[cfg(feature = "board-dabao")]
pub fn start_trng_service() {
    std::thread::spawn({
        move || {
            trng_service();
        }
    });
}
#[cfg(feature = "board-dabao")]
fn trng_service() -> ! {
    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed, anyone including less-trusted processes can get a random number
    let trng_sid = xns.register_name(api::SERVER_NAME_TRNG, None).expect("can't register server");

    let trng = Trng::new(&xns).unwrap();

    let mut error_cb_conns = Vec::<ScalarCallback>::new();

    loop {
        let mut msg = xous::receive_message(trng_sid).unwrap();
        let op: Option<api::Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", op);
        match op {
            Some(api::Opcode::GetTrng) => xous::msg_blocking_scalar_unpack!(msg, count, _, _, _, {
                if count == 1 {
                    xous::return_scalar2(msg.sender, trng.get_u32().unwrap() as usize, 0xdead_beef)
                        .expect("couldn't return GetTrng request");
                } else {
                    xous::return_scalar2(
                        msg.sender,
                        trng.get_u32().unwrap() as usize,
                        trng.get_u32().unwrap() as usize,
                    )
                    .expect("couldn't return GetTrng request");
                }
            }),
            Some(api::Opcode::ErrorSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<api::ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut error_cb_conns);
            }
            Some(api::Opcode::ErrorNotification) => {
                unimplemented!()
            }
            Some(api::Opcode::HealthStats) => {
                unimplemented!()
            }
            Some(api::Opcode::ErrorStats) => {
                unimplemented!()
            }
            Some(api::Opcode::FillTrng) => {
                let mm = msg.body.memory_message_mut().unwrap();
                let buffer =
                    api::IpcTrngBuf::from_memory_message_mut(mm).expect("couldn't unpack FillTrng buffer");
                let len = buffer.len as usize;
                trng.fill_buf(&mut buffer.data[..len]).expect("couldn't fill TRNG buffer");
            }
            None => {
                log::error!("couldn't convert opcode, ignoring");
            }
        }
    }
}

fn do_hook(hookdata: api::ScalarHook, cb_conns: &mut Vec<ScalarCallback>) {
    let (s0, s1, s2, s3) = hookdata.sid;
    let sid = xous::SID::from_u32(s0, s1, s2, s3);
    let server_to_cb_cid = xous::connect(sid).unwrap();
    let cb_dat =
        ScalarCallback { server_to_cb_cid, cb_to_client_cid: hookdata.cid, cb_to_client_id: hookdata.id };
    cb_conns.push(cb_dat);
}
#[allow(dead_code)]
/// called if the server exits, which we don't. Keep around for reference in case for some reason we need to
/// do that...
fn unhook(cb_conns: &mut Vec<ScalarCallback>) {
    for scb in cb_conns.drain(..) {
        xous::send_message(
            scb.server_to_cb_cid,
            xous::Message::new_blocking_scalar(api::EventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .unwrap();
        unsafe {
            xous::disconnect(scb.server_to_cb_cid).unwrap();
        }
    }
}
#[allow(dead_code)]
fn send_event(cb_conns: &Vec<ScalarCallback>) {
    for scb in cb_conns.iter() {
        // note that the "which" argument is only used for GPIO events, to indicate which pin had the
        // event
        xous::send_message(
            scb.server_to_cb_cid,
            xous::Message::new_scalar(
                api::EventCallback::Event.to_usize().unwrap(),
                scb.cb_to_client_cid as usize,
                scb.cb_to_client_id as usize,
                0,
                0,
            ),
        )
        .unwrap();
    }
}
