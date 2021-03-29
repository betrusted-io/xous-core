#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::info;

use core::fmt::Write;
use core::convert::TryFrom;

use rkyv::Deserialize;
use rkyv::archived_value;
use core::pin::Pin;

use ime_plugin_api::{ImeFrontEndApi, ImeFrontEnd};
use graphics_server::{Gid, Point, Rectangle, TextBounds, TextView, DrawStyle, GlyphStyle, PixelColor};
use xous::MessageEnvelope;
use xous_ipc::String;

use heapless::spsc::Queue;
use heapless::consts::U16;

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
    history: Queue<History, U16>,
    content: Gid,
    gam: xous::CID,

    // variables that define our graphical attributes
    screensize: Point,
    bubble_width: u16,
    margin: Point, // margin to edge of canvas
    bubble_margin: Point, // margin of text in bubbles
    bubble_radius: u16,
    bubble_space: i16, // spacing between text bubbles

    // command environment
    env: CmdEnv,
}
impl Repl{
    fn new(my_server_name: &str) -> Self {
        let gam_conn = xns.request_connection_blocking(xous::names::SERVER_NAME_GAM).expect("SHCH: can't connect to GAM");
        let content = gam::request_content_canvas(gam_conn, my_server_name).expect("SHCH: couldn't get content canvas");
        let screensize = gam::get_canvas_bounds(gam_conn, content).expect("SHCH: couldn't get dimensions of content canvas");
        Repl {
            input: None,
            msg: None,
            history: Queue::new(),
            content,
            gam: gam_conn,
            screensize,
            bubble_width: ((screensize.x / 5) * 4) as u16, // 80% width for the text bubbles
            margin: Point::new(4, 4),
            bubble_margin: Point::new(4, 4),
            bubble_radius: 4,
            bubble_space: 4,
            env: CmdEnv::new(gam_conn),
        }
    }

    /// accept a new input string
    fn input(&mut self, line: &str) -> Result<(), xous::Error> {
        let mut local = String::<1024>::new();
        write!(local, "{}", line).expect("SHCH: line too long for history buffer");

        self.input = Some(local);

        Ok(())
    }

    fn msg(&mut self, message: MessageEnvelope) {
        self.msg = Some(message);
    }

    fn circular_push(&mut self, item: History) {
        if self.history.len() == self.history.capacity() {
            self.history.dequeue().expect("SHCH: couldn't dequeue historye");
        }
        self.history.enqueue(item).expect("SHCH: couldn't store input line");
    }

    /// update the loop, in response to various inputs
    fn update(&mut self) -> Result<(), xous::Error> {
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
        self.redraw().expect("SHCH: can't redraw");

        // take the input and pass it on to the various command parsers, and attach result
        if let Some(mut local) = self.input {
            if let Some(res) = self.env.dispatch(Some(&mut local), None).expect("SHCH: command dispatch failed") {
                let mut response = String::<1024>::new();
                write!(response, "{}", res).expect("SHCH: can't copy result to history");
                let output_history = History {
                    text: response,
                    is_input: false
                };
                self.circular_push(output_history);
            }
        } else if let Some(msg) = &self.msg {
            if let Some(res) = self.env.dispatch(None, Some(msg)).expect("SCHC: callback failed") {
                let mut response = String::<1024>::new();
                write!(response, "{}", res).expect("SHCH: can't copy result to history");
                let output_history = History {
                    text: response,
                    is_input: false
                };
                self.circular_push(output_history);
            }
        }

        // clear all the inputs to the loop, so we don't process them twice
        self.input = None;
        self.msg = None;
        // redraw UI now that we've responded
        self.redraw().expect("SHCH: can't redraw");

        if debug1 {
            for h in self.history.iter() {
                info!("SHCH: command history is_input: {}, text:{}", h.is_input, h.text);
            }
        }
        Ok(())
    }

    fn clear_area(&self) {
        gam::draw_rectangle(self.gam, self.content,
            Rectangle::new_with_style(Point::new(0, 0), self.screensize,
            DrawStyle {
                fill_color: Some(PixelColor::Light),
                stroke_color: None,
                stroke_width: 0
            }
        )).expect("SHCH: can't clear content area");
    }
    fn redraw(&mut self) -> Result<(), xous::Error> {
        self.clear_area();

        // this defines the bottom border of the text bubbles as they stack up wards
        let mut bubble_baseline = self.screensize.y - self.margin.y;

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
            write!(bubble_tv.text, "{}", h.text.as_str().expect("SHCH: couldn't convert history text")).expect("SHCH: couldn't write history text to TextView");
            gam::post_textview(self.gam, &mut bubble_tv).expect("SHCH: couldn't render bubble textview");

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
        Ok(())
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = false;
    log_server::init_wait().unwrap();
    info!("SHCH: my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let shch_sid = xns.register_name(xous::names::SERVER_NAME_SHELLCHAT).expect("SHCH: can't register server");
    info!("SHCH: registered with NS -- {:?}", shch_sid);

    let imef_conn = xns.request_connection_blocking(xous::names::SERVER_NAME_IME_FRONT).expect("SHCH: can't connect to IMEF");
    let imef = ImeFrontEnd { connection: Some(imef_conn) };
    imef.register_listener(xous::names::SERVER_NAME_SHELLCHAT).expect("SHCH: couldn't request events from IMEF");

    let mut repl = Repl::new(xous::names::SERVER_NAME_SHELLCHAT);
    let mut update_repl = false;
    info!("SHCH: starting main loop");
    loop {
        let envelope = xous::receive_message(shch_sid).unwrap();
        if debug1{info!("SHCH: got message {:?}", envelope);}
        if let xous::Message::Borrow(m) = &envelope.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<ime_plugin_api::ImefOpcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<ime_plugin_api::ImefOpcode>::GotInputLine(rkyv_s) => {
                    let s: String<4000> = rkyv_s.deserialize(&mut xous_ipc::XousDeserializer {}).unwrap();
                    repl.input(s.as_str().expect("SHCH: couldn't convert incoming string")).expect("SHCH: REPL couldn't accept input string");
                    update_repl = true; // set a flag, instead of calling here, so message can drop and calling server is released
                },
                _ => panic!("SHCH: invalid response from server -- corruption occurred in MemoryMessage")
            };
        } else if let Ok(opcode) = content_plugin_api::Opcode::try_from(&envelope.body) {
            match opcode {
                content_plugin_api::Opcode::Redraw => {
                    repl.redraw().expect("SHCH: REPL couldn't redraw");
                }
            }
        } else {
            repl.msg(envelope);
            update_repl = true;
        }

        if update_repl {
            repl.update().expect("SHCH: REPL had problems updating");
            update_repl = false;
        }
    }
}
