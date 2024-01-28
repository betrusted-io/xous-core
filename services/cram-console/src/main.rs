use utralib::generated::*;
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

    let xns = xous_api_names::XousNames::new().unwrap();
    let mut ball = Ball::new(&xns);
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
