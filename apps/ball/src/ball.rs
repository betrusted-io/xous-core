use gam::menu::api::DrawStyle;
use gam::menu::*;
use gam::*;
use locales::t;

use super::*;

#[derive(PartialEq, Eq)]
enum BallMode {
    Random,
    Tilt,
}

const BALL_RADIUS: i16 = 10;
const MOMENTUM_LIMIT: i32 = 8;
const BORDER_WIDTH: i16 = 5;
pub(crate) struct Ball {
    gam: gam::Gam,
    gid: Gid,
    screensize: Point,
    // our security token for making changes to our record on the GAM
    _token: [u32; 4],
    ball: Circle,
    momentum: Point,
    trng: trng::Trng,
    modals: modals::Modals,
    mode: BallMode,
    com: com::Com,
}

impl Ball {
    pub(crate) fn new(sid: xous::SID) -> Self {
        let xns = xous_names::XousNames::new().expect("couldn't connect to Xous Namespace Server");
        let gam = gam::Gam::new(&xns).expect("can't connect to Graphical Abstraction Manager");

        let token = gam
            .register_ux(UxRegistration {
                app_name: xous_ipc::String::<128>::from_str(gam::APP_NAME_BALL),
                ux_type: gam::UxType::Framebuffer,
                predictor: None,
                listener: sid.to_array(), /* note disclosure of our SID to the GAM -- the secret is now
                                           * shared with the GAM! */
                redraw_id: AppOp::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                focuschange_id: Some(AppOp::FocusChange.to_u32().unwrap()),
                rawkeys_id: Some(AppOp::Rawkeys.to_u32().unwrap()),
            })
            .expect("couldn't register Ux context for shellchat");

        let gid = gam.request_content_canvas(token.unwrap()).expect("couldn't get content canvas");
        let screensize = gam.get_canvas_bounds(gid).expect("couldn't get dimensions of content canvas");

        gam.draw_rectangle(
            gid,
            Rectangle::new_coords_with_style(
                0,
                0,
                screensize.x,
                screensize.y,
                DrawStyle::new(PixelColor::Light, PixelColor::Dark, 2),
            ),
        )
        .expect("couldn't draw our rectangle");

        let trng = trng::Trng::new(&xns).unwrap();
        let x = ((trng.get_u32().unwrap() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
        let y = ((trng.get_u32().unwrap() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;

        let mut ball = Circle::new(Point::new(screensize.x / 2, screensize.y / 2), BALL_RADIUS);
        ball.style = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
        gam.draw_circle(gid, ball).expect("couldn't erase ball's previous position");
        let modals = modals::Modals::new(&xns).unwrap();
        let com = com::Com::new(&xns).unwrap();
        Ball {
            gid,
            gam,
            screensize,
            _token: token.unwrap(),
            ball,
            momentum: Point::new(x as i16, y as i16),
            trng,
            modals,
            mode: BallMode::Random,
            com,
        }
    }

    pub(crate) fn update(&mut self) {
        // send a list of objects to draw to the GAM, to avoid race conditions in between operations
        let mut draw_list = GamObjectList::new(self.gid);

        // clear the previous location of the ball
        self.ball.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
        draw_list.push(GamObjectType::Circ(self.ball)).unwrap();
        if self.mode == BallMode::Tilt {
            let (x, y, _z, _id) = self.com.gyro_read_blocking().unwrap();
            let ix = x as i16;
            let iy = y as i16;
            log::debug!("x: {}, y: {}", ix, iy);
            // negative x => tilt to right
            // positive x => tilt to left
            // negative y => tilt toward top
            // positive y => tilt toward bottom
            self.momentum = Point::new(-(ix / 200), iy / 200);
        }
        // update the ball position based on the momentum vector
        self.ball.translate(self.momentum);

        // check if the ball hits the wall, if so, snap its position to the wall
        let mut hit_right = false;
        let mut hit_left = false;
        let mut hit_top = false;
        let mut hit_bott = false;
        if self.ball.center.x + (BALL_RADIUS + BORDER_WIDTH) >= self.screensize.x {
            hit_right = true;
            self.ball.center.x = self.screensize.x - (BALL_RADIUS + BORDER_WIDTH);
        }
        if self.ball.center.x - (BALL_RADIUS + BORDER_WIDTH) <= 0 {
            hit_left = true;
            self.ball.center.x = BALL_RADIUS + BORDER_WIDTH;
        }
        if self.ball.center.y + (BALL_RADIUS + BORDER_WIDTH) >= self.screensize.y {
            hit_bott = true;
            self.ball.center.y = self.screensize.y - (BALL_RADIUS + BORDER_WIDTH);
        }
        if self.ball.center.y - (BALL_RADIUS + BORDER_WIDTH) <= 0 {
            hit_top = true;
            self.ball.center.y = BALL_RADIUS + BORDER_WIDTH;
        }

        if (hit_right || hit_left || hit_bott || hit_top) && (self.mode == BallMode::Random) {
            let mut x = ((self.trng.get_u32().unwrap() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
            let mut y = ((self.trng.get_u32().unwrap() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
            if hit_right {
                x = -x.abs();
            }
            if hit_left {
                x = x.abs();
            }
            if hit_top {
                y = y.abs();
            }
            if hit_bott {
                y = -y.abs();
            }
            self.momentum = Point::new(x as i16, y as i16);
        }

        // draw the new location for the ball
        self.ball.style = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
        draw_list.push(GamObjectType::Circ(self.ball)).unwrap();
        self.gam.draw_list(draw_list).expect("couldn't execute draw list");
        log::trace!("ball app redraw##");
        self.gam.redraw().unwrap();
    }

    pub(crate) fn focus(&mut self) {
        // draw the background entirely
        self.gam
            .draw_rectangle(
                self.gid,
                Rectangle::new_coords_with_style(
                    0,
                    0,
                    self.screensize.x,
                    self.screensize.y,
                    DrawStyle::new(PixelColor::Light, PixelColor::Dark, BORDER_WIDTH),
                ),
            )
            .expect("couldn't draw our rectangle");
    }

    pub(crate) fn rawkeys(&mut self, keys: [char; 4]) {
        log::debug!("got rawkey {:?}", keys); // you could use the raw keypresses, but modals are easier...
        let mut note = String::new();
        use std::fmt::Write;
        write!(
            note,
            "{}'{}'.\n\n{}",
            t!("ballapp.notification_a", locales::LANG),
            keys[0],
            t!("ballapp.notification_b", locales::LANG),
        )
        .unwrap();
        self.modals.show_notification(&note, None).unwrap();
        self.modals.add_list_item(t!("ballapp.random", locales::LANG)).unwrap();
        self.modals.add_list_item(t!("ballapp.tilt", locales::LANG)).unwrap();
        let mode = self.modals.get_radiobutton(t!("ballapp.mode_prompt", locales::LANG)).unwrap();
        if mode == t!("ballapp.random", locales::LANG) {
            self.mode = BallMode::Random;
        } else if mode == t!("ballapp.tilt", locales::LANG) {
            self.mode = BallMode::Tilt;
        } else {
            log::warn!("got an unexpected response from the radio button function: {}", mode);
        }
    }
}

pub(crate) fn ball_pump_thread(cid_to_main: xous::CID, pump_sid: xous::SID) {
    let _ = std::thread::spawn({
        let cid_to_main = cid_to_main; // kind of redundant but I like making the closure captures explicit
        let sid = pump_sid;
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let cid_to_self = xous::connect(sid).unwrap();
            let mut run = true;
            loop {
                // this blocks the process until a message is received, descheduling it from the run queue
                let msg = xous::receive_message(sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(PumpOp::Run) => {
                        run = true;
                        xous::send_message(
                            cid_to_self,
                            Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't pump the main loop event thread");
                    }
                    Some(PumpOp::Stop) => run = false,
                    Some(PumpOp::Pump) => {
                        xous::send_message(
                            cid_to_main,
                            Message::new_blocking_scalar(AppOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .expect("couldn't pump the main loop event thread");
                        if run {
                            tt.sleep_ms(BALL_UPDATE_RATE_MS).unwrap();
                            xous::send_message(
                                cid_to_self,
                                Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                            )
                            .expect("couldn't pump the main loop event thread");
                        }
                    }
                    Some(PumpOp::Quit) => {
                        xous::return_scalar(msg.sender, 1).expect("couldn't ack the quit message");
                        break;
                    }
                    _ => log::error!("Got unrecognized message: {:?}", msg),
                }
            }
            xous::destroy_server(sid).ok();
        }
    });
}
