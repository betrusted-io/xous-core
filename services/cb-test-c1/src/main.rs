#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use cb_test_srv::*;
use num_traits::{FromPrimitive, ToPrimitive};

const SERVER_NAME: &str = "_CB test client 1_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum Opcode {
    Tick,
    Result,
}

use core::sync::atomic::{AtomicU32, Ordering};
static CB_TO_MAIN_CONN: AtomicU32 = AtomicU32::new(0);
fn do_result(result: u32) {
    let cb_to_main_conn = CB_TO_MAIN_CONN.load(Ordering::Relaxed);
    if cb_to_main_conn != 0 {
        xous::send_message(
            cb_to_main_conn,
            xous::Message::new_scalar(Opcode::Result.to_usize().unwrap(), result as usize, 0, 0, 0),
        )
        .unwrap();
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns
        .register_name(SERVER_NAME, None)
        .expect("can't register server");
    log::trace!("registered with NS -- {:?}", sid);

    CB_TO_MAIN_CONN.store(xous::connect(sid).unwrap(), Ordering::Relaxed);

    let mut cb_serv = CbTestServer::new(&xns).unwrap();
    let tick_cid = xous::connect(sid).unwrap();
    cb_serv
        .hook_tick_callback(Opcode::Tick.to_u32().unwrap(), tick_cid)
        .unwrap();

    log::trace!("ready to accept requests");
    let mut state = 0;
    let mut hooked = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Tick) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                if (state % 2) == 0 {
                    log::trace!("hook1");
                    cb_serv.hook_req_callback(do_result).unwrap();
                    hooked = true;
                    cb_serv.req().unwrap();
                } else {
                    log::trace!("idle1");
                }
                state += 1;
            }),
            Some(Opcode::Result) => xous::msg_scalar_unpack!(msg, result, _, _, _, {
                log::info!("C1 result: {}", result);
                if hooked == false {
                    log::info!("**C1 without hook");
                }
                cb_serv.unhook_req_callback().unwrap();
                hooked = false;
                log::trace!("unhook1");
            }),
            None => {
                log::error!("couldn't convert opcode");
                break;
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
