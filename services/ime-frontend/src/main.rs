#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]


use ime_plugin_api::ImefOpcode;

use core::convert::TryFrom;
use core::fmt::Write;

use log::{error, info};

use graphics_server::{Gid, Line, PixelColor, Point, Rectangle, TextBounds, TextView, DrawStyle};
use blitstr_ref as blitstr;
use blitstr::Cursor;
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
    insertion: Cursor,
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
            insertion: Cursor::new(0, 0, 0), // canvases always have (0,0) as the top left
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
        self.input_canvas.is_some() && self.pred_canvas.is_some() && self.predictor.is_some()
    }

    pub fn update(&mut self, newkeys: [char; 4]) -> Result<(), xous::Error> {
        let debug1= false;
        // just draw a rectangle for the prediction area for now
        if let Some(pc) = self.pred_canvas {
            if debug1{info!("IMEF: updating prediction area");}
            let pc_bounds: Point = gam::get_canvas_bounds(self.gam_conn, pc).expect("IMEF: Couldn't get prediction canvas bounds");
            if debug1{info!("IMEF: got pc_bound {:?}", pc_bounds);}
            let mut starting_tv = TextView::new(pc, 255,
                TextBounds::BoundingBox(Rectangle::new(Point::new(0, 0), pc_bounds)));
            starting_tv.draw_border = false;
            starting_tv.border_width = 1;
            starting_tv.clear_area = false;

            if debug1{info!("IMEF: posting textview {:?}", starting_tv);}
            gam::post_textview(self.gam_conn, &mut starting_tv).expect("IMEF: can't draw prediction TextView");

            // add the border line on top
            gam::draw_line(self.gam_conn, pc,
                Line::new_with_style(
                    Point::new(0,0),
                    Point::new(pc_bounds.x, 0),
                   DrawStyle {
                       fill_color: None,
                       stroke_color: Some(PixelColor::Dark),
                       stroke_width: 1,
                   })
            ).expect("IMEF: can't draw prediction top border");
        }

        if let Some(ic) = self.input_canvas {
            if debug1{info!("IMEF: updating input area");}
            let mut ic_bounds: Point = gam::get_canvas_bounds(self.gam_conn, ic).expect("IMEF: Couldn't get input canvas bounds");
            let mut input_tv = TextView::new(ic, 255,
                TextBounds::BoundingBox(Rectangle::new(Point::new(0,1), ic_bounds)));
            input_tv.draw_border = false;
            input_tv.border_width = 1;
            input_tv.clear_area = true;

            for &k in newkeys.iter() {
                if debug1{info!("IMEF: got key '{}'", k);}
                match k {
                    '\u{0000}' => (),
                    '\u{000d}' => {
                        // carriage return case
                        write!(self.line, "").expect("IMEF: can't clear line after carriage return");
                    },
                    _ => {
                        self.line.push(k).expect("IMEF: ran out of space pushing character into input line");
                        write!(input_tv.text, "{}", self.line.as_str().expect("IMEF: couldn't convert str")).expect("IMEF: couldn't update TextView string in input canvas");
                        gam::post_textview(self.gam_conn, &mut input_tv).expect("IMEF: can't draw input TextView");
                        if debug1{info!("IMEF: got computed cursor of {:?}", input_tv.cursor);}
                        if self.insertion.pt.y == input_tv.cursor.pt.y {
                            self.charwidths[self.line.len()] = (input_tv.cursor.pt.x - self.insertion.pt.x) as u8;
                        } else {
                            // line wrapped, assume we wrapped to x = 0
                            self.charwidths[self.line.len()] = (input_tv.cursor.pt.x - 0) as u8;
                        }
                        self.insertion.pt.y = input_tv.cursor.pt.y;
                        self.insertion.pt.x = input_tv.cursor.pt.x;
                        self.insertion.line_height = input_tv.cursor.line_height;
                    },
                }
            }

            // draw the insertion point
            // do a manual type conversion, because external crate etc. etc.
            let ins_pt: Point = Point::new(self.insertion.pt.x as i16, self.insertion.pt.y as i16);

            if debug1{info!("IMEF: drawing insertion point: {:?}, height: {}", ins_pt, self.insertion.line_height);}
            gam::draw_line(self.gam_conn, ic,
                Line::new(ins_pt,
                    ins_pt + Point::new(0, self.insertion.line_height as i16 - input_tv.margin.y) ))
                    .expect("IMEF: can't draw insertion point");

            // add the border line on top
            gam::draw_line(self.gam_conn, ic,
                Line::new_with_style(
                    Point::new(0,0),
                    Point::new(ic_bounds.x, 0),
                    DrawStyle {
                        fill_color: None,
                        stroke_color: Some(PixelColor::Dark),
                        stroke_width: 1,
                    }))
                    .expect("IMEF: can't draw input top line border");
        }
        /*
        what else do we need:
        - current string that is being built up
        - cursor position in string, so we can do insertion/deletion
        */
        Ok(())
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = false;
    log_server::init_wait().unwrap();
    info!("IMEF: my PID is {}", xous::process::id());

    let imef_sid = xous_names::register_name(xous::names::SERVER_NAME_IME_FRONT).expect("IMEF: can't register server");
    info!("IMEF: registered with NS -- {:?}", imef_sid);

    let kbd_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_KBD).expect("IMEF: can't connect to KBD");
    keyboard::request_events(xous::names::SERVER_NAME_IME_FRONT, kbd_conn).expect("IMEF: couldn't request events from keyboard");

    let mut tracker = InputTracker::new(
        xous_names::request_connection_blocking(xous::names::SERVER_NAME_GAM).expect("IMEF: can't connect to GAM"),
    );
    // The main lop can't start until we've been assigned Gids from the GAM, and a Predictor.
    info!("IMEF: waiting for my canvas Gids");
    loop {
        let envelope = xous::receive_message(imef_sid).unwrap();
        if let Ok(opcode) = ImefOpcode::try_from(&envelope.body) {
            match opcode {
                ImefOpcode::SetInputCanvas(g) => {
                    if debug1{info!("IMEF: got input canvas {:?}", g);}
                    tracker.set_input_canvas(g);
                },
                ImefOpcode::SetPredictionCanvas(g) => {
                    if debug1{info!("IMEF: got prediction canvas {:?}", g);}
                    tracker.set_pred_canvas(g);
                },
                _ => info!("IMEF: expected canvas Gid, but got {:?}", opcode)
            }
        } else if let xous::Message::Borrow(m) = &envelope.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<ImefOpcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<ImefOpcode>::SetPredictionServer(rkyv_s) => {
                    let s: xous::String<256> = rkyv_s.unarchive();
                    match xous_names::request_connection(s.as_str().expect("IMEF: SetPrediction received malformed server name")) {
                        Ok(pc) => tracker.set_predictor(ime_plugin_api::PredictionPlugin {connection: Some(pc)}),
                        _ => error!("IMEF: can't find predictive engine {}, retaining existing one.", s.as_str().expect("IMEF: SetPrediction received malformed server name")),
                    }
                },
                _ => panic!("IME_SH: invalid response from server -- corruption occurred in MemoryMessage")
            };
        } else {
            info!("IMEF: expected canvas Gid, but got other message first {:?}", envelope);
        }
        if tracker.is_init() {
            break;
        }
    }

    // force a redraw of the UI with no keys
    if debug1{info!("IMEF: forcing initial UI redraw");}
    tracker.update(['\u{0000}'; 4]).expect("IMEF: couldn't redraw initial UI");

    let mut key_queue: Vec<char, U32> = Vec::new();
    info!("IMEF: entering main loop");
    loop {
        let envelope = xous::receive_message(imef_sid).unwrap();
        if debug1{info!("IMEF: got message {:?}", envelope);}
        if let Ok(opcode) = ImefOpcode::try_from(&envelope.body) {
            match opcode {
                ImefOpcode::SetInputCanvas(g) => {
                    // there are valid reasons for this to happen, but it should be rare.
                    info!("IMEF: warning: input canvas Gid has been reset");
                    tracker.set_input_canvas(g);
                },
                ImefOpcode::SetPredictionCanvas(g) => {
                    // there are valid reasons for this to happen, but it should be rare.
                    info!("IMEF: warning: prediction canvas Gid has been reset");
                    tracker.set_pred_canvas(g);
                },
                _ => info!("IMEF: unhandled opcode {:?}", opcode)
            }
        } else if let xous::Message::Borrow(m) = &envelope.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<ImefOpcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<ImefOpcode>::SetPredictionServer(rkyv_s) => {
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
                    tracker.update(keys).expect("IMEF: couldn't update input tracker with latest key presses");
                },
                _ => error!("IMEF: received KBD event opcode that wasn't expected"),
            }
        } else {
            info!("IMEF: expected canvas Gid, but got {:?}", envelope);
        }
    }

}
