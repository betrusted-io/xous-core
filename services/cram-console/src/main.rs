use std::fmt::Write;
use std::thread;

use graphics_server::api::GlyphStyle;
use graphics_server::Gid;
use graphics_server::*;
use locales::t;
use num_traits::*;
use utralib::generated::*;

const SERVER_NAME_STATUS_GID: &str = "_Status bar GID receiver_";
const SERVER_NAME_STATUS: &str = "_Status_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum StatusOpcode {
    Quit,
}

// TODO:
//   - create_server_with_address can OOM without recovery -- figure out how to fix this.
//   - clean up the mess here

fn main() {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    #[cfg(feature = "hwsim")]
    let csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::main::HW_MAIN_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map Core Control CSR range");
    #[cfg(feature = "hwsim")]
    let mut core_csr = Some(CSR::new(csr.as_mut_ptr() as *mut u32));

    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::REPORT_REPORT, 0x600d_0000);

    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::REPORT_REPORT, 0xa51d_0000);
    let coreuser_csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::coreuser::HW_COREUSER_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map Core User CSR range");
    let mut coreuser = CSR::new(coreuser_csr.as_mut_ptr() as *mut u32);
    // first, clear the ASID table to 0
    for asid in 0..512 {
        coreuser.wo(
            utra::coreuser::SET_ASID,
            coreuser.ms(utra::coreuser::SET_ASID_ASID, asid)
                | coreuser.ms(utra::coreuser::SET_ASID_TRUSTED, 0),
        );
    }
    // set my PID to trusted
    coreuser.wo(
        utra::coreuser::SET_ASID,
        coreuser.ms(utra::coreuser::SET_ASID_ASID, xous::process::id() as u32)
            | coreuser.ms(utra::coreuser::SET_ASID_TRUSTED, 1),
    );
    // set the required `mpp` state to user code (mpp == 0)
    coreuser.wfo(utra::coreuser::SET_PRIVILEGE_MPP, 0);
    // turn on the coreuser computation
    coreuser.wo(
        utra::coreuser::CONTROL,
        coreuser.ms(utra::coreuser::CONTROL_ASID, 1)
            | coreuser.ms(utra::coreuser::CONTROL_ENABLE, 1)
            | coreuser.ms(utra::coreuser::CONTROL_PRIVILEGE, 1),
    );
    // turn off coreuser control updates
    coreuser.wo(utra::coreuser::PROTECT, 1);
    #[cfg(feature = "hwsim")]
    core_csr.wfo(utra::main::REPORT_REPORT, 0xa51d_600d);

    log::info!("my PID is {}", xous::process::id());

    #[cfg(feature = "pio-test")]
    {
        log::info!("running PIO tests");
        xous_pio::pio_tests::pio_tests();
        log::info!("resuming console tests");
    }

    #[cfg(feature = "pl230-test")]
    {
        log::info!("running PL230 tests");
        xous_pl230::pl230_tests::pl230_tests();
        log::info!("resuming console tests");
    }

    #[cfg(feature = "message-passing-test")]
    {
        let tt = xous_api_ticktimer::Ticktimer::new().unwrap();
        let mut total = 0;
        let mut iter = 0;
        log::info!("running message passing test");
        loop {
            // this conjures a scalar message
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, 0x1111_0000 + iter);
            let now = tt.elapsed_ms();
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, 0x2222_0000 + iter);
            total += now;

            if iter >= 8 && iter < 12 {
                #[cfg(feature = "hwsim")]
                core_csr.wfo(utra::main::REPORT_REPORT, 0x5133_D001);
                tt.sleep_ms(1).ok();
            } else if iter >= 12 && iter < 13 {
                #[cfg(feature = "hwsim")]
                core_csr.wfo(utra::main::REPORT_REPORT, 0x5133_D002);
                tt.sleep_ms(2).ok();
            } else if iter >= 13 && iter < 14 {
                #[cfg(feature = "hwsim")]
                core_csr.wfo(utra::main::REPORT_REPORT, 0x5133_D002);
                tt.sleep_ms(3).ok();
            } else if iter >= 14 {
                break;
            }

            // something lame to just conjure a memory message
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, 0x3333_0000 + iter);
            let version = tt.get_version();
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, 0x4444_0000 + iter);
            total += version.len() as u64;
            iter += 1;
            #[cfg(feature = "hwsim")]
            core_csr.wfo(utra::main::REPORT_REPORT, now as u32);
            log::info!("message passing test progress: {}ms", tt.elapsed_ms());
        }
        #[cfg(feature = "hwsim")]
        core_csr.wfo(utra::main::REPORT_REPORT, 0x6969_6969);
        println!("Elapsed: {}", total);
        #[cfg(feature = "hwsim")]
        core_csr.wfo(utra::main::REPORT_REPORT, 0x600d_c0de);

        #[cfg(feature = "hwsim")]
        core_csr.wfo(utra::main::SUCCESS_SUCCESS, 1);
        tt.sleep_ms(4).ok();
        #[cfg(feature = "hwsim")]
        core_csr.wfo(utra::main::DONE_DONE, 1); // this should stop the simulation
        log::info!("message passing test done at {}ms!", tt.elapsed_ms());
    }

    thread::spawn(move || {
        let mut count = 0;
        loop {
            log::info!("Still alive! #{}", count);
            count += 1;
            std::thread::sleep(std::time::Duration::from_millis(2000));
        }
    });

    let xns = xous_api_names::XousNames::new().unwrap();

    let status_gam_getter =
        xns.register_name(SERVER_NAME_STATUS_GID, Some(1)).expect("can't register server");
    let mut canvas_gid: [u32; 4] = [0; 4];
    // wait until we're assigned a GID -- this is a one-time message from the GAM
    let msg = xous::receive_message(status_gam_getter).unwrap();
    log::trace!("GID assignment message: {:?}", msg);
    xous::msg_scalar_unpack!(msg, g0, g1, g2, g3, {
        canvas_gid[0] = g0 as u32;
        canvas_gid[1] = g1 as u32;
        canvas_gid[2] = g2 as u32;
        canvas_gid[3] = g3 as u32;
    });
    match xns.unregister_server(status_gam_getter) {
        Err(e) => {
            log::error!("couldn't unregister getter server: {:?}", e);
        }
        _ => {}
    }
    xous::destroy_server(status_gam_getter).unwrap();

    let status_gid: Gid = Gid::new(canvas_gid);
    // Expected connections:
    //   - from keyboard
    //   - from USB HID
    let status_sid = xns.register_name(SERVER_NAME_STATUS, Some(2)).unwrap();
    // create a connection for callback hooks
    let cb_cid = xous::connect(status_sid).unwrap();

    let gam = gam::Gam::new(&xns).expect("|status: can't connect to GAM");
    // screensize is controlled by the GAM, it's set in main.rs near the top
    let screensize = gam.get_canvas_bounds(status_gid).expect("|status: Couldn't get canvas size");

    // build security status textview
    let mut security_tv = TextView::new(
        status_gid,
        TextBounds::BoundingBox(Rectangle::new(
            Point::new(0, screensize.y / 2 + 1),
            Point::new(screensize.x, screensize.y),
        )),
    );

    security_tv.style = GlyphStyle::Tall; // was: Regular, but not available on this target
    security_tv.draw_border = false;
    security_tv.margin = Point::new(0, 0);
    security_tv.token = gam.claim_token(gam::STATUS_BAR_NAME).expect("couldn't request token"); // this is a shared magic word to identify this process
    security_tv.clear_area = true;
    security_tv.invert = true;
    write!(&mut security_tv, "{}", t!("secnote.startup", locales::LANG)).unwrap();
    gam.post_textview(&mut security_tv).unwrap();
    gam.draw_line(
        status_gid,
        Line::new_with_style(
            Point::new(0, screensize.y),
            screensize,
            DrawStyle::new(PixelColor::Light, PixelColor::Light, 1),
        ),
    )
    .unwrap();
    log::trace!("status redraw## initial");
    gam.redraw().unwrap(); // initial boot redraw

    // ------------------ start a 'gutter' thread to handle incoming events while we go through the
    // boot/autoupdate process
    let gutter = thread::spawn({
        let gutter_sid = status_sid.clone();
        move || {
            loop {
                let msg = xous::receive_message(gutter_sid).unwrap();
                let opcode: Option<StatusOpcode> = FromPrimitive::from_usize(msg.body.id());
                log::info!("Guttering {:?}", opcode);
                match opcode {
                    Some(StatusOpcode::Quit) => {
                        xous::return_scalar(msg.sender, 1).ok();
                        break;
                    }
                    _ => (), // ignore everything else.
                }
            }
        }
    });

    log::info!("starting modal");
    let modals = modals::Modals::new(&xns).unwrap();
    modals.show_notification("This is a test", Some("This is a test")).ok();
    log::info!("exiting modal");

    let mut ball = Ball::new(&xns);
    log::info!("starting ball");
    loop {
        ball.update();
        /* // for testing full-frame graphics drawing
            ball.draw_boot();
            tt.sleep_ms(100).ok();
        */
    }
}

