#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::{ToPrimitive, FromPrimitive};
use xous_ipc::*;
use api::Opcode;
use xous::{CID, msg_scalar_unpack};

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
    let server_conn = xns.request_connection_blocking(api::SERVER_NAME).expect("can't connect to main program");
    loop {
        xous::send_message(server_conn,
            xous::Message::new_scalar(Opcode::Tick.to_usize().unwrap(), 0, 0, 0, 0)).expect("couldn't send Tick message");
        ticktimer.sleep_ms(2_000).expect("couldn't sleep");
    }
}

#[xous::xous_main]
fn shell_main() -> ! {
    log_server::init_wait().unwrap();
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let server = xns.register_name(api::SERVER_NAME).expect("can't register server");

    xous::create_thread_0(pump_thread).unwrap();
    log::info!("pump thread started");

    let mut tick_cb: [Option<ScalarCallback>; 32] = [None; 32];
    let mut add_cb: [Option<CID>; 32] = [None; 32];

    let mut sum = 0;
    loop {
        let msg = xous::receive_message(server).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Tick) => {
                // pump the tick callbacks
                for maybe_conn in tick_cb.iter_mut() {
                if let Some(scb) = maybe_conn {
                    match xous::send_message(scb.server_to_cb_cid,
                        xous::Message::new_scalar(api::TickCallback::Tick.to_usize().unwrap(),
                           scb.cb_to_client_cid as usize, scb.cb_to_client_id as usize, 0, 0)) {
                            Err(xous::Error::ServerNotFound) => {
                                *maybe_conn = None // automatically de-allocate callbacks for clients that have dropped
                            },
                            Ok(xous::Result::Ok) => {}
                            _ => panic!("unhandled error or result in callback processing")
                        }
                    }
                }
            },
            Some(Opcode::RegisterTickListener) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut tick_cb);
            },
            Some(Opcode::RegisterAddListener) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = Some(xous::connect(sid).unwrap());
                let mut found = false;
                for entry in add_cb.iter_mut() {
                    if *entry == None {
                        *entry = cid;
                        found = true;
                        break;
                    }
                }
                if !found {
                    log::error!("RegisterTickListener ran out of space registering callback");
                }
            }),
            Some(Opcode::Add) => msg_scalar_unpack!(msg, s, _, _, _, {
                sum += s;
                // send results to add listeners
                for maybe_conn in add_cb.iter_mut() {
                    if let Some(conn) = maybe_conn {
                        match xous::send_message(*conn,
                            xous::Message::new_scalar(api::AddCallback::Sum.to_usize().unwrap(), sum, 0, 0, 0)) {
                                Err(xous::Error::ServerNotFound) => {
                                    *maybe_conn = None // automatically de-allocate callbacks for clients that have dropped
                                },
                                Ok(xous::Result::Ok) => {}
                                _ => panic!("unhandled error or result in callback processing")
                        }
                    }
                }
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