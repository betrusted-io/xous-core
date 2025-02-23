#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod emoji;
use core::fmt::Write;

use emoji::*;
use gam::api::SetCanvasBoundsRequest;
use ime_plugin_api::{ApiToken, PredictionApi, PredictionPlugin, PredictionTriggers};
use ime_plugin_api::{ImefCallback, ImefDescriptor, ImefOpcode};
use locales::t;
use log::{error, info};
use num_traits::{FromPrimitive, ToPrimitive};
#[cfg(feature = "tts")]
use tts_frontend::*;
use ux_api::minigfx::*;
use ux_api::service::api::*;
use xous::{CID, msg_scalar_unpack};
use xous_ipc::Buffer;

/// max number of prediction options to track/render
const MAX_PREDICTION_OPTIONS: usize = 4;

struct InputTracker {
    /// connection for handling graphical update requests
    pub gam: gam::Gam,

    /// input area canvas, as given by the GAM
    input_canvas: Option<Gid>,
    /// prediction display area, as given by the GAM
    pred_canvas: Option<Gid>,
    /// gam token, so the GAM can find the application that requested us
    gam_token: Option<[u32; 4]>,

    /// our current prediction engine
    predictor: Option<PredictionPlugin>,
    /// the name & token of our engine, so we can disconnect later on
    pub predictor_conn: Option<(String, [u32; 4])>,
    /// cached copy of the predictor's triggers for predictions. Only valid if predictor is not None
    pred_triggers: Option<PredictionTriggers>,
    /// set if we're in a state where a backspace should trigger an unpredict
    can_unpick: bool, // note: untested as of Mar 7 2021
    /// the predictor string -- this is different from the input line, because it can be broken up by spaces
    /// and punctuatino
    pred_phrase: String, // note: untested as of Mar 7 2021
    /// character position of the last prediction trigger -- this is where the prediction overwrite starts
    /// if None, it means we were unable to determine the trigger (e.g., we went back and edited text
    /// manually)
    last_trigger_char: Option<usize>,

    /// track the progress of our input line
    line: String,
    /// length of the line in *characters*, not bytes (which is what .len() returns), used to index char_locs
    characters: usize,
    /// the insertion point, 0 is inserting characters before the first, 1 inserts characters after the
    /// first, etc.
    insertion: usize,
    /// last returned line height, which is used as a reference for growing the area when we run out of space
    last_height: u32,
    /// keep track if our box was grown
    was_grown: bool,

    /// if set to true, the F1-F4 keys work as menu selects, and not as predictive inputs
    menu_mode: bool,

    /// render the predictions. Slightly awkward because this code comes from before we had libstd
    pred_options: [Option<String>; MAX_PREDICTION_OPTIONS],
    #[cfg(feature = "tts")]
    tts: TtsFrontend,
}

impl InputTracker {
    pub fn new(xns: &xous_names::XousNames) -> InputTracker {
        InputTracker {
            gam: gam::Gam::new(&xns).unwrap(),
            input_canvas: None,
            pred_canvas: None,
            predictor: None,
            predictor_conn: None,
            pred_triggers: None,
            gam_token: None,
            can_unpick: false,
            pred_phrase: String::new(),
            last_trigger_char: Some(0),
            line: String::new(),
            characters: 0,
            insertion: 0,
            last_height: 0,
            was_grown: false,
            pred_options: Default::default(),
            menu_mode: false,
            #[cfg(feature = "tts")]
            tts: TtsFrontend::new(xns).unwrap(),
        }
    }

    pub fn set_gam_token(&mut self, token: [u32; 4]) { self.gam_token = Some(token); }

    /// this is a separate, non-blocking call instead of a return because
    /// the call which sets the predictor *must* complete to allow further drawing
    /// this does mean there is a tiny bit of a race condition between when
    /// a context is swapped and when a predictor can run.
    pub fn send_api_token(&self, at: &ApiToken) {
        self.gam
            .set_predictor_api_token(at.api_token, at.gam_token)
            .expect("couldn't set predictor API token");
    }

