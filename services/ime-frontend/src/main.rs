#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]


use gam::api::SetCanvasBoundsRequest;
use ime_plugin_api::ImefOpcode;

use core::convert::TryFrom;
use core::fmt::Write;

use log::{error, info};

use graphics_server::{Gid, Line, PixelColor, Point, Rectangle, TextBounds, TextView, DrawStyle};
use blitstr_ref as blitstr;
use heapless::Vec;
use heapless::consts::U32;
use core::pin::Pin;
use ime_plugin_api::{PredictionTriggers, PredictionPlugin, PredictionApi};

use rkyv::Unarchive;
use rkyv::archived_value;

/// max number of prediction options to track/render
const MAX_PREDICTION_OPTIONS: usize = 4;

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
    /// set if we're in a state where a backspace should trigger an unpredict
    can_unpick: bool, // note: untested as of Mar 7 2021
    /// the predictor string -- this is different from the input line, because it can be broken up by spaces and punctuatino
    pred_phrase: xous::String::<4096>, // note: untested as of Mar 7 2021
    /// character position of the last prediction trigger -- this is where the prediction overwrite starts
    /// if None, it means we were unable to determine the trigger (e.g., we went back and edited text manually)
    last_trigger_char: Option<usize>,

    /// track the progress of our input line
    line: xous::String::<4096>,
    /// length of the line in *characters*, not bytes (which is what .len() returns), used to index char_locs
    characters: usize,
    /// the insertion point, 0 is inserting characters before the first, 1 inserts characters after the first, etc.
    insertion: usize,
    /// last returned line height, which is used as a reference for growing the area when we run out of space
    last_height: u32,
    /// keep track if our box was grown
    was_grown: bool,

    /// render the predictions
    pred_options: [Option<xous::String::<4096>>; MAX_PREDICTION_OPTIONS],
}

