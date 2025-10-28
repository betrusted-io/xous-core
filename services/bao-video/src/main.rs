// Maintainer's note: more character sets are added to baosec targets by modifying
// the character map resolution macro in libs/blitstr2/src/style_macro.rs/english_rules
// Including a resolver to a given character map also pulls the font data into the
// bao-video binary, increasing its size.

use bao1x_hal_service::Hal;
use ux_api::minigfx::*;

mod gfx;
#[cfg(feature = "board-baosec")]
mod panic;
mod qr;
#[cfg(feature = "gfx-testing")]
mod testing;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use bao1x_api::*;
#[cfg(feature = "hosted-baosec")]
use bao1x_emu::{
    camera::Gc2145,
    display::{MainThreadToken, Mono, Oled128x128, claim_main_thread},
    i2c::I2c,
    udma::UdmaGlobal,
};
// breadcrumb to future self:
//   - For GC0308 drivers, look in code/esp32-camera for sample code/constants
#[cfg(feature = "board-baosec")]
use bao1x_hal::{
    gc2145::Gc2145,
    sh1107::{MainThreadToken, Mono, Oled128x128, claim_main_thread},
};
#[cfg(feature = "board-baosec")]
use bao1x_hal_service::{I2c, UdmaGlobal};
#[cfg(feature = "b64-export")]
use base64::{Engine as _, engine::general_purpose};
#[cfg(feature = "board-baosec")]
use num_traits::*;
#[cfg(not(feature = "hosted-baosec"))]
use utralib::utra;
use ux_api::minigfx::{self, FrameBuffer};
use ux_api::service::api::*;
use xous::sender::Sender;

// Scope of this crate: *No calls to modals* this can create dependency lockups.
//
// bao-video contains the platform-specific drivers for the baosec platform that pertain
// to video: both the capture of video, as well as any operations involving drawing to
// the display (rendering graphics primitives, etc).
//
// Note that explicitly out of scope are the higher-level API calls for UI management, e.g.
// creation of modals and managing draw lists. Only the hardware renderers should be implemented
// in this crate. Think of it like a kernel module that handles a video subsystem, where both
// camera and display are co-located in the same module for fast data sharing (keep in mind
// this is a microkernel, so we don't have a monolith data space like Linux: all drivers are
// in their own process space unless explicitly co-located).
//
// It also pulls in QR code processing for performance reasons - by keeping the QR code
// processing in the process space of the camera, we can avoid an expensive memcopy between
// process spaces and improve the responsiveness of the feedback loop while QR searching happens.

pub const IMAGE_WIDTH: usize = 256;
pub const IMAGE_HEIGHT: usize = 240;

// Next steps for performance improvement:
//
// Improve qr::mapping -> point_from_hv_lines such that we're not just deriving the HV
// lines from the the edges of the finder regions, we're also using the very edge of
// the whole QR code itself to guide the line. This will improve the intersection point
// so that we can accurately hit the "fourth corner". At the moment it's sort of a
// luck of the draw if the interpolation hits exactly right, or if we're roughly a module
// off from ideal, which causes the data around that point to be interpreted incorrectly.

#[cfg(feature = "b64-export")]
#[allow(dead_code)]
fn encode_base64(input: &[u8]) -> String { general_purpose::STANDARD.encode(input) }

