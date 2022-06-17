use super::*;
use gam::{UxRegistration, GlyphStyle};
use graphics_server::{Gid, Point, Rectangle, DrawStyle, PixelColor, TextView};
use xous::MessageEnvelope;
use std::fmt::Write;

#[allow(dead_code)]
pub(crate) struct Repl {
    // optional structures that indicate new input to the Repl loop per iteration
    // an input string
    input: Option<String>,
    // messages not handled by the main loop are routed here
    msg: Option<MessageEnvelope>,

    /// the content area
    content: Gid,
    gam: gam::Gam,

    /// screensize of the content area
    screensize: Point,
    margin: Point, // margin to edge of canvas

    // our security token for making changes to our record on the GAM
    token: [u32; 4],
}
impl Repl{
    pub(crate) fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        let gam = gam::Gam::new(xns).expect("can't connect to GAM");

        let app_name_ref = gam::APP_NAME_VAULT;
        let token = gam.register_ux(UxRegistration {
            app_name: xous_ipc::String::<128>::from_str(app_name_ref),
            ux_type: gam::UxType::Chat,
            predictor: Some(xous_ipc::String::<64>::from_str(icontray::SERVER_NAME_ICONTRAY)),
            listener: sid.to_array(), // note disclosure of our SID to the GAM -- the secret is now shared with the GAM!
            redraw_id: VaultOp::Redraw.to_u32().unwrap(),
            gotinput_id: Some(VaultOp::Line.to_u32().unwrap()),
            audioframe_id: None,
            rawkeys_id: None,
            focuschange_id: Some(VaultOp::ChangeFocus.to_u32().unwrap()),
        }).expect("couldn't register Ux context for repl").unwrap();

        let content = gam.request_content_canvas(token).expect("couldn't get content canvas");
        let screensize = gam.get_canvas_bounds(content).expect("couldn't get dimensions of content canvas");
        gam.toggle_menu_mode(token).expect("couldnt't toggle menu mode");

        Repl {
            input: None,
            msg: None,
            content,
            gam,
            screensize,
            margin: Point::new(4, 4),
            token,
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
        self.redraw().unwrap();
    }
    pub(crate) fn redraw(&mut self) -> Result<(), xous::Error> {
        self.clear_area();
        let mut test_text = TextView::new(self.content,
            graphics_server::TextBounds::CenteredTop(
                Rectangle::new(
                    Point::new(self.margin.x, 0),
                    Point::new(self.screensize.x - self.margin.x, 48)
                )
            )
        );
        test_text.draw_border = false;
        test_text.clear_area = true;
        test_text.style = GlyphStyle::Large;
        write!(test_text, "FIDO").ok();
        self.gam.post_textview(&mut test_text).expect("couldn't post test text");

        log::trace!("repl app redraw##");
        self.gam.redraw().expect("couldn't redraw screen");
        Ok(())
    }

    pub(crate) fn raise_menu(&self) {
        self.gam.raise_menu(gam::APP_MENU_0_VAULT).expect("couldn't raise our submenu");
    }
}