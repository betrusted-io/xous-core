#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use core::convert::TryFrom;

use log::{error, info};

use graphics_server::Gid;
use heapless::Vec;
use heapless::consts::U32;

/*
what else do we need:
  - current string that is being built up
  - cursor position in string, so we can do insertion/deletion
 */
fn draw_canvas(gam_conn: xous::CID, canvas: Gid, pred_conn: xous::CID, newkeys: [char; 4]) {

    // this is just reference to remind me how to decode the key array
    for &k in newkeys.iter() {
        if k != '\u{0000}' {
            key_queue.push(k).unwrap();
            if debug1{info!("IMEF: got key '{}'", k);}
        }
    }
}


#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = true;
    log_server::init_wait().unwrap();
    info!("IMEF: my PID is {}", xous::process::id());

    let imef_sid = xous_names::register_name(xous::names::SERVER_NAME_IME_FRONT).expect("IMEF: can't register server");
    info!("IMEF: registered with NS -- {:?}", imef_sid);

    let kbd_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_KBD).expect("IMEF: can't connect to KBD");
    keyboard::request_events(xous::names::SERVER_NAME_IME_FRONT, kbd_conn).expect("IMEF: couldn't request events from keyboard");

    let gam_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_GAM).expect("IMEF: can't connect to GAM");

    // set a "default" prediction that is a shell-like history buffer of things previously typed
    let mut prediction_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_IME_PLUGIN_SHELL)
    .expect("IMEF: can't connect to shell prediction engine");

    // The first message should be my Gid from the GAM.
    let mut canvas: Gid = Gid::new([0,0,0,0]);
    info!("IMEF: waiting for my canvas Gid");
    loop {
        let envelope = xous::receive_message(imef_sid).unwrap();
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            match opcode {
                Opcode::SetCanvas(g) => {
                    canvas = g;
                },
                _ => info!("IMEF: expected canvas Gid, but got {:?}", opcode)
            }
        } else {
            info!("IMEF: expected canvas Gid, but got other message first {:?}", envelope);
        }
    }

    let mut key_queue: Vec<char, U32> = Vec::new();
    info!("IMEF: entering main loop");
    loop {
        let envelope = xous::receive_message(imef_sid).unwrap();
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            match opcode {
                Opcode::SetCanvas(g) => {
                    // there are valid reasons for this to happen, but it should be rare.
                    info!("IMEF: warning: canvas Gid has been reset");
                    canvas = g;
                },
                _ => info!("IMEF: unhandled opcode {:?}", opcode)
            }
        } else if let xous::Message::Borrow(m) = &msg.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<api::Opcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<api::Opcode>::SetPrediction(rkyv_s) => {
                    let s: xous::String<4096> = rkyv_s.unarchive();
                    prediction_conn = xous_names::request_connection(s.as_str());
                },
                _ => panic!("IME_SH: invalid response from server -- corruption occurred in MemoryMessage")
            };
        } else if let Ok(opcode) = keyboard::api::Opcode::try_from(&envelope.body) {
            match opcode {
                keyboard::api::Opcode::KeyboardEvent(keys) => {
                    draw_canvas(gam_conn, canvas, prediction_conn, keys);
                },
                _ => error!("IMEF: received KBD event opcode that wasn't expected"),
            }
        } else {
            info!("IMEF: expected canvas Gid, but got {:?}", envelope);
        }
    }

}
