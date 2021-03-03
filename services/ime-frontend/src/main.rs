#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use core::convert::TryFrom;

use log::{error, info};

use graphics_server::Gid;
use heapless::Vec;
use heapless::consts::U32;
use core::pin::Pin;
use ime_plugin_api::{PredictionTriggers, PredictionApi};

use rkyv::Unarchive;
use rkyv::archived_value;

struct InputTracker {
    /// connection for handling graphical update requests
    pub gam_conn: xous::CID,
    /// input area canvas, as given by the GAM
    pub input_canvas: Option<Gid>,
    /// a connection to our current prediction engine
    pub pred_conn: xous::CID,
    /// prediction display area, as given by the GAM
    pub pred_canvas: Option<Gid>,
    /// triggers for predictions
    pub pred_triggers: PredictionTriggers,

    /// track the progress of our input line
    line: xous::String::<4096>,
    /// we need to track character widths so we can draw a cursor
    charwidths: [u8; 4096],
    /// what position the insertion cursor is at in the string. 0 is inserting elements into the front of the string
    insertion: u8,
}

impl InputTracker {
    pub fn new(gam_conn: xous::CID, pred_conn: xous::CID, predictor: impl PredictionApi)-> InputTracker {
        // a little dangerous: bypassing any explicit API call implementation for this one
        /*
        let response = xous::send_message(pred_conn, ime_plugin_api::Opcode::GetPredictionTriggers.into())
            .expect("IMEF: InputTracker failed to get predictions from default plugin");
        if let xous::Result::Scalar1(code) = response {
            InputTracker {
                gam_conn,
                input_canvas: None,
                pred_conn,
                pred_canvas: None,
                pred_triggers: code.into(),
                line: xous::String::<4096>::new(),
                charwidths: [0; 4096],
                insertion: 0,
            }
        } else {
            panic!("IMEF: InputTracker::new() failed to get prediction triggers")
        }*/

        let pred_triggers = predictor.get_prediction_triggers(pred_conn)
            .expect("IMEF: InputTracker failed to get predictions from default plugin");
        InputTracker {
            gam_conn,
            input_canvas: None,
            pred_conn,
            pred_canvas: None,
            pred_triggers,
            line: xous::String::<4096>::new(),
            charwidths: [0; 4096],
            insertion: 0,
        }
    }

    pub fn update(&mut self, newkeys: [char; 4]) {

        /*
        what else do we need:
        - current string that is being built up
        - cursor position in string, so we can do insertion/deletion
        */
        // this is just reference to remind me how to decode the key array
        /*
        for &k in newkeys.iter() {
            if k != '\u{0000}' {
                key_queue.push(k).unwrap();
                if debug1{info!("IMEF: got key '{}'", k);}
            }
        }*/
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

    let default_predictor = ime_plugin_api::StandardPredictionPlugin {};
    let mut tracker = InputTracker::new(
        xous_names::request_connection_blocking(xous::names::SERVER_NAME_GAM).expect("IMEF: can't connect to GAM"),
        // set a "default" prediction that is a shell-like history buffer of things previously typed
        xous_names::request_connection_blocking(xous::names::SERVER_NAME_IME_PLUGIN_SHELL).expect("IMEF: can't connect to shell prediction engine"),
        default_predictor
    );

    // The first message should be my Gid from the GAM.
    info!("IMEF: waiting for my canvas Gids");
    loop {
        let envelope = xous::receive_message(imef_sid).unwrap();
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            match opcode {
                Opcode::SetInputCanvas(g) => {
                    if debug1{info!("IMEF: got input canvas {:?}", g);}
                    tracker.input_canvas = Some(g);
                },
                Opcode::SetPredictionCanvas(g) => {
                    if debug1{info!("IMEF: got prediction canvas {:?}", g);}
                    tracker.pred_canvas = Some(g);
                },
                _ => info!("IMEF: expected canvas Gid, but got {:?}", opcode)
            }
        } else {
            info!("IMEF: expected canvas Gid, but got other message first {:?}", envelope);
        }
        if tracker.input_canvas.is_some() && tracker.pred_canvas.is_some() {
            break;
        }
    }

    let mut key_queue: Vec<char, U32> = Vec::new();
    info!("IMEF: entering main loop");
    loop {
        let envelope = xous::receive_message(imef_sid).unwrap();
        if debug1{info!("IMEF: got message {:?}", envelope);}
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            match opcode {
                Opcode::SetInputCanvas(g) => {
                    // there are valid reasons for this to happen, but it should be rare.
                    info!("IMEF: warning: input canvas Gid has been reset");
                    tracker.input_canvas = Some(g);
                },
                Opcode::SetPredictionCanvas(g) => {
                    // there are valid reasons for this to happen, but it should be rare.
                    info!("IMEF: warning: prediction canvas Gid has been reset");
                    tracker.pred_canvas = Some(g);
                },
                _ => info!("IMEF: unhandled opcode {:?}", opcode)
            }
        } else if let xous::Message::Borrow(m) = &envelope.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<api::Opcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<api::Opcode>::SetPredictionServer(rkyv_s) => {
                    let s: xous::String<256> = rkyv_s.unarchive();
                    match xous_names::request_connection(s.as_str().expect("IMEF: SetPrediction received malformed server name")) {
                        Ok(pc) => tracker.pred_conn = pc,
                        _ => error!("IMEF: can't find predictive engine {}, retaining existing one.", s.as_str().expect("IMEF: SetPrediction received malformed server name")),
                    }
                },
                _ => panic!("IME_SH: invalid response from server -- corruption occurred in MemoryMessage")
            };
        } else if let Ok(opcode) = keyboard::api::Opcode::try_from(&envelope.body) {
            match opcode {
                keyboard::api::Opcode::KeyboardEvent(keys) => {
                    tracker.update(keys);
                },
                _ => error!("IMEF: received KBD event opcode that wasn't expected"),
            }
        } else {
            info!("IMEF: expected canvas Gid, but got {:?}", envelope);
        }
    }

}
