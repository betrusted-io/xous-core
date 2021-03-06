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
    /// length of the line in *characters*, not bytes (which is what .len() returns), used to index char_locs
    characters: usize,
    /// the insertion point, 0 is inserting characters before the first, 1 inserts characters after the first, etc.
    insertion: usize,
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
            characters: 0,
            insertion: 0,
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

    pub fn clear_area(&mut self) -> Result<(), xous::Error> {
        if let Some(pc) = self.pred_canvas {
            let pc_bounds: Point = gam::get_canvas_bounds(self.gam_conn, pc).expect("IMEF: Couldn't get prediction canvas bounds");
            gam::draw_rectangle(self.gam_conn, pc,
                Rectangle::new_with_style(Point::new(0, 0), pc_bounds,
                DrawStyle {
                    fill_color: Some(PixelColor::Light),
                    stroke_color: None,
                    stroke_width: 0
                }
            )).expect("IMEF: can't clear prediction area");
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
            let mut ic_bounds: Point = gam::get_canvas_bounds(self.gam_conn, ic).expect("IMEF: Couldn't get input canvas bounds");
            gam::draw_rectangle(self.gam_conn, ic,
                Rectangle::new_with_style(Point::new(0, 0), ic_bounds,
                DrawStyle {
                    fill_color: Some(PixelColor::Light),
                    stroke_color: None,
                    stroke_width: 0
                }
            )).expect("IMEF: can't clear input area");

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

        Ok(())
    }

    pub fn update(&mut self, newkeys: [char; 4]) -> Result<(), xous::Error> {
        let debug1= true;
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
        }

        if let Some(ic) = self.input_canvas {
            if debug1{info!("IMEF: updating input area");}
            let mut ic_bounds: Point = gam::get_canvas_bounds(self.gam_conn, ic).expect("IMEF: Couldn't get input canvas bounds");
            let mut input_tv = TextView::new(ic, 255,
                TextBounds::BoundingBox(Rectangle::new(Point::new(0,1), ic_bounds)));
            input_tv.draw_border = false;
            input_tv.border_width = 1;
            input_tv.clear_area = true; // need this so that the insertion point is cleared and moved

            let mut do_redraw = false;
            for &k in newkeys.iter() {
                if debug1{info!("IMEF: got key '{}'", k);}
                match k {
                    '\u{0000}' => (),
                    '←' => { // move insertion point back
                        if self.insertion > 0 {
                            info!("IMEF: moving insertion point back");
                            self.insertion -= 1;
                        }
                        do_redraw = true;
                    }
                    '→' => {
                        if self.insertion < self.characters {
                            self.insertion += 1;
                        }
                        do_redraw = true;
                    }
                    '\u{0008}' => { // backspace
                        if (self.characters > 0) && (self.insertion == self.characters) {
                            self.line.pop();
                            self.characters -= 1;
                            self.insertion -= 1;
                            do_redraw = true;
                        } else if (self.characters > 0) && (self.insertion > 0) {
                            // awful O(N) algo because we have to decode variable-length utf8 strings to figure out character boundaries
                            // first, make a copy of the string
                            let tempbytes: [u8; 4096] = self.line.as_bytes();
                            let tempstr = unsafe { core::str::from_utf8_unchecked(&tempbytes[0..self.line.len()]) }.clone();
                            // clear the string
                            self.line.clear();

                            // delete the character in the array
                            let mut i = 0;
                            for c in tempstr.chars() {
                                if debug1{info!("checking index {}", i);}
                                if i == self.insertion - 1 {
                                    if debug1{info!("skipping char");}
                                } else {
                                    if debug1{info!("copying char {}", c);}
                                    self.line.push(c).expect("IMEF: ran out of space inserting orignial characters into input line")
                                }
                                i += 1;
                            }
                            self.insertion -= 1;
                            self.characters -= 1;
                            do_redraw = true;
                        } else {
                            // ignore, we are either at the front of the string, or the string had no characters
                        }
                    }
                    '\u{000d}' => { // carriage return
                        // TODO: send string to registered listeners
                        // TODO: update the predictor on carriage return

                        if debug1{info!("IMEF: got carriage return");}
                        self.line.clear();
                        // clear all the temporary variables
                        self.characters = 0;
                        self.insertion = 0;
                        self.clear_area().expect("IMEF: can't clear on carriage return");
                    },
                    _ => {
                        if self.insertion == self.characters {
                            self.line.push(k).expect("IMEF: ran out of space pushing character into input line");
                            self.characters += 1;
                            self.insertion += 1;
                            do_redraw = true;
                        } else {
                            if debug1{info!("IMEF: handling case of inserting characters. insertion: {}", self.insertion)};
                            // awful O(N) algo because we have to decode variable-length utf8 strings to figure out character boundaries
                            // first, make a copy of the string
                            let tempbytes: [u8; 4096] = self.line.as_bytes();
                            let tempstr = unsafe { core::str::from_utf8_unchecked(&tempbytes[0..self.line.len()]) }.clone();
                            // clear the string
                            self.line.clear();

                            // insert the character in the array
                            let mut i = 0;
                            for c in tempstr.chars() {
                                if debug1{info!("checking index {}", i);}
                                if i == self.insertion {
                                    if debug1{info!("inserting char {}", k);}
                                    self.line.push(k).expect("IMEF: ran out of space inserting new character into input line");
                                    self.line.push(c).expect("IMEF: ran out of space inserting orignial characters into input line")
                                } else {
                                    if debug1{info!("copying char {}", c);}
                                    self.line.push(c).expect("IMEF: ran out of space inserting orignial characters into input line")
                                }
                                i += 1;
                            }
                            self.insertion += 1;
                            self.characters += 1;
                            do_redraw = true;
                        }
                    },
                }
            }

            input_tv.insertion = Some(self.insertion as u32);
            info!("IMEF: insertion point is {}, characters in string {}", self.insertion, self.characters);
            if do_redraw {
                write!(input_tv.text, "{}", self.line.as_str().expect("IMEF: couldn't convert str")).expect("IMEF: couldn't update TextView string in input canvas");
                gam::post_textview(self.gam_conn, &mut input_tv).expect("IMEF: can't draw input TextView");
                if debug1{info!("IMEF: got computed cursor of {:?}", input_tv.cursor);}
            }
        }
        /*
        what else do we need:
        - backspace capability
        - insertion point movement and character insert

        - height up request of canvas when string wraps, and reset of canvas height after carriage return
        - registration for listening to string results
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

    // force a redraw of the UI
    if debug1{info!("IMEF: forcing initial UI redraw");}
    tracker.clear_area().expect("IMEF: can't initially clear areas");

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
