use super::*;
use gam::*;
use gam::menu::*;
use gam::menu::api::DrawStyle;

pub(crate) struct Ball {
    gam: gam::Gam,
    xns: xous_names::XousNames,
    gid: Gid,
    screensize: Point,
    // our security token for making changes to our record on the GAM
    token: [u32; 4],
}

impl Ball {
    pub(crate) fn new(sid: xous::SID) -> Self {
        let xns = xous_names::XousNames::new().expect("couldn't connect to Xous Namespace Server");
        let gam = gam::Gam::new(&xns).expect("can't connect to Graphical Abstraction Manager");

        let token = gam.register_ux(UxRegistration {
            app_name: xous_ipc::String::<128>::from_str(gam::APP_NAME_BALL),
            ux_type: gam::UxType::Chat,
            predictor: None,
            listener: sid.to_array(), // note disclosure of our SID to the GAM -- the secret is now shared with the GAM!
            redraw_id: AppOp::Redraw.to_u32().unwrap(),
            gotinput_id: None,
            audioframe_id: None,
            focuschange_id: Some(AppOp::FocusChange.to_u32().unwrap()),
            rawkeys_id: Some(AppOp::Rawkeys.to_u32().unwrap()),
        }).expect("couldn't register Ux context for shellchat");

        let gid = gam.request_content_canvas(token.unwrap()).expect("couldn't get content canvas");
        let screensize = gam.get_canvas_bounds(gid).expect("couldn't get dimensions of content canvas");
        Ball {
            gid,
            xns,
            gam,
            screensize,
            token: token.unwrap(),
        }
    }
    pub(crate) fn redraw(&mut self) {
        // just grief the UX for now
        self.gam.draw_rectangle(self.gid,
            Rectangle::new_coords_with_style(0, 0, self.screensize.x, self.screensize.y,
                DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1))
            ).expect("couldn't draw our rectangle");
    }
    pub(crate) fn rawkeys(&mut self, keys: [char; 4]) {
        log::info!("got rawkey {:?}", keys);
    }
}