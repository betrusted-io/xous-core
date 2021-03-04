#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use core::convert::TryFrom;

use log::{error, info};

use graphics_server::{Gid, TextView};
use heapless::Vec;
use heapless::consts::U32;
use core::pin::Pin;
use ime_plugin_api::{PredictionTriggers, PredictionPlugin, PredictionApi};

use rkyv::Unarchive;
use rkyv::archived_value;

struct InputTracker {
    /// connection for handling graphical update requests
    pub gam_conn: xous::CID,


    /// input area canvas, as given by the GAM
    input_canvas: Option<Gid>,
    /// prediction display area, as given by the GAM
    pred_canvas: Option<Gid>,

    /// our current prediction engine
    predictor: Option<PredictionPlugin>,
    /// cached copy of the predictor's triggers for predictions. Only valid if predictor is not None
    pred_triggers: Option<PredictionTriggers>,

    /// track the progress of our input line
    line: xous::String::<4096>,
    /// we need to track character widths so we can draw a cursor
    charwidths: [u8; 4096],
    /// what position the insertion cursor is at in the string. 0 is inserting elements into the front of the string
    insertion: u8,

    predictions: [Option<TextView>; 4],
}


impl InputTracker {
    pub fn new(gam_conn: xous::CID)-> InputTracker {
        InputTracker {
            gam_conn,
            input_canvas: None,
            pred_canvas: None,
            predictor: None,
            pred_triggers: None,
            line: xous::String::<4096>::new(),
            charwidths: [0; 4096],
            insertion: 0,
            predictions: [None; 4],
        }
    }
    pub fn set_predictor(&mut self, predictor: PredictionPlugin) {
        self.predictor = Some(predictor);
        self.pred_triggers = Some(predictor.get_prediction_triggers()
        .expect("IMEF: InputTracker failed to get prediction triggers from plugin"));
    }
    pub fn set_input_canvas(&mut self, input: Gid) {
        self.input_canvas = Some(input);
    }
    pub fn set_pred_canvas(&mut self, pred: Gid) {
        self.pred_canvas = Some(pred);
    }
    pub fn is_init(&self) -> bool {
        self.input_canvas.is_some() && self.pred_canvas.is_some()
    }

    pub fn update(&mut self, newkeys: [char; 4]) {

        // just draw a rectangle for the prediction area for now

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

    let mut tracker = InputTracker::new(
        xous_names::request_connection_blocking(xous::names::SERVER_NAME_GAM).expect("IMEF: can't connect to GAM"),
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
        if tracker.is_init() {
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
                        Ok(pc) => tracker.set_predictor(ime_plugin_api::PredictionPlugin {connection: Some(pc)}),
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