    pub fn set_predictor(&mut self, predictor: Option<PredictionPlugin>) {
        self.predictor = predictor;
        if let Some(pred) = predictor {
            self.pred_triggers = Some(
                pred.get_prediction_triggers()
                    .expect("InputTracker failed to get prediction triggers from plugin"),
            );
        }
    }

    pub fn get_predictor(&self) -> Option<PredictionPlugin> { self.predictor }

    pub fn set_input_canvas(&mut self, input: Gid) { self.input_canvas = Some(input); }

    pub fn clear_input_canvas(&mut self) { self.input_canvas = None }

    pub fn set_pred_canvas(&mut self, pred: Gid) { self.pred_canvas = Some(pred); }

    pub fn clear_pred_canvas(&mut self) { self.pred_canvas = None }

    pub fn is_init(&self) -> bool {
        self.input_canvas.is_some() && self.pred_canvas.is_some() && self.predictor.is_some()
    }

    pub fn activate_emoji(&self) {
        self.gam.raise_menu(gam::EMOJI_MENU_NAME).expect("couldn't activate emoji menu");
    }

    pub fn set_menu_mode(&mut self, mode: bool) { self.menu_mode = mode; }

    pub fn clear_area(&mut self) -> Result<(), xous::Error> {
        if let Some(pc) = self.pred_canvas {
            let pc_bounds: Point =
                self.gam.get_canvas_bounds(pc).expect("Couldn't get prediction canvas bounds");
            self.gam
                .draw_rectangle(
                    pc,
                    Rectangle::new_with_style(
                        Point::new(0, 0),
                        pc_bounds,
                        DrawStyle {
                            fill_color: Some(PixelColor::Light),
                            stroke_color: None,
                            stroke_width: 0,
                        },
                    ),
                )
                .expect("can't clear prediction area");
            // add the border line on top
            self.gam
                .draw_line(
                    pc,
                    Line::new_with_style(
                        Point::new(0, 0),
                        Point::new(pc_bounds.x, 0),
                        DrawStyle { fill_color: None, stroke_color: Some(PixelColor::Dark), stroke_width: 1 },
                    ),
                )
                .expect("can't draw prediction top border");
        }

        if let Some(ic) = self.input_canvas {
            let ic_bounds: Point = self.gam.get_canvas_bounds(ic).expect("Couldn't get input canvas bounds");
            self.gam
                .draw_rectangle(
                    ic,
                    Rectangle::new_with_style(
                        Point::new(0, 0),
                        ic_bounds,
                        DrawStyle {
                            fill_color: Some(PixelColor::Light),
                            stroke_color: None,
                            stroke_width: 0,
                        },
                    ),
                )
                .expect("can't clear input area");

            // add the border line on top
            self.gam
                .draw_line(
                    ic,
                    Line::new_with_style(
                        Point::new(0, 0),
                        Point::new(ic_bounds.x, 0),
                        DrawStyle { fill_color: None, stroke_color: Some(PixelColor::Dark), stroke_width: 1 },
                    ),
                )
                .expect("can't draw input top line border");
        }

        Ok(())
    }

