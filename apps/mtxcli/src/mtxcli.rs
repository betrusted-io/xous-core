use core::fmt::Write;

use gam::UxRegistration;
use graphics_server::api::GlyphStyle;
use graphics_server::{DrawStyle, Gid, PixelColor, Point, Rectangle, TextBounds, TextView};
use locales::t;
use xous::MessageEnvelope;

use super::*;

#[derive(Debug)]
struct History {
    // the history record
    pub text: String,
    // if true, this was input from the user; if false, it's a response from the shell
    pub is_input: bool,
}

pub(crate) struct Mtxcli {
    // optional structures that indicate new input to the Mtxcli loop per iteration
    // an input string
    input: Option<String>,
    // messages from other servers
    msg: Option<MessageEnvelope>,

    // record our input history
    history: Vec<History>,
    history_len: usize,
    content: Gid,
    gam: gam::Gam,

    // variables that define our graphical attributes
    screensize: Point,
    bubble_width: u16,
    margin: Point,        // margin to edge of canvas
    bubble_margin: Point, // margin of text in bubbles
    bubble_radius: u16,
    bubble_space: i16, // spacing between text bubbles

    // command environment
    env: CmdEnv,
}
impl Mtxcli {
    pub(crate) fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        let gam = gam::Gam::new(xns).expect("can't connect to GAM");

        let token = gam
            .register_ux(UxRegistration {
                app_name: String::from(gam::APP_NAME_MTXCLI),
                ux_type: gam::UxType::Chat,
                predictor: Some(String::from(ime_plugin_shell::SERVER_NAME_IME_PLUGIN_SHELL)),
                listener: sid.to_array(), /* note disclosure of our SID to the GAM -- the secret is now
                                           * shared with the GAM! */
                redraw_id: MtxcliOp::Redraw.to_u32().unwrap(),
                gotinput_id: Some(MtxcliOp::Line.to_u32().unwrap()),
                audioframe_id: None,
                rawkeys_id: None,
                focuschange_id: Some(MtxcliOp::ChangeFocus.to_u32().unwrap()),
            })
            .expect("couldn't register Ux context for mtxcli");

        let content = gam.request_content_canvas(token.unwrap()).expect("couldn't get content canvas");
        let screensize = gam.get_canvas_bounds(content).expect("couldn't get dimensions of content canvas");
        let history: Vec<History> =
            vec![History { text: String::from(t!("mtxcli.greeting", locales::LANG)), is_input: false }];
        let env = CmdEnv::new();
        Mtxcli {
            input: None,
            msg: None,
            history,
            history_len: 10,
            content,
            gam,
            screensize,
            bubble_width: ((screensize.x / 5) * 4) as u16, // 80% width for the text bubbles
            margin: Point::new(4, 4),
            bubble_margin: Point::new(4, 4),
            bubble_radius: 4,
            bubble_space: 4,
            env,
        }
    }

    /// accept a new input string
    pub(crate) fn input(&mut self, line: &str) -> Result<(), xous::Error> {
        self.input = Some(String::from(line));

        Ok(())
    }

    pub(crate) fn msg(&mut self, message: MessageEnvelope) { self.msg = Some(message); }

    fn circular_push(&mut self, item: History) {
        if self.history.len() >= self.history_len {
            self.history.remove(0);
        }
        self.history.push(item);
    }

    /// update the loop, in response to various inputs
    pub(crate) fn update(&mut self, was_callback: bool) -> Result<(), xous::Error> {
        // if we had an input string, do something
        if let Some(local) = &self.input {
            let input_history = History { text: local.to_string(), is_input: true };
            self.circular_push(input_history);
        }

        // AT THIS POINT: if we have other inputs, update accordingly
        // other inputs might be, for example, events that came in from other servers that would
        // side effect our commands

        // redraw UI once upon accepting all input
        if !was_callback {
            // don't need to redraw on a callback, save some cycles
            self.redraw().expect("can't redraw");
        }

        let mut dirty = true;
        // take the input and pass it on to the various command parsers, and attach result
        if let Some(local) = &self.input {
            if let Some(res) =
                self.env.dispatch(Some(&mut String::from(&local)), None).expect("command dispatch failed")
            {
                let output_history =
                    History { text: String::from(res.as_str().unwrap_or("UTF-8 Error")), is_input: false };
                self.circular_push(output_history);
            } else {
                dirty = false;
            }
        } else if let Some(msg) = &self.msg {
            if let Some(res) = self.env.dispatch(None, Some(msg)).expect("callback failed") {
                let output_history =
                    History { text: String::from(res.as_str().unwrap_or("UTF-8 Error")), is_input: false };
                self.circular_push(output_history);
            } else {
                dirty = false;
            }
        }

        // clear all the inputs to the loop, so we don't process them twice
        self.input = None;
        self.msg = None;
        // redraw UI now that we've responded
        if dirty {
            self.redraw().expect("can't redraw");
        }

        Ok(())
    }

    fn clear_area(&self) {
        self.gam
            .draw_rectangle(
                self.content,
                Rectangle::new_with_style(
                    Point::new(0, 0),
                    self.screensize,
                    DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 },
                ),
            )
            .expect("can't clear content area");
    }

    pub(crate) fn redraw(&mut self) -> Result<(), xous::Error> {
        self.clear_area();

        // this defines the bottom border of the text bubbles as they stack up wards
        let mut bubble_baseline = self.screensize.y - self.margin.y;

        // iterator returns from oldest to newest
        // .rev() iterator is from newest to oldest
        for h in self.history.iter().rev() {
            let mut bubble_tv = if h.is_input {
                TextView::new(
                    self.content,
                    TextBounds::GrowableFromBr(
                        Point::new(self.screensize.x - self.margin.x, bubble_baseline),
                        self.bubble_width,
                    ),
                )
            } else {
                TextView::new(
                    self.content,
                    TextBounds::GrowableFromBl(Point::new(self.margin.x, bubble_baseline), self.bubble_width),
                )
            };
            if h.is_input {
                bubble_tv.border_width = 1;
            } else {
                bubble_tv.border_width = 2;
            }
            bubble_tv.draw_border = true;
            bubble_tv.clear_area = true;
            bubble_tv.rounded_border = Some(self.bubble_radius);
            bubble_tv.style = GlyphStyle::Regular;
            bubble_tv.margin = self.bubble_margin;
            bubble_tv.ellipsis = false;
            bubble_tv.insertion = None;
            write!(bubble_tv.text, "{}", h.text.as_str()).expect("couldn't write history text to TextView");
            self.gam.post_textview(&mut bubble_tv).expect("couldn't render bubble textview");

            if let Some(bounds) = bubble_tv.bounds_computed {
                // we only subtract 1x of the margin because the bounds were computed from a "bottom right"
                // that already counted the margin once.
                bubble_baseline -= (bounds.br.y - bounds.tl.y) + self.bubble_space + self.bubble_margin.y;
                if bubble_baseline <= 0 {
                    // don't draw history that overflows the top of the screen
                    break;
                }
            } else {
                break; // we get None on the bounds computed if the text view fell off the top of the screen
            }
        }
        log::trace!("mtxcli app redraw##");
        self.gam.redraw().expect("couldn't redraw screen");
        Ok(())
    }
}
