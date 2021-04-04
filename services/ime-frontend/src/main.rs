#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]


use gam::api::SetCanvasBoundsRequest;
use ime_plugin_api::{ImefOpcode, ImefCallback};

use log::{error, info};

use graphics_server::{Gid, Line, PixelColor, Point, Rectangle, TextBounds, TextView, DrawStyle};
use blitstr_ref as blitstr;
use ime_plugin_api::{PredictionTriggers, PredictionPlugin, PredictionApi};

use num_traits::{ToPrimitive,FromPrimitive};
use xous_ipc::{String, Buffer};
use xous::{CID, msg_scalar_unpack};

use core::fmt::Write;

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
    pred_phrase: String::<4000>, // note: untested as of Mar 7 2021
    /// character position of the last prediction trigger -- this is where the prediction overwrite starts
    /// if None, it means we were unable to determine the trigger (e.g., we went back and edited text manually)
    last_trigger_char: Option<usize>,

    /// track the progress of our input line
    line: String::<4000>,
    /// length of the line in *characters*, not bytes (which is what .len() returns), used to index char_locs
    characters: usize,
    /// the insertion point, 0 is inserting characters before the first, 1 inserts characters after the first, etc.
    insertion: usize,
    /// last returned line height, which is used as a reference for growing the area when we run out of space
    last_height: u32,
    /// keep track if our box was grown
    was_grown: bool,

    /// render the predictions
    pred_options: [Option<String::<4000>>; MAX_PREDICTION_OPTIONS],
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
            pred_phrase: String::<4000>::new(),
            last_trigger_char: Some(0),
            line: String::<4000>::new(),
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
        .expect("InputTracker failed to get prediction triggers from plugin"));
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
            let pc_bounds: Point = gam::get_canvas_bounds(self.gam_conn, pc).expect("Couldn't get prediction canvas bounds");
            gam::draw_rectangle(self.gam_conn, pc,
                Rectangle::new_with_style(Point::new(0, 0), pc_bounds,
                DrawStyle {
                    fill_color: Some(PixelColor::Light),
                    stroke_color: None,
                    stroke_width: 0
                }
            )).expect("can't clear prediction area");
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
            ).expect("can't draw prediction top border");
        }

        if let Some(ic) = self.input_canvas {
            let ic_bounds: Point = gam::get_canvas_bounds(self.gam_conn, ic).expect("Couldn't get input canvas bounds");
            gam::draw_rectangle(self.gam_conn, ic,
                Rectangle::new_with_style(Point::new(0, 0), ic_bounds,
                DrawStyle {
                    fill_color: Some(PixelColor::Light),
                    stroke_color: None,
                    stroke_width: 0
                }
            )).expect("can't clear input area");

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
                    .expect("can't draw input top line border");
        }

        Ok(())
    }

    fn insert_prediction(&mut self, index: usize) {
        let debug1 = false;
        if debug1{info!("IMEF|insert_prediction index {}", index);}
        let pred_str = match self.pred_options[index] {
            Some(s) => s,
            _ => return // if the index doesn't exist for some reason, do nothing without throwing an error
        };
        if debug1{info!("IMEF|insert_prediction string {}, last_trigger {:?}", pred_str, self.last_trigger_char);}
        if let Some(offset) = self.last_trigger_char {
            if offset < self.characters {
                // copy the bytes in the original string, up to the offset; and then copy the bytes in the selected predictor
                let tempbytes: [u8; 4000] = self.line.as_bytes();
                let tempstr = unsafe { core::str::from_utf8_unchecked(&tempbytes[0..self.line.len()]) }.clone();
                self.line.clear();
                let mut chars = 0;
                let mut c_iter = tempstr.chars();
                loop {
                    if chars == offset + 1 { // +1 to include the original trigger (don't overwrite it)
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
                    self.line.push(c).expect("ran out of space inserting prediction");
                    chars += 1;
                }
                // forward until we find the next prediction trigger in the original string
                while let Some(c) = c_iter.next() {
                    if let Some(trigger) = self.pred_triggers {
                        if trigger.whitespace && c.is_ascii_whitespace() ||
                           trigger.punctuation && c.is_ascii_punctuation() {
                               // include the trigger that was found
                               self.line.push(c).expect("ran out of space inserting prediction");
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
                    self.line.push(c).expect("ran out of space inserting prediction");
                    chars += 1;
                }
                self.characters = chars;
            } else {
                // just append the prediction to the line
                for c in pred_str.as_str().unwrap().chars() {
                    self.line.push(c).expect("ran out of space inserting prediction");
                    self.characters += 1;
                }
                self.last_trigger_char = Some(self.insertion);
                self.insertion = self.characters;
            }
        }
    }

    pub fn update(&mut self, newkeys: [char; 4]) -> Result<Option<String::<4000>>, xous::Error> {
        let debug1= false;
        let mut update_predictor = false;
        let mut retstring: Option<String::<4000>> = None;
        if let Some(ic) = self.input_canvas {
            if debug1{info!("updating input area");}
            let ic_bounds: Point = gam::get_canvas_bounds(self.gam_conn, ic).expect("Couldn't get input canvas bounds");
            let mut input_tv = TextView::new(ic,
                TextBounds::BoundingBox(Rectangle::new(Point::new(0,1), ic_bounds)));
            input_tv.draw_border = false;
            input_tv.border_width = 1;
            input_tv.clear_area = true; // need this so that the insertion point is cleared and moved

            let mut do_redraw = false;
            for &k in newkeys.iter() {
                if debug1{info!("got key '{}'", k);}
                match k {
                    '\u{0000}' => (),
                    '←' => { // move insertion point back
                        if self.insertion > 0 {
                            info!("moving insertion point back");
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
                            if debug1{info!("simple backspace case")}
                            self.line.pop();
                            self.characters -= 1;
                            self.insertion -= 1;
                            do_redraw = true;

                            if let Some(predictor) = self.predictor {
                                if self.can_unpick {
                                    predictor.unpick().expect("couldn't unpick last prediction");
                                    self.can_unpick = false;
                                    update_predictor = true;
                                }
                                self.pred_phrase.clear();
                            }
                        } else if (self.characters > 0)  && (self.insertion > 0) {
                            if debug1{info!("mid-string backspace case")}
                            // awful O(N) algo because we have to decode variable-length utf8 strings to figure out character boundaries
                            // first, make a copy of the string
                            let tempbytes: [u8; 4000] = self.line.as_bytes();
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
                                    self.line.push(c).expect("ran out of space inserting orignial characters into input line");
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
                        let mut ret = String::<4000>::new();
                        write!(ret, "{}", self.line.as_str().expect("couldn't convert input line")).expect("couldn't copy input ilne to output");
                        retstring = Some(ret);

                        if let Some(trigger) = self.pred_triggers {
                            if trigger.newline {
                                self.predictor.unwrap().feedback_picked(self.line).expect("couldn't send feedback to predictor");
                            } else if trigger.punctuation {
                                self.predictor.unwrap().feedback_picked(self.pred_phrase).expect("couldn't send feedback to predictor");
                            }
                        }
                        self.can_unpick = false;
                        self.pred_phrase.clear();

                        if debug1{info!("got carriage return");}
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
                            if debug1{info!("attempting resize to {:?}", req.requested);}
                            gam::set_canvas_bounds_request(self.gam_conn, &mut req).expect("couldn't call set_bounds_request on input area overflow");
                            if debug1{
                                info!("carriage return resize to {:?}", req.granted);
                            }
                            self.last_height = 0;
                            self.was_grown = false;
                        }
                        self.clear_area().expect("can't clear on carriage return");
                        update_predictor = true;
                    },
                    _ => {
                        if let Some(trigger) = self.pred_triggers {
                            if trigger.whitespace && k.is_ascii_whitespace() {
                                if self.pred_phrase.len() > 0 {
                                    self.predictor.unwrap().feedback_picked(self.pred_phrase).expect("couldn't send feedback to predictor");
                                    self.pred_phrase.clear();
                                    self.can_unpick = true;
                                    update_predictor = true;
                                }
                                self.last_trigger_char = Some(self.insertion);
                            }
                            if trigger.punctuation && k.is_ascii_punctuation() {
                                if self.pred_phrase.len() > 0 {
                                    self.predictor.unwrap().feedback_picked(self.pred_phrase).expect("couldn't send feedback to predictor");
                                    self.pred_phrase.clear();
                                    self.can_unpick = true;
                                    update_predictor = true;
                                }
                                self.last_trigger_char = Some(self.insertion);
                            }
                        }
                        if self.insertion == self.characters {
                            self.line.push(k).expect("ran out of space pushing character into input line");
                            if let Some(trigger) = self.pred_triggers {
                                if !(trigger.punctuation && k.is_ascii_punctuation() ||
                                    trigger.whitespace  && k.is_ascii_whitespace() ) {
                                    self.pred_phrase.push(k).expect("ran out of space pushing character into prediction phrase");
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

                            if debug1{info!("handling case of inserting characters. insertion: {}", self.insertion)};
                            // awful O(N) algo because we have to decode variable-length utf8 strings to figure out character boundaries
                            // first, make a copy of the string
                            let tempbytes: [u8; 4000] = self.line.as_bytes();
                            let tempstr = unsafe { core::str::from_utf8_unchecked(&tempbytes[0..self.line.len()]) }.clone();
                            // clear the string
                            self.line.clear();

                            // insert the character in the array
                            let mut i = 0;
                            for c in tempstr.chars() {
                                if debug1{info!("checking index {}", i);}
                                if i == self.insertion {
                                    if debug1{info!("inserting char {}", k);}
                                    self.line.push(k).expect("ran out of space inserting new character into input line");
                                    self.line.push(c).expect("ran out of space inserting orignial characters into input line");
                                } else {
                                    if debug1{info!("copying char {}", c);}
                                    self.line.push(c).expect("ran out of space inserting orignial characters into input line");
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

            input_tv.insertion = Some(self.insertion as _);
            if debug1{info!("insertion point is {}, characters in string {}", self.insertion, self.characters);}
            if do_redraw {
                write!(input_tv.text, "{}", self.line.as_str().expect("couldn't convert str")).expect("couldn't update TextView string in input canvas");
                gam::post_textview(self.gam_conn, &mut input_tv).expect("can't draw input TextView");
                if debug1{info!("got computed cursor of {:?}", input_tv.cursor);}

                // check if the cursor is now at the bottom of the textview, this means we need to grow the box
                if input_tv.cursor.line_height == 0 && self.characters > 0 {
                    if debug1{info!("caught case of overflowed text box, attempting to resize");}
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
                    if debug1{info!("attempting resize to {:?}", req.requested);}
                    gam::set_canvas_bounds_request(self.gam_conn, &mut req).expect("couldn't call set_bounds_request on input area overflow");
                    self.clear_area().expect("couldn't clear area after resize");
                    match req.granted {
                        Some(bounds) => {
                            self.was_grown = true;
                            if debug1{info!("refresh succeeded, now redrawing with height of {:?}", bounds);}
                            // request was approved, redraw with the new bounding box
                            input_tv.bounds_hint = TextBounds::BoundingBox(Rectangle::new(Point::new(0,1), bounds));
                            input_tv.bounds_computed = None;
                            gam::post_textview(self.gam_conn, &mut input_tv).expect("can't draw input TextView");
                        },
                        _ => info!("couldn't resize input canvas after overflow of text")
                    }
                } else {
                    self.last_height = input_tv.cursor.line_height as u32;
                }
            }
        }

        // prediction area is drawn second because the area could be cleared on behalf of a resize of the text box
        // just draw a rectangle for the prediction area for now
        if let Some(pc) = self.pred_canvas {
            if debug1{info!("updating prediction area");}
            let pc_bounds: Point = gam::get_canvas_bounds(self.gam_conn, pc).expect("Couldn't get prediction canvas bounds");
            let pc_clip: Rectangle = Rectangle::new_with_style(Point::new(0,1), pc_bounds,
                DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 }
            );
            if debug1{info!("got pc_bound {:?}", pc_bounds);}

            if update_predictor {
                if self.pred_phrase.len() > 0 {
                    if let Some(pred) = self.predictor {
                        pred.set_input(self.pred_phrase).expect("couldn't update predictor with current input");
                    }
                }

                // Query the prediction engine for the latest predictions
                if let Some(pred) = self.predictor {
                    for i in 0..self.pred_options.len() {
                        self.pred_options[i] = pred.get_prediction(i as u32).expect("couldn't query prediction engine");
                    }
                }
            }

            // count the number of valid options
            let mut valid_predictions = 0;
            for p in self.pred_options.iter() {
                if p.is_some() {
                    valid_predictions += 1;
                }
            }

            let debug_canvas = false;
            if valid_predictions == 0 {
                let mut empty_tv = TextView::new(pc,
                    TextBounds::BoundingBox(Rectangle::new(Point::new(0, 1), pc_bounds)));
                empty_tv.draw_border = false;
                empty_tv.border_width = 1;
                empty_tv.clear_area = true;
                write!(empty_tv.text, "Ready for input...").expect("couldn't set up empty TextView");
                if debug_canvas { info!("pc canvas {:?}", pc) }
                gam::post_textview(self.gam_conn, &mut empty_tv).expect("can't draw prediction TextView");
            } else if update_predictor  {
                // alright, first, let's clear the area
                gam::draw_rectangle(self.gam_conn, pc, pc_clip).expect("couldn't clear predictor area");

                if debug1{info!("valid_predictions: {}", valid_predictions);}
                // OK, let's start initially with just a naive, split-by-N layout of the prediction area
                let approx_width = pc_bounds.x / valid_predictions as i16;

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
                            )).expect("couldn't draw dividing lines in prediction area");
                        }
                        let mut p_tv = TextView::new(pc,
                            TextBounds::BoundingBox(p_clip));
                        p_tv.draw_border = false;
                        p_tv.border_width = 1;
                        p_tv.clear_area = false;
                        p_tv.ellipsis = true;
                        p_tv.style = blitstr::GlyphStyle::Small;
                        write!(p_tv.text, "{}", pred_str).expect("can't write the prediction string");
                        gam::post_textview(self.gam_conn, &mut p_tv).expect("couldn't post prediction text");
                        i += 1;
                    }
                }
            }
        }

        gam::redraw(self.gam_conn).expect("couldn't redraw screen");

        Ok(retstring)
    }
}

// we have to store this connection state somewhere, either in the lib side or the local side
static mut CB_TO_MAIN_CONN: Option<CID> = None;
fn handle_keyevents(keys: [char; 4]) {
    if let Some(cb_to_main_conn) = unsafe{CB_TO_MAIN_CONN} {
        xous::send_message(cb_to_main_conn,
            xous::Message::new_scalar(ImefOpcode::ProcessKeys.to_usize().unwrap(),
            keys[0] as u32 as usize,
            keys[1] as u32 as usize,
            keys[2] as u32 as usize,
            keys[3] as u32 as usize,
        )).unwrap();
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = false;
    let dbglistener = false;
    let dbgcanvas = false;
    log_server::init_wait().unwrap();
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let imef_sid = xns.register_name(ime_plugin_api::SERVER_NAME_IME_FRONT).expect("can't register server");
    log::trace!("registered with NS -- {:?}", imef_sid);

    // hook the keyboard event server and have it forward keys to our local main loop
    unsafe{CB_TO_MAIN_CONN = Some(xous::connect(imef_sid).unwrap())};
    let mut kbd = keyboard::Keyboard::new(&xns).expect("can't connect to KBD");
    kbd.hook_keyboard_events(handle_keyevents).unwrap();

    let mut tracker = InputTracker::new(
        xns.request_connection_blocking(xous::names::SERVER_NAME_GAM).expect("can't connect to GAM"),
    );

    let mut listeners: [Option<CID>; 32] = [None; 32];

    // The main lop can't start until we've been assigned Gids from the GAM, and a Predictor.
    info!("waiting for my canvas Gids");
    let mut init_done = false;
    loop {
        let msg = xous::receive_message(imef_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ImefOpcode::SetInputCanvas) => {
                msg_scalar_unpack!(msg, g0, g1, g2, g3, {
                    let g = Gid::new([g0 as _, g1 as _, g2 as _, g3 as _]);
                    if debug1 || dbgcanvas {info!("got input canvas {:?}", g);}
                    tracker.set_input_canvas(g);
                });
            }
            Some(ImefOpcode::SetPredictionCanvas) => {
                msg_scalar_unpack!(msg, g0, g1, g2, g3, {
                    let g = Gid::new([g0 as _, g1 as _, g2 as _, g3 as _]);
                    if debug1 || dbgcanvas {info!("got prediction canvas {:?}", g);}
                    tracker.set_pred_canvas(g);
                });
            }
            Some(ImefOpcode::SetPredictionServer) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<String::<64>, _>().unwrap();
                match xns.request_connection(s.as_str()) {
                    Ok(pc) => tracker.set_predictor(ime_plugin_api::PredictionPlugin {connection: Some(pc)}),
                    _ => error!("can't find predictive engine {}, retaining existing one.", s.as_str()),
                }
            }
            Some(ImefOpcode::RegisterListener) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = Some(xous::connect(sid).unwrap());
                let mut found = false;
                for entry in listeners.iter_mut() {
                    if *entry == None {
                        *entry = cid;
                        found = true;
                        break;
                    }
                }
                if !found {
                    error!("RegisterCallback listener ran out of space registering callback");
                }
            }),
            Some(ImefOpcode::ProcessKeys) => {
                if init_done {
                    msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                        let keys = [
                            if let Some(a) = core::char::from_u32(k1 as u32) {
                                a
                            } else {
                                '\u{0000}'
                            },
                            if let Some(a) = core::char::from_u32(k2 as u32) {
                                a
                            } else {
                                '\u{0000}'
                            },
                            if let Some(a) = core::char::from_u32(k3 as u32) {
                                a
                            } else {
                                '\u{0000}'
                            },
                            if let Some(a) = core::char::from_u32(k4 as u32) {
                                a
                            } else {
                                '\u{0000}'
                            },
                        ];
                        if let Some(line) = tracker.update(keys).expect("couldn't update input tracker with latest key presses") {
                            if dbglistener{info!("sending listeners {:?}", line);}
                            let buf = Buffer::into_buf(line).or(Err(xous::Error::InternalError)).unwrap();

                            for maybe_conn in listeners.iter_mut() {
                                if let Some(conn) = maybe_conn {
                                    if dbglistener{info!("sending to conn {:?}", conn);}
                                    match buf.lend(*conn, ImefCallback::GotInputLine.to_u32().unwrap()) {
                                        Err(xous::Error::ServerNotFound) => {
                                            *maybe_conn = None // automatically de-allocate callbacks for clients that have dropped
                                        },
                                        Ok(xous::Result::Ok) => {}
                                        _ => panic!("unhandled error or result in callback processing")
                                    }
                                }
                            }
                        }
                    });
                } else {
                    // ignore keyboard events until we've fully initialized
                }
            }
            None => log::error!("couldn't convert opcode")
        }
        if !init_done && tracker.is_init() {
            init_done = true;
            // force a redraw of the UI
            if debug1{info!("forcing initial UI redraw");}
            tracker.clear_area().expect("can't initially clear areas");
            tracker.update(['\u{0000}'; 4]).expect("can't setup initial screen arrangement");
        }
    }
}
