#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::info;

use core::fmt::Write;

use gam::UxRegistration;
use graphics_server::{Gid, Point, Rectangle, TextBounds, TextView, DrawStyle, GlyphStyle, PixelColor};
use xous::MessageEnvelope;
use xous_ipc::{String, Buffer};

use heapless::spsc::Queue;

mod cmds;
use cmds::*;

#[derive(Debug)]
struct History {
    // the history record
    pub text: String<1024>,
    // if true, this was input from the user; if false, it's a response from the shell
    pub is_input: bool,
}

#[derive(Debug)]
struct Repl {
    // optional structures that indicate new input to the Repl loop per iteration
    // an input string
    input: Option<String<1024>>,
    // messages from other servers
    msg: Option<MessageEnvelope>,

    // record our input history
    history: Queue::<History, 16>,
    content: Gid,
    gam: gam::Gam,

    // variables that define our graphical attributes
    screensize: Point,
    bubble_width: u16,
    margin: Point, // margin to edge of canvas
    bubble_margin: Point, // margin of text in bubbles
    bubble_radius: u16,
    bubble_space: i16, // spacing between text bubbles

    // command environment
    env: CmdEnv,

    // our security token for making changes to our record on the GAM
    token: [u32; 4],
}
impl Repl{
    fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        let gam = gam::Gam::new(xns).expect("can't connect to GAM");

        let token = gam.register_ux(UxRegistration {
            app_name: String::<128>::from_str(APP_NAME_SHELLCHAT),
            ux_type: gam::UxType::Chat,
            predictor: Some(String::<64>::from_str(ime_plugin_shell::SERVER_NAME_IME_PLUGIN_SHELL)),
            listener: sid.to_array(), // note disclosure of our SID to the GAM -- the secret is now shared with the GAM!
            redraw_id: ShellOpcode::Redraw.to_u32().unwrap(),
            gotinput_id: Some(ShellOpcode::Line.to_u32().unwrap()),
            audioframe_id: None,
            rawkeys_id: None,
        }).expect("couldn't register Ux context for shellchat");

        // we should be the first app running, so get the focus
        gam.request_focus(token.unwrap()).expect("couldn't take focus");

