#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

/*! Detailed docs are parked under Modules/cmds "Shell Chat" below

To make your own command, copy the `ver.rs` template (for an example with argument parsing),
or snag the very simple echo template below, and put
it in the services/shellchat/src/cmds/ directory:

```Rust
use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;

#[derive(Debug)]
pub struct Echo {
}

impl<'a> ShellCmdApi<'a> for Echo {
    cmd_api!(echo); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        Ok(Some(rest))
    }
}
```

Once you've added your command to the directory, go to the `cmds.rs` file, and follow
the four-step instructions embedded within the file, starting around line 40.

Check for more detailed docs under Modules/cmds "Shell Chat" below
*/
use log::info;

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use gam::UxRegistration;
use graphics_server::{Gid, Point, Rectangle, TextBounds, TextView, DrawStyle, PixelColor};
use xous::MessageEnvelope;
use xous_ipc::Buffer;

use std::thread;
use std::sync::Arc;

#[doc = include_str!("../README.md")]
mod cmds;
use cmds::*;

#[cfg(not(feature="no-codec"))]
mod oqc_test;

#[cfg(feature="nettest")]
mod nettests;

#[cfg(target_os = "xous")] // only draw "please wait" when not in hosted mode
use locales::t;
#[cfg(feature="tts")]
use tts_frontend::*;

#[cfg(feature="tracking-alloc")]
use tracking_allocator::{
    AllocationGroupId, AllocationTracker, Allocator,
};
#[cfg(feature="tracking-alloc")]
use std::alloc::System;
#[cfg(feature="tracking-alloc")]
#[global_allocator]
static GLOBAL: Allocator<System> = Allocator::system();
#[cfg(feature="tracking-alloc")]
use core::sync::atomic::AtomicIsize;
#[cfg(feature="tracking-alloc")]
struct StdoutTracker {
    pub total: AtomicIsize,
}

#[cfg(feature="tracking-alloc")]
impl AllocationTracker for StdoutTracker {
    fn allocated(&self, addr: usize, size: usize, group_id: AllocationGroupId) {
        // Allocations have all the pertinent information upfront, which you may or may not want to store for further
        // analysis. Notably, deallocations also know how large they are, and what group ID they came from, so you
        // typically don't have to store much data for correlating deallocations with their original allocation.
        self.total.store(self.total.load(Ordering::SeqCst) + size as isize, Ordering::SeqCst);
        println!(
            "allocation -> total={} addr=0x{:0x} size={} group_id={:?}",
            self.total.load(Ordering::SeqCst), addr, size, group_id
        );
    }

    fn deallocated(
        &self,
        addr: usize,
        size: usize,
        source_group_id: AllocationGroupId,
        current_group_id: AllocationGroupId,
    ) {
        // When a deallocation occurs, as mentioned above, you have full access to the address, size of the allocation,
        // as well as the group ID the allocation was made under _and_ the active allocation group ID.
        //
        // This can be useful beyond just the obvious "track how many current bytes are allocated by the group", instead
        // going further to see the chain of where allocations end up, and so on.
        self.total.store(self.total.load(Ordering::SeqCst) - size as isize, Ordering::SeqCst);
        println!(
            "deallocation -> total={} addr=0x{:0x} size={} source_group_id={:?} current_group_id={:?}",
            self.total.load(Ordering::SeqCst), addr, size, source_group_id, current_group_id
        );
    }
}
#[derive(Debug)]
struct History {
    // the history record
    pub text: String,
    // if true, this was input from the user; if false, it's a response from the shell
    pub is_input: bool,
}

#[allow(dead_code)]
struct Repl {
    // optional structures that indicate new input to the Repl loop per iteration
    // an input string
    input: Option<String>,
    // messages from other servers
    msg: Option<MessageEnvelope>,

    // record our input history
    history: Vec::<History>,
    history_len: usize,
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
    #[cfg(feature="tts")]
    tts: TtsFrontend,
}
impl Repl{
    fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        let gam = gam::Gam::new(xns).expect("can't connect to GAM");