fn crappy_rng_u32() -> u32 { xous::create_server_id().unwrap().to_u32().0 }

const BALL_RADIUS: i16 = 10;
const MOMENTUM_LIMIT: i32 = 8;
const BORDER_WIDTH: i16 = 5;
use graphics_server::{Circle, ClipObjectList, ClipObjectType, DrawStyle, PixelColor, Point, Rectangle};

struct Ball {
    gfx: graphics_server::Gfx,
    screensize: Point,
    ball: Circle,
    momentum: Point,
    clip: Rectangle,
}
impl Ball {
    pub fn new(xns: &xous_api_names::XousNames) -> Ball {
        let gfx = graphics_server::Gfx::new(xns).unwrap();
        gfx.draw_boot_logo().unwrap();

        let screensize = gfx.screen_size().unwrap();
        let mut ball = Circle::new(Point::new(screensize.x / 2, screensize.y / 2), BALL_RADIUS);
        ball.style = DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1);
        let clip = Rectangle::new(Point::new(0, 0), screensize);
        gfx.draw_circle(ball).unwrap();
        let x = ((crappy_rng_u32() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
        let y = ((crappy_rng_u32() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
        Ball { gfx, screensize, ball, momentum: Point::new(x as i16, y as i16), clip }
    }

    #[allow(dead_code)]
    pub fn draw_boot(&self) { self.gfx.draw_boot_logo().ok(); }

    pub fn update(&mut self) {
        /* // for testing fonts, etc.
        use std::fmt::Write;
        let mut tv = graphics_server::TextView::new(
            graphics_server::Gid::new([0, 0, 0, 0]),
            graphics_server::TextBounds::BoundingBox(self.clip),
        );
        tv.clip_rect = Some(self.clip);
        tv.set_dry_run(false);
        tv.set_op(graphics_server::TextOp::Render);
        tv.style = graphics_server::api::GlyphStyle::Tall;
        write!(tv.text, "hello world! 😀").ok();
        self.gfx.draw_textview(&mut tv).ok();
        */
        let mut draw_list = ClipObjectList::default();

        // clear the previous location of the ball
        self.ball.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
        draw_list.push(ClipObjectType::Circ(self.ball), self.clip).unwrap();

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

        if hit_right || hit_left || hit_bott || hit_top {
            let mut x = ((crappy_rng_u32() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
            let mut y = ((crappy_rng_u32() / 2) as i32) % (MOMENTUM_LIMIT * 2) - MOMENTUM_LIMIT;
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
        draw_list.push(ClipObjectType::Circ(self.ball), self.clip).unwrap();

        self.gfx.draw_object_list_clipped(draw_list).ok();
        self.gfx.flush().unwrap();
    }
}