        let content = gam.request_content_canvas(token.unwrap()).expect("couldn't get content canvas");
        log::debug!("content canvas {:?}", content);
        let screensize = gam.get_canvas_bounds(content).expect("couldn't get dimensions of content canvas");
        log::debug!("size {:?}", screensize);
        Repl {
            input: None,
            msg: None,
            history: Queue::new(),
            content,
            gam,
            screensize,
            bubble_width: ((screensize.x / 5) * 4) as u16, // 80% width for the text bubbles
            margin: Point::new(4, 4),
            bubble_margin: Point::new(4, 4),
            bubble_radius: 4,
            bubble_space: 4,
            env: CmdEnv::new(xns),
            token: token.unwrap(),
        }
    }

    /// accept a new input string
    fn input(&mut self, line: &str) -> Result<(), xous::Error> {
        let mut local = String::<1024>::new();
        write!(local, "{}", line).expect("line too long for history buffer");

        self.input = Some(local);

        Ok(())
    }

    fn msg(&mut self, message: MessageEnvelope) {
        self.msg = Some(message);
    }

    fn circular_push(&mut self, item: History) {
        if self.history.len() == self.history.capacity() {
            self.history.dequeue().expect("couldn't dequeue historye");
        }
        self.history.enqueue(item).expect("couldn't store input line");
    }

    /// update the loop, in response to various inputs
    fn update(&mut self, was_callback: bool) -> Result<(), xous::Error> {
        let debug1 = false;
        // if we had an input string, do something
        if let Some(local) = self.input {
            let input_history = History {
                text: local,
                is_input: true,
            };
            self.circular_push(input_history);
        }

        // AT THIS POINT: if we have other inputs, update accordingly
        // other inputs might be, for example, events that came in from other servers that would
        // side effect our commands

        // redraw UI once upon accepting all input
        if !was_callback { // don't need to redraw on a callback, save some cycles
            self.redraw().expect("can't redraw");
        }

        let mut dirty = true;
        // take the input and pass it on to the various command parsers, and attach result
        if let Some(mut local) = self.input {
            log::trace!("processing line: {}", local);
            if let Some(res) = self.env.dispatch(Some(&mut local), None).expect("command dispatch failed") {
                let mut response = String::<1024>::new();
                write!(response, "{}", res).expect("can't copy result to history");
                let output_history = History {
                    text: response,
                    is_input: false
                };
                self.circular_push(output_history);
            } else {
                dirty = false;
            }
        } else if let Some(msg) = &self.msg {
            log::trace!("processing callback msg: {:?}", msg);
            if let Some(res) = self.env.dispatch(None, Some(msg)).expect("callback failed") {
                let mut response = String::<1024>::new();
                write!(response, "{}", res).expect("can't copy result to history");
                let output_history = History {
                    text: response,
                    is_input: false
                };
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

        if debug1 {
            for h in self.history.iter() {
                info!("command history is_input: {}, text:{}", h.is_input, h.text);
            }
        }
        Ok(())
    }

    fn clear_area(&self) {
        self.gam.draw_rectangle(self.content,
            Rectangle::new_with_style(Point::new(0, 0), self.screensize,
            DrawStyle {
                fill_color: Some(PixelColor::Light),
                stroke_color: None,
                stroke_width: 0
            }
        )).expect("can't clear content area");
    }
    fn redraw(&mut self) -> Result<(), xous::Error> {
        log::trace!("going into redraw");
        self.clear_area();

        // this defines the bottom border of the text bubbles as they stack up wards
        let mut bubble_baseline = self.screensize.y - self.margin.y;

        log::trace!("drawing chat history");
        // iterator returns from oldest to newest
        // .rev() iterator is from newest to oldest
        for h in self.history.iter().rev() {
            let mut bubble_tv =
                if h.is_input {
                    TextView::new(self.content,
                    TextBounds::GrowableFromBr(
                        Point::new(self.screensize.x - self.margin.x, bubble_baseline),
                        self.bubble_width))
                } else {
                    TextView::new(self.content,
                        TextBounds::GrowableFromBl(
                            Point::new(self.margin.x, bubble_baseline),
                            self.bubble_width))
                };
            if h.is_input {
                bubble_tv.border_width = 1;
            } else {
                bubble_tv.border_width = 2;
            }
            bubble_tv.draw_border = true;
            bubble_tv.clear_area = true;
            bubble_tv.rounded_border = Some(self.bubble_radius);
            bubble_tv.style = GlyphStyle::Small;
            bubble_tv.margin = self.bubble_margin;
            bubble_tv.ellipsis = false; bubble_tv.insertion = None;
            write!(bubble_tv.text, "{}", h.text.as_str().expect("couldn't convert history text")).expect("couldn't write history text to TextView");
            log::trace!("posting {}", bubble_tv.text);
            self.gam.post_textview(&mut bubble_tv).expect("couldn't render bubble textview");

            if let Some(bounds) = bubble_tv.bounds_computed {
                // we only subtract 1x of the margin because the bounds were computed from a "bottom right" that already counted
                // the margin once.
                bubble_baseline -= (bounds.br.y - bounds.tl.y) + self.bubble_space + self.bubble_margin.y;
                if bubble_baseline <= 0 {
                    // don't draw history that overflows the top of the screen
                    break;
                }
            } else {
                break; // we get None on the bounds computed if the text view fell off the top of the screen
            }
        }
        self.gam.redraw().expect("couldn't redraw screen");
        // self.gam.request_ime_redraw().expect("couldn't redraw the IME area");
        Ok(())
    }
}

////////////////// local message passing from Ux Callback
use num_traits::{ToPrimitive, FromPrimitive};

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum ShellOpcode {
    /// a line of text has arrived
    Line = 0, // make sure we occupy opcodes with discriminants < 1000, as the rest are used for callbacks
    /// redraw our UI
    Redraw,
    /// exit the application
    Quit,
}
//////////////////

// nothing prevents the two from being the same, other than naming conventions
pub(crate) const SERVER_NAME_SHELLCHAT: &str = "_Shell chat application_"; // used internally by xous-names
pub(crate) const APP_NAME_SHELLCHAT: &str = "shellchat"; // the user-facing name

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Debug);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed, this is a user app and it's up to the app to decide its policy
    let shch_sid = xns.register_name(SERVER_NAME_SHELLCHAT, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", shch_sid);

    let mut repl = Repl::new(&xns, shch_sid);
    let mut update_repl = false;
    let mut was_callback = false;

    log::trace!("starting main loop");
    loop {
        let msg = xous::receive_message(shch_sid).unwrap();
        log::trace!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ShellOpcode::Line) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<String<4000>, _>().unwrap();
                log::trace!("shell got input line: {}", s.as_str());
                repl.input(s.as_str()).expect("REPL couldn't accept input string");
                update_repl = true; // set a flag, instead of calling here, so message can drop and calling server is released
                was_callback = false;
            }
            Some(ShellOpcode::Redraw) => {
                log::trace!("got Redraw");
                repl.redraw().expect("REPL couldn't redraw");
            }
            Some(ShellOpcode::Quit) => {
                log::trace!("got Quit");
                break;
            }
            _ => {
                log::trace!("got unknown message, treating as callback");
                repl.msg(msg);
                update_repl = true;
                was_callback = true;
            }
        }
        if update_repl {
            repl.update(was_callback).expect("REPL had problems updating");
            update_repl = false;
        }
        log::trace!("reached bottom of main loop");
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(shch_sid).unwrap();
    xous::destroy_server(shch_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