        let token = gam.register_ux(UxRegistration {
            app_name: xous_ipc::String::<128>::from_str(gam::APP_NAME_SHELLCHAT),
            ux_type: gam::UxType::Chat,
            #[cfg(not(feature="tts"))]
            predictor: Some(xous_ipc::String::<64>::from_str(ime_plugin_shell::SERVER_NAME_IME_PLUGIN_SHELL)),
            #[cfg(feature="tts")]
            predictor: Some(xous_ipc::String::<64>::from_str(ime_plugin_tts::SERVER_NAME_IME_PLUGIN_TTS)),
            listener: sid.to_array(), // note disclosure of our SID to the GAM -- the secret is now shared with the GAM!
            redraw_id: ShellOpcode::Redraw.to_u32().unwrap(),
            gotinput_id: Some(ShellOpcode::Line.to_u32().unwrap()),
            audioframe_id: None,
            rawkeys_id: None,
            focuschange_id: Some(ShellOpcode::ChangeFocus.to_u32().unwrap()),
        }).expect("couldn't register Ux context for shellchat");

        let content = gam.request_content_canvas(token.unwrap()).expect("couldn't get content canvas");
        log::trace!("content canvas {:?}", content);
        let screensize = gam.get_canvas_bounds(content).expect("couldn't get dimensions of content canvas");
        log::trace!("size {:?}", screensize);
        Repl {
            input: None,
            msg: None,
            history: Vec::new(),
            history_len: 10,
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
            #[cfg(feature="tts")]
            tts: TtsFrontend::new(xns).unwrap(),
        }
    }

    /// accept a new input string
    fn input(&mut self, line: &str) -> Result<(), xous::Error> {
        self.input = Some(String::from(line));

        Ok(())
    }

    fn msg(&mut self, message: MessageEnvelope) {
        self.msg = Some(message);
    }

    fn circular_push(&mut self, item: History) {
        if self.history.len() >= self.history_len {
            self.history.remove(0);
        }
        self.history.push(item);
    }

    /// update the loop, in response to various inputs
    fn update(&mut self, was_callback: bool, init_done: bool) -> Result<(), xous::Error> {
        let debug1 = false;
        // if we had an input string, do something
        if let Some(local) = &self.input {
            let input_history = History {
                text: local.to_string(),
                is_input: true,
            };
            self.circular_push(input_history);
        }

        // AT THIS POINT: if we have other inputs, update accordingly
        // other inputs might be, for example, events that came in from other servers that would
        // side effect our commands

        // redraw UI once upon accepting all input
        if !was_callback { // don't need to redraw on a callback, save some cycles
            self.redraw(init_done).expect("can't redraw");
        }

        let mut dirty = true;
        // take the input and pass it on to the various command parsers, and attach result
        if let Some(local) = &self.input {
            log::trace!("processing line: {}", local);
            if let Some(res) = self.env.dispatch(Some(&mut xous_ipc::String::<1024>::from_str(&local)), None).expect("command dispatch failed") {
                #[cfg(feature="tts")]
                {
                    let mut output = t!("shellchat.output-tts", locales::LANG).to_string();
                    output.push_str(res.as_str().unwrap_or("UTF-8 error"));
                    self.tts.tts_simple(&output).unwrap();
                }
                let output_history = History {
                    text: String::from(res.as_str().unwrap_or("UTF-8 Error")),
                    is_input: false
                };
                self.circular_push(output_history);
            } else {
                dirty = false;
            }
        } else if let Some(msg) = &self.msg {
            log::trace!("processing callback msg: {:?}", msg);
            if let Some(res) = self.env.dispatch(None, Some(msg)).expect("callback failed") {
                #[cfg(feature="tts")]
                {
                    let mut output = t!("shellchat.output-tts", locales::LANG).to_string();
                    output.push_str(res.as_str().unwrap_or("UTF-8 error"));
                    self.tts.tts_simple(&output).unwrap();
                }
                let output_history = History {
                    text: String::from(res.as_str().unwrap_or("UTF-8 Error")),
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
            self.redraw(init_done).expect("can't redraw");
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
    fn redraw(&mut self, _init_done: bool) -> Result<(), xous::Error> {
        log::trace!("going into redraw");
        self.clear_area();

        #[cfg(target_os = "xous")] // only draw "please wait" if we're not in hosted mode
        if !_init_done {
            let mut init_tv = TextView::new(
                self.content,
                TextBounds::CenteredTop(
                    Rectangle::new(
                        Point::new(0, self.screensize.y / 3 - 64),
                        Point::new(self.screensize.x, self.screensize.y / 3)
                    )
                )
            );
            init_tv.style = GlyphStyle::Bold;
            init_tv.draw_border = false;
            write!(init_tv.text, "{}", t!("shellchat.bootwait", locales::LANG)).ok();
            self.gam.post_textview(&mut init_tv).expect("couldn't render wait text");
            self.gam.redraw().expect("couldn't redraw screen");
        }

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
            bubble_tv.style = gam::SYSTEM_STYLE;
            bubble_tv.margin = self.bubble_margin;
            bubble_tv.ellipsis = false; bubble_tv.insertion = None;
            write!(bubble_tv.text, "{}", h.text.as_str()).expect("couldn't write history text to TextView");
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
        log::trace!("shellchat redraw##");
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
    /// change focus
    ChangeFocus,
    /// exit the application
    Quit,
}
//////////////////

// nothing prevents the two from being the same, other than naming conventions
pub(crate) const SERVER_NAME_SHELLCHAT: &str = "_Shell chat application_"; // used internally by xous-names
fn main () -> ! {
    #[cfg(not(feature="ditherpunk"))]
    wrapped_main();

    #[cfg(feature="ditherpunk")]
    let stack_size = 2048 * 1024;
    #[cfg(feature="ditherpunk")]
    std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(wrapped_main)
        .unwrap()
        .join()
        .unwrap()
}
fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed, this is a user app and it's up to the app to decide its policy
    let shch_sid = xns.register_name(SERVER_NAME_SHELLCHAT, None).expect("can't register server");
    //log::trace!("registered with NS -- {:?}", shch_sid);

    #[cfg(feature="tts")]
    let tts = TtsFrontend::new(&xns).unwrap();

    let mut repl = Repl::new(&xns, shch_sid);
    let mut update_repl = false;
    let mut was_callback = false;

    let mut allow_redraw = true;
    let pddb_init_done = Arc::new(AtomicBool::new(false));
    repl.redraw(pddb_init_done.load(Ordering::SeqCst)).ok();
    thread::spawn({
        let pddb_init_done = pddb_init_done.clone();
        let main_conn = xous::connect(shch_sid).unwrap();
        move || {
            let pddb = pddb::Pddb::new();
            pddb.mount_attempted_blocking();
            pddb_init_done.store(true, Ordering::SeqCst);
            xous::send_message(main_conn,
                xous::Message::new_scalar(ShellOpcode::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
            ).ok();
        }
    });

    log::trace!("starting main loop");
    #[cfg(feature = "autobasis-ci")]
    {
        log::info!("starting autobasis CI launcher");
        autobasis_launcher(shch_sid);
    }
    loop {
        let msg = xous::receive_message(shch_sid).unwrap();
        let shell_op: Option::<ShellOpcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("Shellchat got message {:?}", msg);
        match shell_op {
            Some(ShellOpcode::Line) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<xous_ipc::String<4000>, _>().unwrap();
                log::trace!("shell got input line: {}", s.as_str());
                #[cfg(feature="tts")]
                {
                    let mut input = t!("shellchat.input-tts", locales::LANG).to_string();
                    input.push_str(s.as_str());
                    tts.tts_simple(&input).unwrap();
                }
                repl.input(s.as_str()).expect("REPL couldn't accept input string");
                update_repl = true; // set a flag, instead of calling here, so message can drop and calling server is released
                was_callback = false;
            }
            Some(ShellOpcode::Redraw) => {
                if allow_redraw {
                    repl.redraw(pddb_init_done.load(Ordering::SeqCst)).expect("REPL couldn't redraw");
                }
            }
            Some(ShellOpcode::ChangeFocus) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                match new_state {
                    gam::FocusState::Background => {
                        allow_redraw = false;
                    }
                    gam::FocusState::Foreground => {
                        allow_redraw = true;
                    }
                }
            }),
            Some(ShellOpcode::Quit) => {
                log::error!("got Quit");
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
            repl.update(was_callback, pddb_init_done.load(Ordering::SeqCst)).expect("REPL had problems updating");
            update_repl = false;
        }
        log::trace!("reached bottom of main loop");
    }
    // clean up our program
    log::error!("main loop exit, destroying servers");
    xns.unregister_server(shch_sid).unwrap();
    xous::destroy_server(shch_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

#[cfg(feature="autobasis-ci")]
fn autobasis_launcher(sid: xous::SID) {
    let _ = std::thread::spawn({
        let conn = xous::connect(sid).unwrap();
        move || {
            let pddb = pddb::Pddb::new();
            pddb.is_mounted_blocking();
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            tt.sleep_ms(5000).unwrap();
            let cmd = xous_ipc::String::<4000>::from_str("pddb btest");
            let buf = Buffer::into_buf(cmd).unwrap();
            buf.send(conn, ShellOpcode::Line.to_u32().unwrap()).expect("couldn't kick off the CI");
            log::info!("CI run started");
        }
    });
}