impl InputTracker {
    pub fn new(gam_conn: xous::CID)-> InputTracker {
        InputTracker {
            gam_conn,
            input_canvas: None,
            pred_canvas: None,
            predictor: None,
            pred_triggers: None,
            can_unpick: false,
            pred_phrase: xous::String::<4096>::new(),
            last_trigger_char: Some(0),
            line: xous::String::<4096>::new(),
            characters: 0,
            insertion: 0,
            last_height: 0,
            was_grown: false,
            pred_options: [None; MAX_PREDICTION_OPTIONS],
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
            let ic_bounds: Point = gam::get_canvas_bounds(self.gam_conn, ic).expect("IMEF: Couldn't get input canvas bounds");
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

    fn insert_prediction(&mut self, index: usize) {
        let pred_str = match self.pred_options[index] {
            Some(s) => s,
            _ => return // if the index doesn't exist for some reason, do nothing without throwing an error
        };
        if let Some(offset) = self.last_trigger_char {
            if offset < self.characters {
                // copy the bytes in the original string, up to the offset; and then copy the bytes in the selected predictor
                let tempbytes: [u8; 4096] = self.line.as_bytes();
                let tempstr = unsafe { core::str::from_utf8_unchecked(&tempbytes[0..self.line.len()]) }.clone();
                self.line.clear();
                let mut chars = 0;
                let mut c_iter = tempstr.chars();
                loop {
                    if chars == offset {
                        break;
                    }
                    if let Some(c) = c_iter.next() {
                        self.line.push(c).unwrap(); // this should be infallible
                        chars += 1;
                    } else {
                        break;
                    }
                }
                // now push the characters in the predicted options onto the line
                for c in pred_str.as_str().unwrap().chars() {
                    self.line.push(c).expect("IMEF: ran out of space inserting prediction");
                    chars += 1;
                }
                // forward until we find the next prediction trigger in the original string
                while let Some(c) = c_iter.next() {
                    if let Some(trigger) = self.pred_triggers {
                        if trigger.whitespace && c.is_ascii_whitespace() ||
                           trigger.punctuation && c.is_ascii_punctuation() {
                               // include the trigger
                               self.line.push(c).expect("IMEF: ran out of space inserting prediction");
                               chars += 1;
                               break;
                        }
                    } else {
                        // skip the replaced characters
                    }
                }
                self.insertion = chars;
                self.last_trigger_char = Some(chars);
                // copy the remainder of the line, if any
                while let Some(c) = c_iter.next() {
                    self.line.push(c).expect("IMEF: ran out of space inserting prediction");
                    chars += 1;
                }
                self.characters = chars;
            } else {
                // just append the prediction to the line
                for c in pred_str.as_str().unwrap().chars() {
                    self.line.push(c).expect("IMEF: ran out of space inserting prediction");
                    self.characters += 1;
                }
                self.last_trigger_char = Some(self.insertion);
                self.insertion = self.characters;
            }
        }
    }

    pub fn update(&mut self, newkeys: [char; 4]) -> Result<(), xous::Error> {
        let debug1= true;
        let mut update_predictor = false;
        if let Some(ic) = self.input_canvas {
            if debug1{info!("IMEF: updating input area");}
            let ic_bounds: Point = gam::get_canvas_bounds(self.gam_conn, ic).expect("IMEF: Couldn't get input canvas bounds");
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
                        self.pred_phrase.clear(); // don't track predictions on edits
                        self.can_unpick = false;
                        self.last_trigger_char = None;
                    }
                    '→' => {
                        if self.insertion < self.characters {
                            self.insertion += 1;
                        }
                        do_redraw = true;
                        self.pred_phrase.clear();
                        self.can_unpick = false;
                        self.last_trigger_char = None;
                    }
                    '↑' => {
                        // bring the insertion point to the front of the text box
                        self.insertion = 0;
                        do_redraw = true;
                        self.pred_phrase.clear();
                        self.can_unpick = false;
                        self.last_trigger_char = None;
                    }
                    '↓' => {
                        // bring insertion point to the very end of the text box
                        self.insertion = self.characters;
                        do_redraw = true;
                        self.pred_phrase.clear();
                        self.can_unpick = false;
                        // this means that when we resume typing after an edit, the predictor will set its insertion point
                        // at the very end, not the space prior to the last word...
                        self.last_trigger_char = Some(self.characters);
                    }
                    '\u{0011}' => { // F1
                        self.insert_prediction(0);
                        do_redraw = true;
                    }
                    '\u{0012}' => { // F2
                        self.insert_prediction(1);
                        do_redraw = true;
                    }
                    '\u{0013}' => { // F3
                        self.insert_prediction(2);
                        do_redraw = true;
                    }
                    '\u{0014}' => { // F4
                        self.insert_prediction(3);
                        do_redraw = true;
                    }
                    '\u{0008}' => { // backspace
                        if (self.characters > 0) && (self.insertion == self.characters) {
                            self.line.pop();
                            self.characters -= 1;
                            self.insertion -= 1;
                            do_redraw = true;

                            if let Some(predictor) = self.predictor {
                                if self.can_unpick {
                                    predictor.unpick().expect("IMEF: couldn't unpick last prediction");
                                    self.can_unpick = false;
                                    update_predictor = true;
                                }
                                self.pred_phrase.clear();
                            }
                        } else if (self.characters > 0)  && (self.insertion > 0) {
                            // awful O(N) algo because we have to decode variable-length utf8 strings to figure out character boundaries
                            // first, make a copy of the string
                            let tempbytes: [u8; 4096] = self.line.as_bytes();
                            let tempstr = unsafe { core::str::from_utf8_unchecked(&tempbytes[0..self.line.len()]) }.clone();
                            // clear the string
                            self.line.clear();

                            // delete the character in the array
                            let mut i = 0; // character position
                            let mut dest_chars = 0; // destination character position
                            for c in tempstr.chars() {
                                if debug1{info!("checking index {}", i);}
                                if i == self.insertion - 1 {
                                    if debug1{info!("skipping char");}
                                } else {
                                    dest_chars += 1;
                                    // if we encounter a trigger character, set the trigger to just after this point (hence the += before this line)
                                    if let Some(trigger) = self.pred_triggers {
                                        if trigger.punctuation && c.is_ascii_punctuation() {
                                            self.last_trigger_char = Some(dest_chars);
                                        } else if trigger.whitespace && c.is_ascii_whitespace() {
                                            self.last_trigger_char = Some(dest_chars);
                                        }
                                    }
                                    if debug1{info!("copying char {}", c);}
                                    self.line.push(c).expect("IMEF: ran out of space inserting orignial characters into input line");
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
                        if let Some(trigger) = self.pred_triggers {
                            if trigger.newline {
                                self.predictor.unwrap().feedback_picked(self.line).expect("IMEF: couldn't send feedback to predictor");
                            } else if trigger.punctuation {
                                self.predictor.unwrap().feedback_picked(self.pred_phrase).expect("IMEF: couldn't send feedback to predictor");
                            }
                        }
                        self.can_unpick = false;
                        self.pred_phrase.clear();

                        if debug1{info!("IMEF: got carriage return");}
                        self.line.clear();
                        self.last_trigger_char = Some(0);
                        // clear all the temporary variables
                        self.characters = 0;
                        self.insertion = 0;
                        if self.was_grown {
                            let mut req = SetCanvasBoundsRequest {
                                canvas: ic,
                                requested: Point::new(0, 0), // size 0 will snap to the original smallest default size
                                granted: None,
                            };
                            if debug1{info!("IMEF: attempting resize to {:?}", req.requested);}
                            gam::set_canvas_bounds_request(self.gam_conn, &mut req).expect("IMEF: couldn't call set_bounds_request on input area overflow");
                            if debug1{
                                info!("IMEF: carriage return resize to {:?}", req.granted);
                            }
                            self.last_height = 0;
                            self.was_grown = false;
                        }
                        self.clear_area().expect("IMEF: can't clear on carriage return");
                        update_predictor = true;
                    },
                    _ => {
                        if let Some(trigger) = self.pred_triggers {
                            if trigger.whitespace && k.is_ascii_whitespace() {
                                if self.pred_phrase.len() > 0 {
                                    self.predictor.unwrap().feedback_picked(self.pred_phrase).expect("IMEF: couldn't send feedback to predictor");
                                    self.pred_phrase.clear();
                                    self.can_unpick = true;
                                }
                                self.last_trigger_char = Some(self.insertion);
                            }
                            if trigger.punctuation && k.is_ascii_punctuation() {
                                if self.pred_phrase.len() > 0 {
                                    self.predictor.unwrap().feedback_picked(self.pred_phrase).expect("IMEF: couldn't send feedback to predictor");
                                    self.pred_phrase.clear();
                                    self.can_unpick = true;
                                }
                                self.last_trigger_char = Some(self.insertion);
                            }
                        }
                        if self.insertion == self.characters {
                            self.line.push(k).expect("IMEF: ran out of space pushing character into input line");
                            if let Some(trigger) = self.pred_triggers {
                                if !(trigger.punctuation && k.is_ascii_punctuation() ||
                                    trigger.whitespace  && k.is_ascii_whitespace() ) {
                                    self.pred_phrase.push(k).expect("IMEF: ran out of space pushing character into prediction phrase");
                                    update_predictor = true;
                                }
                            }
                            self.characters += 1;
                            self.insertion += 1;
                            do_redraw = true;
                        } else {
                            // we're going back and editing -- clear predictions in this case
                            if self.pred_phrase.len() > 0 {
                                self.pred_phrase.clear();
                                self.can_unpick = false; // we don't know how far back the user is going to make the edit

                                // in order to do predictions on arbitrary words, every time the scroll keys are
                                // pressed, we need to reset the prediction trigger to the previous word, which we
                                // don't keep. so, for now, we just keep the old predictions around, until the user
                                // goes back to appending words at the end of the sentence
                            }

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
                                    self.line.push(c).expect("IMEF: ran out of space inserting orignial characters into input line");
                                } else {
                                    if debug1{info!("copying char {}", c);}
                                    self.line.push(c).expect("IMEF: ran out of space inserting orignial characters into input line");
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

                // check if the cursor is now at the bottom of the textview, this means we need to grow the box
                if input_tv.cursor.line_height == 0 {
                    if debug1{info!("IMEF: caught case of overflowed text box, attempting to resize");}
                    let delta = if self.last_height > 0 {
                        self.last_height + 1 + 1 // 1 pixel allowance for interline space, plus 1 for fencepost
                    } else {
                        31 // a default value to grow in case we don't have a valid last height
                    };
                    let mut req = SetCanvasBoundsRequest {
                        canvas: ic,
                        requested: Point::new(0, ic_bounds.y + delta as i16),
                        granted: None,
                    };
                    if debug1{info!("IMEF: attempting resize to {:?}", req.requested);}
                    gam::set_canvas_bounds_request(self.gam_conn, &mut req).expect("IMEF: couldn't call set_bounds_request on input area overflow");
                    self.clear_area().expect("IMEF: couldn't clear area after resize");
                    match req.granted {
                        Some(bounds) => {
                            self.was_grown = true;
                            if debug1{info!("IMEF: refresh succeeded, now redrawing");}
                            // request was approved, redraw with the new bounding box
                            input_tv.bounds_hint = TextBounds::BoundingBox(Rectangle::new(Point::new(0,1), ic_bounds));
                            input_tv.bounds_computed = None;
                            gam::post_textview(self.gam_conn, &mut input_tv).expect("IMEF: can't draw input TextView");
                        },
                        _ => info!("IMEF: couldn't resize input canvas after overflow of text")
                    }
                } else {
                    self.last_height = input_tv.cursor.line_height as u32;
                }
            }
        }
        /*
        what else do we need:
        - relay hints to the prediction engine, and draw prediction results
        - extend blitstr to add ellipsis '…" when the clipping rectangle overflows
        - registration for listening to string results
        */
        // prediction area is drawn second because the area could be cleared on behalf of a resize of the text box
        // just draw a rectangle for the prediction area for now
        if let Some(pc) = self.pred_canvas {
            if debug1{info!("IMEF: updating prediction area");}
            let pc_bounds: Point = gam::get_canvas_bounds(self.gam_conn, pc).expect("IMEF: Couldn't get prediction canvas bounds");
            let pc_clip: Rectangle = Rectangle::new_with_style(Point::new(0,1), pc_bounds,
                DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 }
            );
            if debug1{info!("IMEF: got pc_bound {:?}", pc_bounds);}

            // count the number of valid options
            let mut valid_predictions = 0;
            for p in self.pred_options.iter() {
                if p.is_some() {
                    valid_predictions += 1;
                }
            }

            if valid_predictions == 0 {
                let mut empty_tv = TextView::new(pc, 255,
                    TextBounds::BoundingBox(Rectangle::new(Point::new(0, 1), pc_bounds)));
                empty_tv.draw_border = false;
                empty_tv.border_width = 1;
                empty_tv.clear_area = true;
                write!(empty_tv.text, "Ready for input...").expect("IMEF: couldn't set up empty TextView");
                gam::post_textview(self.gam_conn, &mut empty_tv).expect("IMEF: can't draw prediction TextView");
            } else if update_predictor  {

                // TODO: insert the routine to query/update the predictions set here
                // for now, without this, we'll just get the empty prediction set, always

                // alright, first, let's clear the area
                gam::draw_rectangle(self.gam_conn, pc, pc_clip).expect("IMEF: couldn't clear predictor area");

                // OK, let's start initially with just a naive, split-by-N layout of the prediction area
                let approx_width = pc_bounds.y / valid_predictions as i16;

                let mut i = 0;
                for p in self.pred_options.iter() {
                    if let Some(pred_str) = p {
                        // the post-clip is necessary because the approx_width is rounded to some integer fraction
                        let p_clip = Rectangle::new(
                            Point::new(i * approx_width, 1),
                            Point::new((i+1) * approx_width, pc_bounds.y)).clip_with(pc_clip).unwrap();
                        if i > 0 {
                            gam::draw_line(self.gam_conn, pc,
                            Line::new_with_style(
                            Point::new(i * approx_width, 1),
                            Point::new( i * approx_width, pc_bounds.y),
                            DrawStyle { fill_color: None, stroke_color: Some(PixelColor::Dark), stroke_width: 1 }
                            )).expect("IMEF: couldn't draw dividing lines in prediction area");
                        }
                        let mut p_tv = TextView::new(pc, 255,
                            TextBounds::BoundingBox(p_clip));
                        p_tv.draw_border = false;
                        p_tv.border_width = 1;
                        p_tv.clear_area = false;
                        p_tv.style = blitstr::GlyphStyle::Small;
                        write!(p_tv.text, "{}", pred_str).expect("IMEF: can't write the prediction string");
                        gam::post_textview(self.gam_conn, &mut p_tv).expect("IMEF: couldn't post prediction text");
                        i += 1;
                    }
                }
            }
        }

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
    tracker.update(['\u{0000}'; 4]).expect("IMEF: can't setup initial screen arrangement");

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