    fn insert_prediction(&mut self, index: usize) {
        let debug1 = false;
        if debug1 {
            info!("IMEF|insert_prediction index {}", index);
        }
        let pred_str = match &self.pred_options[index] {
            Some(s) => s,
            _ => return, // if the index doesn't exist for some reason, do nothing without throwing an error
        };
        if debug1 {
            info!("IMEF|insert_prediction string {}, last_trigger {:?}", pred_str, self.last_trigger_char);
        }
        if let Some(offset) = self.last_trigger_char {
            if offset < self.characters {
                // copy the bytes in the original string, up to the offset; and then copy the bytes in the
                // selected predictor
                let tempstr = self.line.to_string();
                self.line.clear();
                let mut chars = 0;
                let mut c_iter = tempstr.chars();
                loop {
                    if chars == offset + 1 {
                        // +1 to include the original trigger (don't overwrite it)
                        break;
                    }
                    if let Some(c) = c_iter.next() {
                        self.line.push(c);
                        chars += 1;
                    } else {
                        break;
                    }
                }
                // now push the characters in the predicted options onto the line
                for c in pred_str.as_str().chars() {
                    self.line.push(c);
                    chars += 1;
                }
                // forward until we find the next prediction trigger in the original string
                while let Some(c) = c_iter.next() {
                    if let Some(trigger) = self.pred_triggers {
                        if trigger.whitespace && c.is_ascii_whitespace()
                            || trigger.punctuation && c.is_ascii_punctuation()
                        {
                            // include the trigger that was found
                            self.line.push(c);
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
                    self.line.push(c);
                    chars += 1;
                }
                self.characters = chars;
            } else {
                // just append the prediction to the line
                for c in pred_str.as_str().chars() {
                    self.line.push(c);
                    self.characters += 1;
                }
                self.last_trigger_char = Some(self.insertion);
                self.insertion = self.characters;
            }
        }
    }

    pub fn update(
        &mut self,
        newkeys: [char; 4],
        force_redraw: bool,
        api_token: [u32; 4],
    ) -> Result<Option<String>, xous::Error> {
        let debug1 = false;
        let mut update_predictor = force_redraw;
        let mut retstring: Option<String> = None;
        if let Some(ic) = self.input_canvas {
            if debug1 {
                info!("updating input area");
            }
            let ic_bounds: Point = self.gam.get_canvas_bounds(ic).expect("Couldn't get input canvas bounds");
            let mut input_tv =
                TextView::new(ic, TextBounds::BoundingBox(Rectangle::new(Point::new(0, 1), ic_bounds)));
            input_tv.draw_border = false;
            input_tv.border_width = 1;
            input_tv.clear_area = true; // need this so that the insertion point is cleared and moved
            input_tv.style = gam::SYSTEM_STYLE;

            let mut do_redraw = false;
            for &k in newkeys.iter() {
                if debug1 {
                    info!("got key '{}'", k);
                }
                match k {
                    '\u{0000}' => (),
                    '←' => {
                        // move insertion point back
                        if !self.menu_mode {
                            if self.insertion > 0 {
                                log::debug!("moving insertion point back");
                                self.insertion -= 1;
                            }
                            do_redraw = true;
                            self.pred_phrase.clear(); // don't track predictions on edits
                            self.can_unpick = false;
                            self.last_trigger_char = None;
                        } else {
                            return Ok(Some(String::from("←")));
                        }
                    }
                    '→' => {
                        if !self.menu_mode {
                            if self.insertion < self.characters {
                                self.insertion += 1;
                            }
                            do_redraw = true;
                            self.pred_phrase.clear();
                            self.can_unpick = false;
                            self.last_trigger_char = None;
                        } else {
                            return Ok(Some(String::from("→")));
                        }
                    }
                    '↑' => {
                        if !self.menu_mode {
                            // bring the insertion point to the front of the text box
                            self.insertion = 0;
                            do_redraw = true;
                            self.pred_phrase.clear();
                            self.can_unpick = false;
                            self.last_trigger_char = None;
                        } else {
                            return Ok(Some(String::from("↑")));
                        }
                    }
                    '↓' => {
                        if !self.menu_mode {
                            // bring insertion point to the very end of the text box
                            self.insertion = self.characters;
                            do_redraw = true;
                            self.pred_phrase.clear();
                            self.can_unpick = false;
                            // this means that when we resume typing after an edit, the predictor will set its
                            // insertion point at the very end, not the space
                            // prior to the last word...
                            self.last_trigger_char = Some(self.characters);
                        } else {
                            return Ok(Some(String::from("↓")));
                        }
                    }
                    '\u{0011}' => {
                        // F1
                        if !self.menu_mode {
                            self.insert_prediction(0);
                            do_redraw = true;
                        } else {
                            retstring = Some(String::from("\u{0011}"));
                            do_redraw = true;
                        }
                    }
                    '\u{0012}' => {
                        // F2
                        if !self.menu_mode {
                            self.insert_prediction(1);
                            do_redraw = true;
                        } else {
                            retstring = Some(String::from("\u{0012}"));
                            do_redraw = true;
                        }
                    }
                    '\u{0013}' => {
                        // F3
                        if !self.menu_mode {
                            self.insert_prediction(2);
                            do_redraw = true;
                        } else {
                            retstring = Some(String::from("\u{0013}"));
                            do_redraw = true;
                        }
                    }
                    '\u{0014}' => {
                        // F4
                        if !self.menu_mode {
                            self.insert_prediction(3);
                            do_redraw = true;
                        } else {
                            retstring = Some(String::from("\u{0014}"));
                            do_redraw = true;
                        }
                    }
                    '\u{0008}' => {
                        // backspace
                        #[cfg(feature = "tts")]
                        self.tts.tts_simple(t!("input.delete-tts", locales::LANG)).unwrap();
                        if (self.characters > 0) && (self.insertion == self.characters) {
                            if debug1 {
                                info!("simple backspace case")
                            }
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
                                self.pred_phrase.pop();
                                if self.menu_mode {
                                    update_predictor = true;
                                }
                            }
                        } else if (self.characters > 0) && (self.insertion > 0) {
                            if debug1 {
                                info!("mid-string backspace case")
                            }
                            // awful O(N) algo because we have to decode variable-length utf8 strings to
                            // figure out character boundaries first, make a copy
                            // of the string
                            let tempstr = self.line.to_string();
                            // clear the string
                            self.line.clear();

                            // delete the character in the array
                            let mut i = 0; // character position
                            let mut dest_chars = 0; // destination character position
                            for c in tempstr.chars() {
                                if debug1 {
                                    info!("checking index {}", i);
                                }
                                if i == self.insertion - 1 {
                                    if debug1 {
                                        info!("skipping char");
                                    }
                                } else {
                                    dest_chars += 1;
                                    // if we encounter a trigger character, set the trigger to just after this
                                    // point (hence the += before this line)
                                    if let Some(trigger) = self.pred_triggers {
                                        if trigger.punctuation && c.is_ascii_punctuation() {
                                            self.last_trigger_char = Some(dest_chars);
                                        } else if trigger.whitespace && c.is_ascii_whitespace() {
                                            self.last_trigger_char = Some(dest_chars);
                                        }
                                    }
                                    if debug1 {
                                        info!("copying char {}", c);
                                    }
                                    self.line.push(c);
                                }
                                i += 1;
                            }
                            self.insertion -= 1;
                            self.characters -= 1;
                            do_redraw = true;
                            if self.menu_mode {
                                update_predictor = true;
                            }
                        } else {
                            // ignore, we are either at the front of the string, or the string had no
                            // characters
                        }
                    }
                    '\u{000d}' => {
                        // carriage return
                        let mut ret = String::new();
                        write!(ret, "{}", self.line.as_str()).expect("couldn't copy input line to output");
                        retstring = Some(ret);

                        if let Some(trigger) = self.pred_triggers {
                            if trigger.newline {
                                self.predictor
                                    .unwrap()
                                    .feedback_picked(String::from(&self.line))
                                    .expect("couldn't send feedback to predictor");
                            } else if trigger.punctuation {
                                self.predictor
                                    .unwrap()
                                    .feedback_picked(String::from(&self.pred_phrase))
                                    .expect("couldn't send feedback to predictor");
                            }
                        }
                        self.can_unpick = false;
                        self.pred_phrase.clear();

                        if debug1 {
                            info!("got carriage return");
                        }
                        self.line.clear();
                        self.last_trigger_char = Some(0);
                        // clear all the temporary variables
                        self.characters = 0;
                        self.insertion = 0;
                        if self.was_grown {
                            let mut req = SetCanvasBoundsRequest {
                                requested: Point::new(0, 0), /* size 0 will snap to the original smallest
                                                              * default size */
                                granted: None,
                                token_type: gam::TokenType::Gam,
                                token: self.gam_token.unwrap(),
                            };
                            if debug1 {
                                info!("attempting resize to {:?}", req.requested);
                            }
                            self.gam
                                .set_canvas_bounds_request(&mut req)
                                .expect("couldn't call set_bounds_request on input area overflow");
                            if debug1 {
                                info!("carriage return resize to {:?}", req.granted);
                            }
                            self.last_height = 0;
                            self.was_grown = false;
                        }
                        self.clear_area().expect("can't clear on carriage return");
                        update_predictor = true;
                    }
                    _ => {
                        if let Some(trigger) = self.pred_triggers {
                            if trigger.whitespace && k.is_ascii_whitespace() {
                                if self.pred_phrase.len() > 0 {
                                    self.predictor
                                        .unwrap()
                                        .feedback_picked(String::from(&self.pred_phrase))
                                        .expect("couldn't send feedback to predictor");
                                    self.pred_phrase.clear();
                                    self.can_unpick = true;
                                    update_predictor = true;
                                }
                                self.last_trigger_char = Some(self.insertion);
                            } else if trigger.punctuation && k.is_ascii_punctuation() {
                                if self.pred_phrase.len() > 0 {
                                    self.predictor
                                        .unwrap()
                                        .feedback_picked(String::from(&self.pred_phrase))
                                        .expect("couldn't send feedback to predictor");
                                    self.pred_phrase.clear();
                                    self.can_unpick = true;
                                    update_predictor = true;
                                }
                                self.last_trigger_char = Some(self.insertion);
                            }
                        }
                        if self.insertion == self.characters {
                            #[cfg(feature = "tts")]
                            {
                                if !k.is_ascii_whitespace() && !k.is_ascii_punctuation() {
                                    // this is disastisfying in how slow it is
                                    // self.tts.tts_simple(&k.to_string()).unwrap();
                                }
                            }
                            self.line.push(k);
                            if let Some(trigger) = self.pred_triggers {
                                if !(trigger.punctuation && k.is_ascii_punctuation()
                                    || trigger.whitespace && k.is_ascii_whitespace())
                                {
                                    self.pred_phrase.push(k);
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

                                // in order to do predictions on arbitrary words, every time the scroll keys
                                // are pressed, we need to reset the
                                // prediction trigger to the previous word, which we
                                // don't keep. so, for now, we just keep the old predictions around, until the
                                // user goes back to appending words at the
                                // end of the sentence
                            }

                            if debug1 {
                                info!("handling case of inserting characters. insertion: {}", self.insertion)
                            };
                            // awful O(N) algo because we have to decode variable-length utf8 strings to
                            // figure out character boundaries first, make a copy
                            // of the string
                            let tempstr = self.line.to_string();
                            // clear the string
                            self.line.clear();

                            // insert the character in the array
                            let mut i = 0;
                            for c in tempstr.chars() {
                                if debug1 {
                                    info!("checking index {}", i);
                                }
                                if i == self.insertion {
                                    if debug1 {
                                        info!("inserting char {}", k);
                                    }
                                    self.line.push(k);
                                    self.line.push(c);
                                } else {
                                    if debug1 {
                                        info!("copying char {}", c);
                                    }
                                    self.line.push(c);
                                }
                                i += 1;
                            }
                            self.insertion += 1;
                            self.characters += 1;
                            do_redraw = true;
                        }
                    }
                }
            }

            input_tv.insertion = Some(self.insertion as _);
            if debug1 {
                info!("insertion point is {}, characters in string {}", self.insertion, self.characters);
            }
            if do_redraw || force_redraw {
                write!(input_tv.text, "{}", self.line.as_str())
                    .expect("couldn't update TextView string in input canvas");
                self.gam.post_textview(&mut input_tv).expect("can't draw input TextView");
                if debug1 {
                    info!("got computed cursor of {:?}", input_tv.cursor);
                }

                // check if the cursor is now at the bottom of the textview, this means we need to grow the
                // box
                if input_tv.cursor.line_height == 0 && self.characters > 0 {
                    if debug1 {
                        info!("caught case of overflowed text box, attempting to resize");
                    }
                    let delta = if self.last_height > 0 {
                        self.last_height + 1 + 1 // 1 pixel allowance for interline space, plus 1 for fencepost
                    } else {
                        31 // a default value to grow in case we don't have a valid last height
                    };
                    let mut req = SetCanvasBoundsRequest {
                        requested: Point::new(0, ic_bounds.y + delta as isize),
                        granted: None,
                        token_type: gam::TokenType::Gam,
                        token: self.gam_token.unwrap(),
                    };
                    if debug1 {
                        info!("attempting resize to {:?}", req.requested);
                    }
                    self.gam
                        .set_canvas_bounds_request(&mut req)
                        .expect("couldn't call set_bounds_request on input area overflow");
                    self.clear_area().expect("couldn't clear area after resize");
                    match req.granted {
                        Some(bounds) => {
                            self.was_grown = true;
                            if debug1 {
                                info!("refresh succeeded, now redrawing with height of {:?}", bounds);
                            }
                            // request was approved, redraw with the new bounding box
                            input_tv.bounds_hint =
                                TextBounds::BoundingBox(Rectangle::new(Point::new(0, 1), bounds));
                            input_tv.bounds_computed = None;
                            self.gam.post_textview(&mut input_tv).expect("can't draw input TextView");
                        }
                        _ => info!("couldn't resize input canvas after overflow of text"),
                    }
                } else {
                    self.last_height = input_tv.cursor.line_height as u32;
                }
            }
        }

        // prediction area is drawn second because the area could be cleared on behalf of a resize of the text
        // box just draw a rectangle for the prediction area for now
        if let Some(pc) = self.pred_canvas {
            if debug1 {
                info!("updating prediction area");
            }
            let pc_bounds: Point =
                self.gam.get_canvas_bounds(pc).expect("Couldn't get prediction canvas bounds");
            let pc_clip: Rectangle = Rectangle::new_with_style(
                Point::new(0, 1),
                pc_bounds,
                DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 },
            );
            if debug1 {
                info!("got pc_bound {:?}", pc_bounds);
            }

            if update_predictor {
                if self.pred_phrase.len() > 0 || self.menu_mode {
                    if let Some(pred) = self.predictor {
                        pred.set_input(String::from(&self.pred_phrase))
                            .expect("couldn't update predictor with current input");
                    }
                }

                // Query the prediction engine for the latest predictions
                if let Some(pred) = self.predictor {
                    for i in 0..self.pred_options.len() {
                        let p = if let Some(prediction) = pred
                            .get_prediction(i as u32, api_token)
                            .expect("couldn't query prediction engine")
                        {
                            Some(String::from(prediction.as_str()))
                        } else {
                            None
                        };
                        self.pred_options[i] = p;
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
                let mut empty_tv =
                    TextView::new(pc, TextBounds::BoundingBox(Rectangle::new(Point::new(0, 1), pc_bounds)));
                empty_tv.draw_border = false;
                empty_tv.border_width = 1;
                empty_tv.clear_area = true;
                write!(empty_tv.text, "{}", t!("input.greeting", locales::LANG))
                    .expect("couldn't set up empty TextView");
                if debug_canvas {
                    info!("pc canvas {:?}", pc)
                }
                self.gam.post_textview(&mut empty_tv).expect("can't draw prediction TextView");
            } else if update_predictor || force_redraw {
                // alright, first, let's clear the area
                self.gam.draw_rectangle(pc, pc_clip).expect("couldn't clear predictor area");

                if debug1 {
                    info!("valid_predictions: {}", valid_predictions);
                }
                // OK, let's start initially with just a naive, split-by-N layout of the prediction area
                let approx_width = pc_bounds.x / valid_predictions as isize;

                let mut i = 0;
                for p in self.pred_options.iter() {
                    if let Some(pred_str) = p {
                        // the post-clip is necessary because the approx_width is rounded to some integer
                        // fraction
                        let p_clip = Rectangle::new(
                            Point::new(i * approx_width, 1),
                            Point::new((i + 1) * approx_width, pc_bounds.y),
                        )
                        .clip_with(pc_clip)
                        .unwrap();
                        if i > 0 {
                            self.gam
                                .draw_line(
                                    pc,
                                    Line::new_with_style(
                                        Point::new(i * approx_width, 1),
                                        Point::new(i * approx_width, pc_bounds.y),
                                        DrawStyle {
                                            fill_color: None,
                                            stroke_color: Some(PixelColor::Dark),
                                            stroke_width: 1,
                                        },
                                    ),
                                )
                                .expect("couldn't draw dividing lines in prediction area");
                        }
                        let mut p_tv = TextView::new(pc, TextBounds::BoundingBox(p_clip));
                        p_tv.draw_border = false;
                        p_tv.border_width = 1;
                        p_tv.clear_area = false;
                        p_tv.ellipsis = true;
                        p_tv.style = gam::SYSTEM_STYLE;
                        write!(p_tv.text, "{}", pred_str).expect("can't write the prediction string");
                        log::trace!("posting string with length {}", p_tv.text.as_str().len());
                        self.gam.post_textview(&mut p_tv).expect("couldn't post prediction text");
                        i += 1;
                    }
                }
            }
        }

        log::trace!("imef: redraw##");
        self.gam.redraw().expect("couldn't redraw screen");

        Ok(retstring)
    }
}

fn main() -> ! {
    let debug1 = false;
    let dbglistener = false;
    let dbgcanvas = false;
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // only one public connection allowed: GAM
    let imef_sid =
        xns.register_name(ime_plugin_api::SERVER_NAME_IME_FRONT, Some(1)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", imef_sid);

    let mut tracker = InputTracker::new(&xns);

    let mut listener: Option<CID> = None;

    // create the emoji menu handler
    emoji_menu(xous::connect(imef_sid).unwrap());

    log::trace!("Initialized but still waiting for my canvas Gids");
    // the API token allows individual predictor back end uses to have their own history buffers
    let mut api_token: Option<ApiToken> = None;
    loop {
        let msg = xous::receive_message(imef_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ImefOpcode::ConnectBackend) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let descriptor = buffer.to_original::<ImefDescriptor, _>().unwrap();

                if let Some(input) = descriptor.input_canvas {
                    if debug1 || dbgcanvas {
                        info!("got input canvas {:?}", input);
                    }
                    tracker.set_input_canvas(input);
                } else {
                    tracker.clear_input_canvas();
                }
                if let Some(pred) = descriptor.prediction_canvas {
                    if debug1 || dbgcanvas {
                        info!("got prediction canvas {:?}", pred);
                    }
                    tracker.set_pred_canvas(pred);
                } else {
                    tracker.clear_pred_canvas();
                }
                // disconnect any existing predictor, if we have one already
                if let Some(pred) = tracker.get_predictor() {
                    pred.release(api_token.take().unwrap().api_token); // api token *should* be Some() if pred is Some()
                    if let Some((name, token)) = tracker.predictor_conn {
                        xns.disconnect_with_token(name.as_str(), token)
                           .expect("couldn't disconnect from previous predictor. Something is wrong with internal state!");
                    }
                    tracker.predictor_conn = None;
                    tracker.set_predictor(None);
                }
                if let Some(s) = descriptor.predictor {
                    match xns.request_connection_with_token(s.as_str()) {
                        Ok((pc, token)) => {
                            let pred = ime_plugin_api::PredictionPlugin { connection: Some(pc) };
                            match pred.acquire(descriptor.predictor_token) {
                                Ok(confirmation) => {
                                    api_token = Some(ApiToken {
                                        api_token: confirmation,
                                        gam_token: descriptor.token,
                                    });
                                }
                                Err(e) => log::error!("Internal error: {:?}", e),
                            }
                            tracker.set_predictor(Some(pred));
                            tracker.predictor_conn = Some((
                                String::from(s.as_str()),
                                token.expect("didn't get the disconnect token!"),
                            ));
                        }
                        _ => error!("can't find predictive engine {}, retaining existing one.", s.as_str()),
                    }
                }
                log::debug!("predictor: {:?}, api_token: {:?}", tracker.get_predictor(), api_token);
                tracker.set_gam_token(descriptor.token);
                if let Some(at) = &api_token {
                    tracker.send_api_token(at);
                }
            }
            Some(ImefOpcode::RegisterListener) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = Some(xous::connect(sid).unwrap());
                log::trace!("listener registered: {:?}", sid);
                if listener.is_none() {
                    listener = cid;
                } else {
                    error!("RegisterCallback listener ran out of space registering callback");
                }
            }),
            Some(ImefOpcode::ProcessKeys) => {
                if tracker.is_init() && api_token.is_some() {
                    msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                        let keys = [
                            core::char::from_u32(k1 as u32).unwrap_or('\u{0000}'),
                            core::char::from_u32(k2 as u32).unwrap_or('\u{0000}'),
                            core::char::from_u32(k3 as u32).unwrap_or('\u{0000}'),
                            core::char::from_u32(k4 as u32).unwrap_or('\u{0000}'),
                        ];
                        log::trace!("tracking keys: {:?}", keys);
                        if keys[0] == '😊' {
                            tracker.activate_emoji();
                        } else {
                            if let Some(line) = tracker
                                .update(keys, false, api_token.as_ref().unwrap().api_token)
                                .expect("couldn't update input tracker with latest key presses")
                            {
                                if dbglistener {
                                    info!("sending listeners {:?}", line);
                                }
                                if let Some(conn) = listener {
                                    if dbglistener {
                                        info!("sending to conn {:?}", conn);
                                    }
                                    let buf =
                                        Buffer::into_buf(line).or(Err(xous::Error::InternalError)).unwrap();
                                    match buf.send(conn, ImefCallback::GotInputLine.to_u32().unwrap()) {
                                        Err(xous::Error::ServerNotFound) => {
                                            listener = None; // the listener went away, free up our slot so a new one can register
                                        }
                                        Ok(xous::Result::Ok) => {}
                                        Ok(xous::Result::MemoryReturned(offset, valid)) => {
                                            // ignore anything that's returned, but note it in case we're
                                            // debugging
                                            log::trace!(
                                                "memory was returned in callback: offset {:?}, valid {:?}",
                                                offset,
                                                valid
                                            );
                                        }
                                        Err(e) => {
                                            log::error!("unhandled error in callback processing: {:?}", e);
                                        }
                                        Ok(e) => {
                                            log::error!("unexpected result in callback processing: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }
                    });
                } else {
                    log::trace!("got keys, but we're not initialized");
                    // ignore keyboard events until we've fully initialized
                }
            }
            Some(ImefOpcode::Redraw) => msg_scalar_unpack!(msg, arg, _, _, _, {
                if tracker.is_init() && api_token.is_some() {
                    let force = if arg != 0 { true } else { false };
                    tracker.clear_area().expect("can't initially clear areas");
                    tracker
                        .update(['\u{0000}'; 4], force, api_token.as_ref().unwrap().api_token)
                        .expect("can't setup initial screen arrangement");
                } else {
                    log::trace!("got redraw, but we're not initialized");
                    // ignore keyboard events until we've fully initialized
                }
            }),
            Some(ImefOpcode::SetMenuMode) => msg_scalar_unpack!(msg, arg, _, _, _, {
                if arg == 1 {
                    tracker.set_menu_mode(true);
                } else {
                    tracker.set_menu_mode(false);
                }
            }),
            Some(ImefOpcode::Quit) => {
                log::error!("recevied quit, goodbye!");
                break;
            }
            None => {
                log::error!("couldn't convert opcode");
            }
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(imef_sid).unwrap();
    xous::destroy_server(imef_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
