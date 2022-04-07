#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use api::Opcode;
use num_traits::{FromPrimitive, ToPrimitive};
use xous::{msg_scalar_unpack, CID};
use xous_ipc::*;

#[derive(Copy, Clone, Debug)]
struct ScalarCallback {
    server_to_cb_cid: CID,
    cb_to_client_cid: CID,
    cb_to_client_id: u32,
}

fn pump_thread() {
    log::info!("starting pump thread");

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    let xns = xous_names::XousNames::new().unwrap();
    let server_conn = xns
        .request_connection_blocking(api::SERVER_NAME)
        .expect("can't connect to main program");
    loop {
        xous::send_message(
            server_conn,
            xous::Message::new_scalar(Opcode::Tick.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send Tick message");
        ticktimer.sleep_ms(100).expect("couldn't sleep");
    }
}

#[xous::xous_main]
fn shell_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let server = xns
        .register_name(api::SERVER_NAME, None)
        .expect("can't register server");

    xous::create_thread_0(pump_thread).unwrap();
    log::info!("pump thread started");

    let mut tick_cb: [Option<ScalarCallback>; 32] = [None; 32];
    let mut req_cb: [bool; xous::MAX_CID] = [false; xous::MAX_CID];

    let mut ticks = 0;
    let mut state = 0;
    loop {
        let msg = xous::receive_message(server).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Tick) => {
                // pump the tick callbacks
                for maybe_conn in tick_cb.iter_mut() {
                    if let Some(scb) = maybe_conn {
                        match xous::send_message(
                            scb.server_to_cb_cid,
                            xous::Message::new_scalar(
                                api::TickCallback::Tick.to_usize().unwrap(),
                                scb.cb_to_client_cid as usize,
                                scb.cb_to_client_id as usize,
                                0,
                                0,
                            ),
                        ) {
                            Err(xous::Error::ServerNotFound) => {
                                *maybe_conn = None // automatically de-allocate callbacks for clients that have dropped
                            }
                            Ok(xous::Result::Ok) => {}
                            _ => panic!("unhandled error or result in callback processing"),
                        }
                    }
                }
                log::trace!("ticks: {}", ticks);
                ticks += 1;
                state += 1;
            }
            Some(Opcode::RegisterTickListener) => {
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut tick_cb);
            }
            Some(Opcode::RegisterReqListener) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = xous::connect(sid).unwrap();
                if (cid as usize) < req_cb.len() {
                    req_cb[cid as usize] = true;
                } else {
                    log::error!("cid out of allowable range");
                }
            }),
            Some(Opcode::UnregisterReqListener) => {
                msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                    let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                    let cid = xous::connect(sid).unwrap(); // if the connection already exists, this just looks it up in the table
                    log::info!("UnregisterReqListener cid {}", cid);
                    if (cid as usize) < req_cb.len() {
                        req_cb[cid as usize] = false;
                    } else {
                        log::error!("cid out of allowable range");
                    }
                    unsafe { xous::disconnect(cid).unwrap() };
                })
            }
            Some(Opcode::Req) => msg_scalar_unpack!(msg, _, _, _, _, {
                log::debug!("req_cb: {:?}", req_cb);
                // send results to request listeners
                for cid in 1..req_cb.len() {
                    // 0 is not a valid connection
                    if req_cb[cid as usize] {
                        match xous::send_message(
                            cid as u32,
                            xous::Message::new_scalar(
                                api::ResultCallback::Result.to_usize().unwrap(),
                                state as _,
                                0,
                                0,
                                0,
                            ),
                        ) {
                            Err(xous::Error::ServerNotFound) => {
                                log::info!("de-allocate ReqCallback");
                                req_cb[cid] = false;
                            }
                            Ok(xous::Result::Ok) => {}
                            _ => panic!("unhandled error or result in callback processing"),
                        }
                    }
                }
                /*
                for maybe_conn in req_cb.iter_mut() {
                    if let Some(conn) = maybe_conn {
                        match xous::send_message(*conn,
                            xous::Message::new_scalar(api::ResultCallback::Result.to_usize().unwrap(), state as _, 0, 0, 0)) {
                                Err(xous::Error::ServerNotFound) => {
                                    log::info!("de-allocate ReqCallback");
                                    *maybe_conn = None // automatically de-allocate callbacks for clients that have dropped
                                },
                                Ok(xous::Result::Ok) => {}
                                _ => panic!("unhandled error or result in callback processing")
                        }
                    }
                }*/
            }),
            None => {
                log::error!("couldn't convert opcode");
            }
        }
    }
}

fn do_hook(hookdata: ScalarHook, cb_conns: &mut [Option<ScalarCallback>; 32]) {
    let (s0, s1, s2, s3) = hookdata.sid;
    let sid = xous::SID::from_u32(s0, s1, s2, s3);
    let server_to_cb_cid = xous::connect(sid).unwrap();
    let cb_dat = Some(ScalarCallback {
        server_to_cb_cid,
        cb_to_client_cid: hookdata.cid,
        cb_to_client_id: hookdata.id,
    });
    let mut found = false;
    for entry in cb_conns.iter_mut() {
        if entry.is_none() {
            *entry = cb_dat;
            found = true;
            break;
        }
    }
    if !found {
        log::error!("ran out of space registering callback");
    }
}
