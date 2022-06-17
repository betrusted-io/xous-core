use crate::*;
use gam::{UxRegistration, GlyphStyle};
use graphics_server::{Gid, Point, Rectangle, DrawStyle, PixelColor, TextView};
use xous::MessageEnvelope;
use std::fmt::Write;

#[allow(dead_code)]
pub(crate) struct VaultUx {
    // optional structures that indicate new input to the VaultUx loop per iteration
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

    // current operation mode
    mode: VaultMode,
}

const TITLE_HEIGHT: i16 = 32;

impl VaultUx{
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

        VaultUx {
            input: None,
            msg: None,
            content,
            gam,
            screensize,
            margin: Point::new(4, 4),
            token,
            mode: VaultMode::Fido,
        }
    }
    pub(crate) fn set_mode(&mut self, mode: VaultMode) { self.mode = mode; }
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

        // ---- draw title area ----
        let mut title_text = TextView::new(self.content,
            graphics_server::TextBounds::CenteredTop(
                Rectangle::new(
                    Point::new(self.margin.x, 0),
                    Point::new(self.screensize.x - self.margin.x, TITLE_HEIGHT)
                )
            )
        );
        title_text.draw_border = true;
        title_text.rounded_border = Some(8);
        title_text.clear_area = true;
        title_text.style = GlyphStyle::Large;
        match self.mode {
            VaultMode::Fido => write!(title_text, "FIDO").ok(),
            VaultMode::Totp => write!(title_text, "⏳1234").ok(),
            VaultMode::Password => write!(title_text, "🔐****").ok(),
        };
        self.gam.post_textview(&mut title_text).expect("couldn't post title");

        // ---- draw list body area ----
        let available_height = self.screensize.y - TITLE_HEIGHT;
        let style = GlyphStyle::Large;
        let glyph_height = self.gam.glyph_height_hint(style).unwrap();
        let box_height = (glyph_height * 2) as i16;
        let line_count = available_height / box_height;

        let mut test_list = Vec::new();
        for i in 0..16 {
            let test_string = format!("Test item {}\nMore info about the item.", i);
            test_list.push(test_string);
        }

        let mut insert_at = TITLE_HEIGHT;
        for item in test_list {
            if insert_at > self.screensize.y - box_height {
                break;
            }
            let mut box_text = TextView::new(self.content,
                graphics_server::TextBounds::BoundingBox(
                    Rectangle::new(
                        Point::new(self.margin.x, insert_at),
                        Point::new(self.screensize.x - self.margin.x, insert_at + box_height)
                    )
                )
            );
            box_text.draw_border = true;
            box_text.rounded_border = None;
            box_text.clear_area = true;
            box_text.style = style;
            write!(box_text, "{}", item).ok();
            self.gam.post_textview(&mut box_text).expect("couldn't post list item");

            insert_at += box_height;
        }

        log::trace!("vault app redraw##");
        self.gam.redraw().expect("couldn't redraw screen");
        Ok(())
    }

    pub(crate) fn raise_menu(&self) {
        self.gam.raise_menu(gam::APP_MENU_0_VAULT).expect("couldn't raise our submenu");
    }
}