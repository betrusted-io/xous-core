use super::*;
use gam::UxRegistration;
use graphics_server::{Gid, Point, Rectangle, TextBounds, TextView, DrawStyle, PixelColor};
use graphics_server::api::GlyphStyle;
use xous::MessageEnvelope;
use core::fmt::Write;
use locales::t;

#[allow(dead_code)]
pub(crate) struct Repl {
    // optional structures that indicate new input to the Repl loop per iteration
    // an input string
    input: Option<String>,
    // messages from other servers
    msg: Option<MessageEnvelope>,

    // record our input history
    content: Gid,
    gam: gam::Gam,

    // variables that define our graphical attributes
    screensize: Point,
    margin: Point, // margin to edge of canvas

    // our security token for making changes to our record on the GAM
    token: [u32; 4],
}
impl Repl{
    pub(crate) fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        let gam = gam::Gam::new(xns).expect("can't connect to GAM");

        let token = gam.register_ux(UxRegistration {
            app_name: xous_ipc::String::<128>::from_str(gam::APP_NAME_VAULT),
            ux_type: gam::UxType::Chat,
            predictor: Some(xous_ipc::String::<64>::from_str(ime_plugin_shell::SERVER_NAME_IME_PLUGIN_SHELL)),
            listener: sid.to_array(), // note disclosure of our SID to the GAM -- the secret is now shared with the GAM!
            redraw_id: VaultOp::Redraw.to_u32().unwrap(),
            gotinput_id: Some(VaultOp::Line.to_u32().unwrap()),
            audioframe_id: None,
            rawkeys_id: None,
            focuschange_id: Some(VaultOp::ChangeFocus.to_u32().unwrap()),
        }).expect("couldn't register Ux context for repl");

        let content = gam.request_content_canvas(token.unwrap()).expect("couldn't get content canvas");
        let screensize = gam.get_canvas_bounds(content).expect("couldn't get dimensions of content canvas");
        Repl {
            input: None,
            msg: None,
            content,
            gam,
            screensize,
            margin: Point::new(4, 4),
            token: token.unwrap(),
        }
    }

    /// accept a new input string
    pub(crate) fn input(&mut self, line: &str) -> Result<(), xous::Error> {
        self.input = Some(String::from(line));

        Ok(())
    }

    pub(crate) fn msg(&mut self, message: MessageEnvelope) {
        self.msg = Some(message);
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
    // dummy function for now - but this is where the action happens when input events come
    pub (crate) fn update(&mut self, _was_callback: bool) {
        self.redraw();
    }
    pub(crate) fn redraw(&mut self) -> Result<(), xous::Error> {
        self.clear_area();

        log::trace!("repl app redraw##");
        self.gam.redraw().expect("couldn't redraw screen");
        Ok(())
    }
}