/// This converts a frame of `[u8]` grayscale pixels that may be larger than the native
/// frame buffer resolution into a black and white bitmap.
pub fn blit_to_display(display: &mut Oled128x128, frame: &[u8], display_cleared: bool, bw_thresh: &mut u8) {
    let mut sum: u32 = 0;
    let mut count: u32 = 0;
    for (y, row) in frame.chunks(IMAGE_WIDTH).enumerate() {
        if y & 1 == 0 {
            // skip every other line
            for (x, &pixval) in row.iter().enumerate() {
                if x & 1 == 0 {
                    // skip every other pixel
                    if y < display.dimensions().x as usize * 2
                        && x < display.dimensions().y as usize * 2 - (gfx::CHAR_HEIGHT as usize + 1) * 2
                    {
                        let luminance = pixval & 0xff;
                        sum += luminance as u32;
                        count += 1;
                        if luminance > *bw_thresh {
                            display.put_pixel(Point::new(y as isize / 2, x as isize / 2), Mono::White.into());
                        } else {
                            // optimization to avoid some computation if we're blitting to an already-black
                            // buffer
                            if !display_cleared {
                                display.put_pixel(
                                    Point::new(y as isize / 2, x as isize / 2),
                                    Mono::Black.into(),
                                );
                            }
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    }
    *bw_thresh = (sum / count) as u8;
}

#[repr(align(32))]
#[cfg(not(feature = "hosted-baosec"))]
struct CamIrq {
    csr: utralib::CSR<u32>,
    cid: u32,
}

#[cfg(not(feature = "hosted-baosec"))]
fn handle_irq(_irq_no: usize, arg: *mut usize) {
    let cam_irq: &mut CamIrq = unsafe { &mut *(arg as *mut CamIrq) };
    // clear the pending interrupt - assume it's just the camera for now
    let pending = cam_irq.csr.r(utra::irqarray8::EV_PENDING);
    cam_irq.csr.wo(utra::irqarray8::EV_PENDING, pending);

    // activate the handler
    xous::try_send_message(
        cam_irq.cid,
        xous::Message::new_scalar(GfxOpcode::CamIrq.to_usize().unwrap(), pending as usize, 0, 0, 0),
    )
    .ok();
}

fn main() -> ! {
    let stack_size = 2 * 1024 * 1024;
    claim_main_thread(move |main_thread_token| {
        std::thread::Builder::new()
            .stack_size(stack_size)
            .spawn(move || wrapped_main(main_thread_token))
            .unwrap()
            .join()
            .unwrap()
    })
}

pub fn wrapped_main(main_thread_token: MainThreadToken) -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // ---- Xous setup
    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(SERVER_NAME_GFX, None).expect("can't register server");

    let tt = ticktimer::Ticktimer::new().unwrap();
    // wait for other servers to start
    tt.sleep_ms(100).ok();

    // ---- basic hardware setup
    let iox = IoxHal::new();
    let udma_global = UdmaGlobal::new();
    let mut i2c = I2c::new();
    let hal = Hal::new();

    let mut display = Oled128x128::new(main_thread_token, bao1x_api::PERCLK, &iox, &udma_global);
    display.init();
    display.clear();
    display.draw();

    // ---- panic handler - set up early so we can see panics quickly
    // install the graphical panic handler. It won't catch really early panics, or panics in this crate,
    // but it'll do the job 90% of the time and it's way better than having none at all.
    let is_panic = Arc::new(AtomicBool::new(false));

    // ---- boot logo
    display.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
    display.redraw();

    // This is safe because the SPIM is finished with initialization, and the handler is
    // Mutex-protected.
    #[cfg(feature = "board-baosec")]
    {
        let panic_display = unsafe { display.to_raw_parts() };
        panic::panic_handler_thread(is_panic.clone(), panic_display);
    }

    // ---- camera initialization
    #[cfg(not(feature = "hosted-baosec"))]
    {
        // wait for other inits to finish so we can do this roughly atomically
        tt.sleep_ms(1000).ok();

        // setup camera power
        match bao1x_hal::axp2101::Axp2101::new(&mut i2c) {
            Ok(mut pmic) => {
                pmic.set_ldo(&mut i2c, Some(2.85), bao1x_hal::axp2101::WhichLdo::Bldo2).unwrap();
                pmic.set_dcdc(&mut i2c, Some((1.8, false)), bao1x_hal::axp2101::WhichDcDc::Dcdc5).unwrap();
            }
            Err(e) => {
                log::error!("Couldn't setup regulators for camera, camera will be non-functional: {:?}", e);
            }
        };

        // setup camera clock
        iox.setup_pin(IoxPort::PF, 9, Some(IoxDir::Input), Some(IoxFunction::Gpio), None, None, None, None);
        iox.setup_pin(
            IoxPort::PA,
            0,
            Some(IoxDir::Output),
            Some(IoxFunction::Gpio),
            None,
            None,
            Some(IoxEnable::Disable),
            Some(IoxDriveStrength::Drive8mA),
        );
        iox.setup_pin(
            IoxPort::PA,
            0,
            Some(IoxDir::Output),
            Some(IoxFunction::AF3),
            None,
            None,
            Some(IoxEnable::Disable),
            Some(IoxDriveStrength::Drive8mA),
        );
        let timer_range = xous::map_memory(
            xous::MemoryAddress::new(utra::pwm::HW_PWM_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map PWM range");
        let mut timer = utralib::CSR::new(timer_range.as_ptr() as usize as *mut u32);
        timer.wo(utra::pwm::REG_CH_EN, 1);
        timer.rmwf(utra::pwm::REG_TIM0_CFG_R_TIMER0_SAW, 1);
        timer.rmwf(utra::pwm::REG_TIM0_CH0_TH_R_TIMER0_CH0_TH, 0);
        timer.rmwf(utra::pwm::REG_TIM0_CH0_TH_R_TIMER0_CH0_MODE, 3);
        let pwm = timer_range.as_mut_ptr() as *mut u32;
        unsafe { pwm.add(2).write_volatile(0) }; // for some reason the register extraction didn't get this register...
        timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 1);
        log::info!("PWM running on PA0");
        /* // register debug
        for i in 0..12 {
            println!("0x{:2x}: 0x{:08x}", i, unsafe { pwm.add(i).read_volatile() })
        }
        println!("0x{:2x}: 0x{:08x}", 65, unsafe { pwm.add(65).read_volatile() });
        */

        // setup camera pins
        let (cam_pdwn_bnk, cam_pdwn_pin) = bao1x_hal::board::setup_camera_pins(&iox);
        // disable camera powerdown
        iox.set_gpio_pin_value(cam_pdwn_bnk, cam_pdwn_pin, IoxValue::Low);
    }
    udma_global.udma_clock_config(PeriphId::Cam, true);
    // this is safe because we turned on the clocks before calling it
    let mut cam = unsafe { Gc2145::new().expect("couldn't allocate camera") };

    tt.sleep_ms(100).ok();

    let (pid, mid) = cam.read_id(&mut i2c);
    log::info!("Camera pid {:x}, mid {:x}", pid, mid);
    cam.init(&mut i2c, bao1x_api::camera::Resolution::Res320x240);
    tt.sleep_ms(1).ok();

    let (cols, _rows) = cam.resolution();
    let border = (cols - IMAGE_WIDTH) / 2;
    cam.set_slicing((border, 0), (cols - border, IMAGE_HEIGHT));
    log::info!("320x240 resolution setup with 256x240 slicing");

    #[cfg(feature = "decongest-udma")]
    log::info!("Decongest udma option enabled.");

    #[cfg(not(feature = "hosted-baosec"))]
    let cid = xous::connect(sid).unwrap(); // self-connection always succeeds

    // ---- register interrupt handler
    #[cfg(not(feature = "hosted-baosec"))]
    let cam_irq; // this binding has to out-live the temporaries below
    #[cfg(not(feature = "hosted-baosec"))]
    {
        let irq = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::irqarray8::HW_IRQARRAY8_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map IRQ CSR range");
        let mut irq_csr = utralib::CSR::new(irq.as_mut_ptr() as *mut u32);
        irq_csr.wo(utra::irqarray8::EV_PENDING, 0xFFFF); // clear any pending interrupts

        cam_irq = CamIrq { csr: utralib::CSR::new(irq.as_mut_ptr() as *mut u32), cid };
        let irq_arg = &cam_irq as *const CamIrq as *mut usize;
        log::info!("irq_arg: {:x}", irq_arg as usize);
        xous::claim_interrupt(utra::irqarray8::IRQARRAY8_IRQ, handle_irq, irq_arg)
            .expect("couldn't claim IRQ8");
        // enable camera Rx IRQ
        irq_csr.wfo(utra::irqarray8::EV_ENABLE_CAM_RX, 1);
    }

    // ---- main loop variables
    let screen_clip = Rectangle::new(Point::new(0, 0), display.screen_size());

    // this will kick the hardware into the QR code scanning routine automatically. Eventually
    // this needs to be turned into a call that can invoke and abort the QR code scanning.
    #[cfg(feature = "autotest")]
    {
        log::info!("initiating auto test");
        cam.capture_async();
        // cam_irq serves as a preemption timer source, every time it fires a different
        // task can run after cam_irq is handled.
        hal.set_preemption(false);
    }
    #[cfg(feature = "no-gam")]
    let modals = modals::Modals::new(&xns).unwrap();
    let mut modal_queue = VecDeque::<Sender>::new();
    let mut frames = 0;
    let mut frame = [0u8; IMAGE_WIDTH * IMAGE_HEIGHT];
    let mut decode_success;
    let mut msg_opt = None;
    #[cfg(feature = "gfx-testing")]
    testing::tests();
    let mut bw_thresh: u8 = 128;
    loop {
        if !is_panic.load(Ordering::Relaxed) {
            xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
            let msg = msg_opt.as_mut().unwrap();
            let opcode =
                num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(GfxOpcode::InvalidCall);
            log::debug!("{:?}", opcode);
            match opcode {
                GfxOpcode::CamIrq => {
                    #[cfg(not(feature = "decongest-udma"))]
                    cam.capture_async();

                    // copy the camera data to our FB
                    let fb: &[u32] = cam.rx_buf();
                    // fb is an array of IMAGE_WIDTH x IMAGE_HEIGHT x u16
                    // frame is an array of IMAGE_WIDTH x IMAGE_HEIGHT x u8
                    // Take only the "Y" channel out of the fb array and write it to frame, but do it
                    // such that we are fetching a u32 each read from fb as this matches the native
                    // width of the bus (because fb is non-cacheable reading u16 ends up fetching the
                    // same word twice, then masking it at the CPU side in hardware). Also, the fb
                    // is slow to access relative to main memory.
                    //
                    // Also, commit the data to `frame` in inverse line order, e.g. flip the image
                    // vertically.
                    for (y_src, line) in fb.chunks(IMAGE_WIDTH / 2).enumerate() {
                        for (x_src, &u32src) in line.iter().enumerate() {
                            frame[y_src * IMAGE_WIDTH + 2 * x_src] = ((u32src >> 8) & 0xff) as u8;
                            frame[y_src * IMAGE_WIDTH + 2 * x_src + 1] = ((u32src >> 24) & 0xff) as u8;
                        }
                    }
                    frames += 1;

                    let mut candidates = Vec::<Point>::new();
                    decode_success = false;
                    log::debug!("------------- SEARCH {} -----------", frames);
                    let _finder_width =
                        qr::find_finders(&mut candidates, &frame, bw_thresh, IMAGE_WIDTH) as isize;
                    // blit raw camera fb to display
                    blit_to_display(&mut display, &frame, true, &mut bw_thresh);
                    if candidates.len() == 3 {
                        gfx::msg(
                            &mut display,
                            "Decoding...",
                            Point::new(0, 0),
                            Mono::White.into(),
                            Mono::Black.into(),
                        );
                        let mut img =
                            rqrr::PreparedImage::prepare_from_greyscale(IMAGE_WIDTH, IMAGE_HEIGHT, |x, y| {
                                frame[y * IMAGE_WIDTH + x]
                            });
                        let grids = img.detect_grids();
                        if grids.len() == 1 {
                            match grids[0].decode() {
                                Ok((meta, content)) => {
                                    log::info!("meta: {:?}", meta);
                                    log::info!("************ {} ***********", content);
                                    decode_success = true;
                                    gfx::msg(
                                        &mut display,
                                        &format!("{:?}", meta),
                                        Point::new(0, 0),
                                        Mono::White.into(),
                                        Mono::Black.into(),
                                    );
                                    gfx::msg(
                                        &mut display,
                                        &format!("{:?}", content),
                                        Point::new(0, 64),
                                        Mono::White.into(),
                                        Mono::Black.into(),
                                    );
                                }
                                Err(e) => {
                                    log::info!("{:?}", e);
                                    gfx::msg(
                                        &mut display,
                                        &format!("{:?}", e),
                                        Point::new(0, 0),
                                        Mono::White.into(),
                                        Mono::Black.into(),
                                    );
                                }
                            }
                        }
                    } else {
                        gfx::msg(
                            &mut display,
                            "Scan QR code...",
                            Point::new(0, 0),
                            Mono::White.into(),
                            Mono::Black.into(),
                        );
                    }

                    display.draw();
                    if decode_success {
                        tt.sleep_ms(1500).ok();
                    }

                    // clear the front buffer
                    display.clear();

                    // re-initiate the capture. This is done at the bottom of the loop because UDMA
                    // congestion leads to system instability. When this problem is solved, we would
                    // actually want to re-initiate the capture immediately (or leave it on continuous mode)
                    // to allow capture to process concurrently with the code. However, there is a bug
                    // in the SPIM block that prevents proper usage with high bus contention that should
                    // be fixed in bao1x.
                    #[cfg(feature = "decongest-udma")]
                    {
                        const TIMEOUT_MS: u64 = 100;
                        let start = tt.elapsed_ms();
                        let mut now = tt.elapsed_ms();
                        // this is required because if we initiate the capture in the middle
                        // of a frame, we get an offset result. This should be fixed by DAR-704
                        // on bao1x if the pull request is accepted; in which case, we can just rely
                        // on setting bit 30 of the CFG_GLOBAL register which will cause any
                        // RX start request to align to the beginning of a frame automatically.
                        while iox.get_gpio_pin_value(IoxPort::PB, 9) == IoxValue::High
                            && ((now - start) < TIMEOUT_MS)
                        {
                            now = tt.elapsed_ms();
                        }
                        if now - start >= TIMEOUT_MS {
                            log::info!("Timeout before capture_async()!");
                        }
                        cam.capture_async();
                    }

                    // this is no longer the case because we're relying on interrupts to wake us up.
                    /*
                    // wait for the transfer to finish
                    let start = tt.elapsed_ms();
                    let mut now = tt.elapsed_ms();
                    use bao1x_hal::udma::Udma;
                    while cam.udma_busy(bao1x_hal::udma::Bank::Rx) && ((now - start) < TIMEOUT_MS) {
                        now = tt.elapsed_ms();
                        // busy-wait to get better time resolution on when the frame ends
                    }
                    if now - start >= TIMEOUT_MS {
                        log::info!("Timeout before rx_buf()!");
                    }
                    */
                }
                GfxOpcode::InvalidCall => {
                    log::error!("Invalid call to bao video server: {:?}", msg);
                }

                // ---- v2 graphics API
                GfxOpcode::AcquireModal => {
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        #[cfg(feature = "no-gam")]
                        modals.acquire_focus(); // relay this to the modals crate so it knows to ignore key presses
                        let sender = msg.sender;
                        log::debug!("Acquirer Sender: {:x?}", sender);
                        modal_queue.push_back(sender);
                        if modal_queue.len() > 1 {
                            // Prevents `msg` from being "dropped" which would cause the blocking scalar to
                            // return
                            core::mem::forget(msg_opt.take());
                        } else {
                            scalar.arg1 = 0;
                            // the message is responded to, which allows the caller to unblock
                        }
                    }
                }
                GfxOpcode::ReleaseModal => {
                    if let Some(_scalar) = msg.body.scalar_message() {
                        #[cfg(feature = "no-gam")]
                        modals.release_focus(); // relay this to the modals crate so it knows to ignore key presses
                        let sender = msg.sender;
                        log::debug!("Release Sender: {:x?}", sender);
                        if let Some(pos) = modal_queue
                            .iter()
                            .position(|x| x.to_usize() & 0xffff_0000 == sender.to_usize() & 0xffff_0000)
                        {
                            modal_queue.remove(pos);
                        } else {
                            log::error!("Release modal called but sender {:x?} was not found", sender);
                        };
                        if let Some(sender) = modal_queue.front() {
                            // Notify the waiter that it is allowed to run
                            xous::return_scalar(*sender, 0).unwrap();
                        }
                    }
                }

                // ---- "regular" graphics API
                GfxOpcode::DrawClipObject => {
                    minigfx::handlers::draw_clip_object(&mut display, msg);
                }
                GfxOpcode::DrawClipObjectList => {
                    minigfx::handlers::draw_clip_object_list(&mut display, msg);
                }
                GfxOpcode::UnclippedObjectList => {
                    minigfx::handlers::draw_object_list(&mut display, msg);
                }
                GfxOpcode::DrawTextView => {
                    minigfx::handlers::draw_text_view(&mut display, msg);
                }
                GfxOpcode::Flush => {
                    log::trace!("***gfx flush*** redraw##");
                    display.redraw();
                }
                GfxOpcode::Clear => {
                    display.clear();
                }
                GfxOpcode::Line => {
                    minigfx::handlers::line(&mut display, screen_clip.into(), msg);
                }
                GfxOpcode::Rectangle => {
                    minigfx::handlers::rectangle(&mut display, screen_clip.into(), msg);
                }
                GfxOpcode::RoundedRectangle => {
                    minigfx::handlers::rounded_rectangle(&mut display, screen_clip.into(), msg);
                }
                GfxOpcode::Circle => {
                    minigfx::handlers::circle(&mut display, screen_clip.into(), msg);
                }
                GfxOpcode::ScreenSize => {
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        let pt = display.screen_size();
                        scalar.arg1 = pt.x as usize;
                        scalar.arg2 = pt.y as usize;
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::QueryGlyphProps => {
                    minigfx::handlers::query_glyph_props(msg);
                }
                GfxOpcode::DrawSleepScreen => {
                    if let Some(_scalar) = msg.body.scalar_message() {
                        display.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
                        display.redraw();
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::DrawBootLogo => {
                    if let Some(_scalar) = msg.body.scalar_message() {
                        display.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
                        display.redraw();
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::RestartBulkRead => {
                    unimplemented!("Not needed for bao1x target");
                }
                GfxOpcode::BulkReadFonts => {
                    unimplemented!("Not needed for bao1x target");
                }
                GfxOpcode::TestPattern => {
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        let _duration = scalar.arg1;
                        todo!("Need to write this for factory testing");
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::Stash => {
                    display.stash();
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        // ack the message if it's a blocking scalar
                        scalar.arg1 = 1;
                    }
                    // no failure if it's not
                }
                GfxOpcode::Pop => {
                    display.pop();
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        // ack the message if it's a blocking scalar
                        scalar.arg1 = 1;
                    }
                    // no failure if it's not
                }
                GfxOpcode::Quit => break,
                _ => {
                    // This is perfectly normal because not all opcodes are handled by all platforms.
                    log::debug!("Invalid or unhandled opcode: {:?}", opcode);
                }
            }
        } else {
            // just idle while the panic handler does its thing
            tt.sleep_ms(10_000).unwrap();
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
