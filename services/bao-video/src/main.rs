// Maintainer's note: more character sets are added to baosec targets by modifying
// the character map resolution macro in libs/blitstr2/src/style_macro.rs/english_rules
// Including a resolver to a given character map also pulls the font data into the
// bao-video binary, increasing its size.

#[cfg(not(feature = "hosted-baosec"))]
use bao1x_hal_service::Hal;
use ux_api::minigfx::*;

mod gfx;
#[cfg(feature = "board-baosec")]
mod panic;
mod qr;
#[cfg(not(feature = "hosted-baosec"))]
mod qr_warmup;
#[cfg(feature = "gfx-testing")]
mod testing;
mod waitscreen;
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
    udma::UdmaGlobal,
};
// breadcrumb to future self:
//   - For GC0308 drivers, look in code/esp32-camera for sample code/constants
#[cfg(feature = "board-baosec")]
use bao1x_hal::{
    gc2145::Gc2145,
    i2c::I2c,
    sh1107::{MainThreadToken, Mono, Oled128x128, claim_main_thread},
};
#[cfg(feature = "board-baosec")]
use bao1x_hal_service::UdmaGlobal;
#[cfg(feature = "b64-export")]
use base64::{Engine as _, engine::general_purpose};
use num_traits::*;
#[cfg(not(feature = "hosted-baosec"))]
use utralib::utra;
use ux_api::minigfx::{self, FrameBuffer};
use ux_api::service::api::*;
use xous::{CID, sender::Sender};
use xous_ipc::Buffer;

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

const MAX_RETRIES: u32 = 5;

#[derive(PartialEq, Eq, Clone, Copy)]
enum DisplayOrientation {
    Normal,
    UpsideDown,
}

#[cfg(feature = "b64-export")]
#[allow(dead_code)]
fn encode_base64(input: &[u8]) -> String { general_purpose::STANDARD.encode(input) }

/// This converts a frame of `[u8]` grayscale pixels that may be larger than the native
/// frame buffer resolution into a black and white bitmap.
#[cfg(not(feature = "oem-baosec-lite"))]
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

#[cfg(feature = "oem-baosec-lite")]
pub fn blit_to_display(display: &mut Oled128x128, frame: &[u8], display_cleared: bool, bw_thresh: &mut u8) {
    let mut sum: u32 = 0;
    let mut count: u32 = 0;
    let frame_height = frame.len() / IMAGE_WIDTH;
    let thresh = *bw_thresh; // Dereference once

    let max_display_x = display.dimensions().x as isize;
    let max_display_y = display.dimensions().y as isize - (gfx::CHAR_HEIGHT as isize + 1);

    for y in (0..frame_height).step_by(2) {
        // Skip every other line directly
        let row = &frame[y * IMAGE_WIDTH..(y + 1) * IMAGE_WIDTH];
        let display_y = (frame_height - y - 1) as isize / 2;

        if display_y < 0 || display_y >= max_display_y {
            continue;
        }

        for x in (0..IMAGE_WIDTH).step_by(2) {
            // Skip every other pixel directly
            let display_x = x as isize / 2;

            if display_x >= max_display_x {
                break;
            }

            let luminance = row[x] & 0xff;
            sum += luminance as u32;
            count += 1;

            if luminance > thresh {
                unsafe {
                    display.put_pixel_unchecked(Point::new(display_x, display_y), Mono::White.into());
                }
            } else if !display_cleared {
                unsafe {
                    display.put_pixel_unchecked(Point::new(display_x, display_y), Mono::Black.into());
                }
            }
            // else: display already cleared, skip writing black pixels
        }
    }
    *bw_thresh = (sum / count) as u8;
}

#[repr(align(32))]
#[cfg(not(feature = "hosted-baosec"))]
struct CamIrq {
    csr: utralib::CSR<u32>,
    cid: u32,
    got_irq: Arc<AtomicBool>,
}

#[cfg(not(feature = "hosted-baosec"))]
fn handle_irq(_irq_no: usize, arg: *mut usize) {
    let cam_irq: &mut CamIrq = unsafe { &mut *(arg as *mut CamIrq) };
    // clear the pending interrupt - assume it's just the camera for now
    let pending = cam_irq.csr.r(utra::irqarray8::EV_PENDING);
    cam_irq.csr.wo(utra::irqarray8::EV_PENDING, pending);

    cam_irq.got_irq.store(true, Ordering::SeqCst);

    // activate the handler
    xous::try_send_message(
        cam_irq.cid,
        xous::Message::new_scalar(GfxOpcode::CamIrq.to_usize().unwrap(), pending as usize, 0, 0, 0),
    )
    .ok();
}

fn main() -> ! {
    let stack_size = 2 * 1024 * 1024;
    #[allow(unreachable_code)] // false positive
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
    #[cfg(not(feature = "hosted-baosec"))]
    let mut i2c = I2c::new();
    #[allow(unused_variables)]
    #[cfg(not(feature = "hosted-baosec"))]
    let hal = Hal::new();

    let mut display = Oled128x128::new(main_thread_token, bao1x_api::PERCLK, &iox, &udma_global);
    retry_display_op(&udma_global, &mut display, |d| d.init()).unwrap();
    display.clear();
    retry_display_op(&udma_global, &mut display, |d| d.draw()).unwrap();

    // ---- panic handler - set up early so we can see panics quickly
    // install the graphical panic handler. It won't catch really early panics, or panics in this crate,
    // but it'll do the job 90% of the time and it's way better than having none at all.
    let is_panic = Arc::new(AtomicBool::new(false));

    // ---- Baochip boot logo, if enabled
    #[cfg(feature = "with-logo")]
    {
        display.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
        display.redraw();
    }

    // This is safe because the SPIM is finished with initialization, and the handler is
    // Mutex-protected.
    #[cfg(feature = "board-baosec")]
    {
        let panic_display = unsafe { display.to_raw_parts() };
        panic::panic_handler_thread(is_panic.clone(), panic_display);
    }

    // respond to keyboard presses - needed to abort QR code mode
    let kbd = bao1x_api::keyboard::Keyboard::new(&xns).unwrap();
    kbd.register_listener(SERVER_NAME_GFX, GfxOpcode::KeyPress.to_u32().unwrap() as usize);

    #[cfg(not(feature = "hosted-baosec"))]
    let mut timer = {
        let timer_range = xous::map_memory(
            xous::MemoryAddress::new(utra::pwm::HW_PWM_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map PWM range");
        utralib::CSR::new(timer_range.as_ptr() as usize as *mut u32)
    };

    udma_global.udma_clock_config(PeriphId::Cam, true);
    // ---- camera initialization
    #[cfg(all(not(feature = "oem-baosec-lite"), not(feature = "hosted-baosec")))]
    let cam_clk = (IoxPort::PA, 0);
    #[cfg(feature = "oem-baosec-lite")]
    let cam_clk = (IoxPort::PA, 3);

    #[cfg(not(feature = "hosted-baosec"))]
    let (mut cam, cam_pdwn) = {
        // wait for other inits to finish so we can do this roughly atomically
        tt.sleep_ms(1000).ok();

        // setup camera power
        #[cfg(not(feature = "oem-baosec-lite"))]
        match bao1x_hal::axp2101::Axp2101::new(&mut i2c) {
            Ok(mut pmic) => {
                pmic.set_dcdc(&mut i2c, Some((1.8, false)), bao1x_hal::axp2101::WhichDcDc::Dcdc5).unwrap();
                pmic.set_ldo(&mut i2c, Some(2.85), bao1x_hal::axp2101::WhichLdo::Bldo2).unwrap();
            }
            Err(e) => {
                log::error!("Couldn't setup regulators for camera, camera will be non-functional: {:?}", e);
            }
        };

        // ensure that muxed pins are tri-state for low power (maybe move this too loader?)
        iox.setup_pin(IoxPort::PF, 9, Some(IoxDir::Input), Some(IoxFunction::Gpio), None, None, None, None);
        iox.setup_pin(IoxPort::PA, 1, Some(IoxDir::Input), Some(IoxFunction::Gpio), None, None, None, None);
        iox.setup_pin(IoxPort::PA, 2, Some(IoxDir::Input), Some(IoxFunction::Gpio), None, None, None, None);

        // setup camera clock
        iox.setup_pin(
            cam_clk.0,
            cam_clk.1,
            Some(IoxDir::Output),
            Some(IoxFunction::AF3),
            None,
            None,
            Some(IoxEnable::Disable),
            Some(IoxDriveStrength::Drive8mA),
        );
        #[cfg(not(feature = "oem-baosec-lite"))]
        {
            timer.wo(utra::pwm::REG_CH_EN, 1);
            timer.rmwf(utra::pwm::REG_TIM0_CFG_R_TIMER0_SAW, 1);
            timer.rmwf(utra::pwm::REG_TIM0_CH0_TH_R_TIMER0_CH0_TH, 0);
            timer.rmwf(utra::pwm::REG_TIM0_CH0_TH_R_TIMER0_CH0_MODE, 3);
            unsafe { timer.base().add(2).write_volatile(0) }; // for some reason the register extraction didn't get this register...
            timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 0);
        }
        #[cfg(feature = "oem-baosec-lite")]
        {
            timer.wo(utra::pwm::REG_CH_EN, 1);
            timer.rmwf(utra::pwm::REG_TIM0_CFG_R_TIMER0_SAW, 1);
            timer.rmwf(utra::pwm::REG_TIM0_CH3_TH_R_TIMER0_CH3_TH, 0);
            timer.rmwf(utra::pwm::REG_TIM0_CH3_TH_R_TIMER0_CH3_MODE, 3);
            unsafe { timer.base().add(2).write_volatile(0) }; // for some reason the register extraction didn't get this register...
            timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 0);
        }
        /* // register debug
        for i in 0..12 {
            println!("0x{:2x}: 0x{:08x}", i, unsafe { pwm.add(i).read_volatile() })
        }
        println!("0x{:2x}: 0x{:08x}", 65, unsafe { pwm.add(65).read_volatile() });
        */

        // setup camera pins
        let cam_pdwn = bao1x_hal::board::setup_camera_pins(&iox);
        // this is safe because we turned on the clocks before calling it
        let mut cam = unsafe { Gc2145::new().expect("couldn't allocate camera") };

        timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 1);
        tt.sleep_ms(2).ok(); // wait for camera to clock-up
        iox.set_gpio_pin_value(cam_pdwn.0, cam_pdwn.1, IoxValue::Low);

        // power up the camera
        // starts MCLK
        timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 1);
        tt.sleep_ms(2).ok(); // wait for camera to clock-up
        // bring camera out of powerdown
        iox.set_gpio_pin_value(cam_pdwn.0, cam_pdwn.1, IoxValue::Low);
        tt.sleep_ms(3).ok(); // wait for camera to power-up
        let (pid, mid) = cam.read_id(&mut i2c);
        log::info!("Camera pid {:x}, mid {:x}", pid, mid);
        cam.init(&mut i2c, bao1x_api::camera::Resolution::Res320x240);
        tt.sleep_ms(1).ok();

        let (cols, _rows) = cam.resolution();
        let border = (cols - IMAGE_WIDTH) / 2;
        cam.set_slicing((border, 0), (cols - border, IMAGE_HEIGHT));
        tt.sleep_ms(2).ok();

        // power down the camera, now that all the internal registers have been set up
        // assert PWWDN
        iox.set_gpio_pin_value(cam_pdwn.0, cam_pdwn.1, IoxValue::High);
        // stop MCLK
        tt.sleep_ms(2).ok();
        timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 0);
        timer.wo(utra::pwm::REG_CH_EN, 0);
        iox.setup_pin(
            cam_clk.0,
            cam_clk.1,
            Some(IoxDir::Input),
            Some(IoxFunction::Gpio),
            None,
            None,
            None,
            None,
        );

        (cam, cam_pdwn)
    };
    #[cfg(feature = "hosted-baosec")]
    // unused dummy object
    let mut cam = unsafe { Gc2145::new().expect("couldn't allocate camera") };

    #[cfg(not(feature = "hosted-baosec"))]
    let cid = xous::connect(sid).unwrap(); // self-connection always succeeds

    // ---- register interrupt handler
    let got_irq = Arc::new(AtomicBool::new(false));
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

        cam_irq =
            CamIrq { csr: utralib::CSR::new(irq.as_mut_ptr() as *mut u32), cid, got_irq: got_irq.clone() };
        let irq_arg = &cam_irq as *const CamIrq as *mut usize;
        log::info!("irq_arg: {:x}", irq_arg as usize);
        xous::claim_interrupt(utra::irqarray8::IRQARRAY8_IRQ, handle_irq, irq_arg)
            .expect("couldn't claim IRQ8");
        // enable camera Rx IRQ
        irq_csr.wfo(utra::irqarray8::EV_ENABLE_CAM_RX, 1);
    }

    // ---- main loop variables
    let screen_clip = Rectangle::new(Point::new(0, 0), display.screen_size());
    let screen_size = display.screen_size(); // make a copy so the borrow checker doesn't complain

    // this will kick the hardware into the QR code scanning routine automatically. Eventually
    // this needs to be turned into a call that can invoke and abort the QR code scanning.
    #[cfg(feature = "autotest")]
    {
        log::info!("initiating auto test");
        let acquisition = QrAcquisition { content: None, meta: None };
        let mut buf = Buffer::into_buf(acquisition).unwrap();
        buf.lend_mut(cid, GfxOpcode::AcquireQr.to_u32().unwrap()).ok();
    }
    #[cfg(feature = "no-gam")]
    let modals = modals::Modals::new(&xns).unwrap();
    let mut modal_queue = VecDeque::<Sender>::new();
    let mut frames = 0;
    let mut frame = [0u8; IMAGE_WIDTH * IMAGE_HEIGHT];
    let mut msg_opt = None;
    #[cfg(feature = "gfx-testing")]
    testing::tests();
    let mut bw_thresh: u8 = 128;
    let mut qr_request: Option<xous::MessageEnvelope> = None;
    let mut kbd_listeners: Vec<(CID, usize)> = Vec::new();
    let mut dry_run = false;
    #[allow(unused_mut)]
    let mut orientation = DisplayOrientation::Normal;
    loop {
        if !is_panic.load(Ordering::Relaxed) {
            xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
            let msg = msg_opt.as_mut().unwrap();
            let opcode =
                num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(GfxOpcode::InvalidCall);
            log::debug!("{:?}", opcode);
            match opcode {
                #[cfg(not(feature = "hosted-baosec"))]
                GfxOpcode::AcquireQr => {
                    if qr_request.is_none() {
                        // decode dummy data - what this does is load the swapped out QR decoding logic, thus
                        // improving the latency of the decoder on the "first hit". The sole purpose of this
                        // is to improve the user experience during scanning.
                        let mut img = rqrr::PreparedImage::prepare_from_bitmap(
                            bao1x_hal::sh1107::COLUMN as _,
                            bao1x_hal::sh1107::ROW as _,
                            |x, y| {
                                let bitnum = x + y * bao1x_hal::sh1107::COLUMN as usize;
                                // true is `black`
                                crate::qr_warmup::BITMAP[bitnum / 32] & 1 << (bitnum % 32) != 0
                            },
                        );
                        let grids = img.detect_grids();
                        if grids.len() == 1 {
                            match grids[0].decode() {
                                Ok((_meta, data)) => {
                                    log::info!("warmed up decoder with {}", data);
                                }
                                Err(e) => {
                                    log::error!("Test image failed to decode! {:?}", e)
                                }
                            }
                        } else {
                            log::error!("test image failed to decode, this shouldn't happen!");
                        }

                        // camera hardware can be a bit finnicky about starting reliably. give it
                        // a couple of tries before giving up. I think it might have to do with I2C
                        // contention - the working theory is that if another I2C driver inserts
                        // a transaction in the middle of the long camera init sequence, we can
                        // end up with the camera in a bad/unknown state. Unfortunately, the I2C
                        // "atomic" implemenation can't handle the size of the I2C poke list,
                        // so for now we're going to do a re-try hoping that on the retry the
                        // I2C bus is clear for us.
                        const RETRY_LIMIT: usize = 3;
                        let mut retries = 0;
                        while retries < RETRY_LIMIT {
                            // reset camera UDMA block
                            udma_global.reset(PeriphId::Cam);

                            // orientation is fixed through a reset of the whole display subsystem
                            // now issue a PRST_N - this will reset camera and OLED
                            iox.set_gpio_pin_value(IoxPort::PA, 6, IoxValue::High);
                            tt.sleep_ms(5).ok();
                            iox.set_gpio_pin_value(IoxPort::PA, 6, IoxValue::Low);
                            // have to re-init display after reset
                            tt.sleep_ms(100).ok();
                            display
                                .init()
                                .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));

                            // display "wait" icon
                            display.blit_screen(&crate::waitscreen::BITMAP);
                            display
                                .redraw()
                                .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));

                            // this will defer response until later
                            qr_request = msg_opt.take();

                            // power up the camera
                            // starts MCLK
                            iox.setup_pin(
                                cam_clk.0,
                                cam_clk.1,
                                Some(IoxDir::Output),
                                Some(IoxFunction::AF3),
                                None,
                                None,
                                Some(IoxEnable::Disable),
                                Some(IoxDriveStrength::Drive8mA),
                            );
                            timer.wo(utra::pwm::REG_CH_EN, 1);
                            timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 1);
                            tt.sleep_ms(10).ok(); // wait for camera to clock-up
                            // bring camera out of powerdown
                            iox.set_gpio_pin_value(cam_pdwn.0, cam_pdwn.1, IoxValue::Low);
                            tt.sleep_ms(10).ok(); // wait for camera to power-up
                            let (pid, mid) = cam.read_id(&mut i2c);
                            log::info!("Camera pid {:x}, mid {:x}", pid, mid);
                            cam.init(&mut i2c, bao1x_api::camera::Resolution::Res320x240);
                            tt.sleep_ms(15).ok();

                            let (cols, _rows) = cam.resolution();
                            let border = (cols - IMAGE_WIDTH) / 2;
                            cam.set_slicing((border, 0), (cols - border, IMAGE_HEIGHT));
                            log::info!("320x240 resolution setup with 256x240 slicing");

                            hal.set_preemption(false);

                            const STARTUP_TIMEOUT_MS: u128 = 2500;
                            let start = std::time::Instant::now();
                            cam_irq.got_irq.store(false, Ordering::SeqCst);
                            // now start an acquisition
                            cam.capture_async();

                            let mut started = false;
                            while std::time::Instant::now().duration_since(start).as_millis()
                                < STARTUP_TIMEOUT_MS
                            {
                                // this effectively halts anything else from happening until a CamIrq is
                                // produced
                                if cam_irq.got_irq.load(Ordering::SeqCst) {
                                    started = true;
                                    break;
                                }
                            }
                            if started {
                                break;
                            }

                            // not started, reset camera and try again
                            hal.set_preemption(true);
                            retries += 1;
                            log::warn!("Retrying camera start-up sequence {}/{}", retries, RETRY_LIMIT);
                            // power down the camera
                            iox.set_gpio_pin_value(cam_pdwn.0, cam_pdwn.1, IoxValue::High);
                            // stop MCLK
                            tt.sleep_ms(2).ok();
                            timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 0);
                            timer.wo(utra::pwm::REG_CH_EN, 0);
                            iox.setup_pin(
                                cam_clk.0,
                                cam_clk.1,
                                Some(IoxDir::Input),
                                Some(IoxFunction::Gpio),
                                None,
                                None,
                                None,
                                None,
                            );
                            // restore the message into its original location
                            msg_opt = qr_request.take();
                        }

                        if retries == RETRY_LIMIT {
                            log::error!("Couldn't start camera, rebooting whole system");
                            let xns = xous_names::XousNames::new().unwrap();
                            let susres = susres::Susres::new_without_hook(&xns).unwrap();
                            susres.reboot(true).ok();
                        }
                    }
                    // if qr_request is already pending, ignore any new acquisition requests
                }
                GfxOpcode::KeyPress => {
                    if let Some(scalar) = msg.body.scalar_message() {
                        #[cfg(not(feature = "hosted-baosec"))]
                        let k = char::from_u32(scalar.arg1 as u32).unwrap_or('\u{0000}');
                        #[cfg(not(feature = "hosted-baosec"))]
                        // ignore accelerometer reports
                        if !(k == '🔽' || k == '🔼') {
                            // any key press will abort QR acquisition by taking the qr_request.
                            if let Some(mut envelope) = qr_request.take() {
                                let acquisition = QrAcquisition { content: None, meta: None };
                                let mut response = unsafe {
                                    xous_ipc::Buffer::from_memory_message_mut(
                                        envelope.body.memory_message_mut().unwrap(),
                                    )
                                };
                                response.replace(acquisition).unwrap();
                                if orientation == DisplayOrientation::UpsideDown {
                                    display.flip_vertical(true).unwrap_or_else(|_| {
                                        display_timeout_handler(&udma_global, &mut display)
                                    })
                                }
                                // remove "frozen" frame
                                display.clear();
                                display
                                    .redraw()
                                    .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));

                                hal.set_preemption(true);
                            }
                        }
                        // forward messages on to listeners iff we don't have an active modal
                        if modal_queue.len() == 0 {
                            for &(listener_conn, listener_op) in kbd_listeners.iter() {
                                xous::try_send_message(
                                    listener_conn,
                                    xous::Message::new_scalar(
                                        listener_op,
                                        scalar.arg1,
                                        scalar.arg2,
                                        scalar.arg3,
                                        scalar.arg4,
                                    ),
                                )
                                .ok();
                            }
                        }
                    }
                }
                GfxOpcode::CamIrq => {
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

                    if qr_request.is_some() {
                        cam.capture_async();
                    } else {
                        #[cfg(not(feature = "hosted-baosec"))]
                        {
                            // power down the camera, now that the request is done
                            // assert PWWDN
                            iox.set_gpio_pin_value(cam_pdwn.0, cam_pdwn.1, IoxValue::High);
                            // stop MCLK
                            tt.sleep_ms(2).ok();
                            timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 0);
                            timer.wo(utra::pwm::REG_CH_EN, 0);
                            iox.setup_pin(
                                cam_clk.0,
                                cam_clk.1,
                                Some(IoxDir::Input),
                                Some(IoxFunction::Gpio),
                                None,
                                None,
                                None,
                                None,
                            );
                            continue;
                        }
                    }

                    let mut candidates = Vec::<Point>::new();
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
                            orientation == DisplayOrientation::UpsideDown,
                            screen_size,
                        );
                        display
                            .draw()
                            .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                        #[cfg(feature = "eternal-scan")]
                        {
                            display.clear();
                            continue;
                        }
                        let mut img =
                            rqrr::PreparedImage::prepare_from_greyscale(IMAGE_WIDTH, IMAGE_HEIGHT, |x, y| {
                                frame[y * IMAGE_WIDTH + x]
                            });
                        let grids = img.detect_grids();
                        if grids.len() == 1 {
                            match grids[0].decode() {
                                Ok((meta, content)) => {
                                    gfx::msg(
                                        &mut display,
                                        "Success!",
                                        Point::new(0, 0),
                                        Mono::White.into(),
                                        Mono::Black.into(),
                                        orientation == DisplayOrientation::UpsideDown,
                                        screen_size,
                                    );
                                    display.draw().unwrap_or_else(|_| {
                                        display_timeout_handler(&udma_global, &mut display)
                                    });
                                    // this take will cause the QR response to be routed to the sender since
                                    // the Message `Drop`s. It will also cause the sampling of the camera to
                                    // stop on the next frame.
                                    if let Some(mut envelope) = qr_request.take() {
                                        // remove "frozen" frame
                                        display.clear();
                                        display.redraw().unwrap_or_else(|_| {
                                            display_timeout_handler(&udma_global, &mut display)
                                        });

                                        let metadata = format!("{:?}", meta);
                                        #[cfg(not(feature = "hosted-baosec"))]
                                        if content.starts_with("test://") {
                                            log::info!(
                                                "{}{},{}",
                                                bao1x_hal::board::BOOKEND_START,
                                                content,
                                                bao1x_hal::board::BOOKEND_END
                                            );
                                        }
                                        let acquisition =
                                            QrAcquisition { content: Some(content), meta: Some(metadata) };
                                        let mut response = unsafe {
                                            xous_ipc::Buffer::from_memory_message_mut(
                                                envelope.body.memory_message_mut().unwrap(),
                                            )
                                        };
                                        response.replace(acquisition).unwrap();
                                        if orientation == DisplayOrientation::UpsideDown {
                                            display.flip_vertical(true).unwrap_or_else(|_| {
                                                display_timeout_handler(&udma_global, &mut display)
                                            })
                                        }
                                        #[cfg(not(feature = "hosted-baosec"))]
                                        hal.set_preemption(true);
                                        continue;
                                    } else {
                                        log::info!("meta: {:?}", meta);
                                        log::info!("************ {} ***********", content);
                                        gfx::msg(
                                            &mut display,
                                            &format!("{:?}", meta),
                                            Point::new(0, 0),
                                            Mono::White.into(),
                                            Mono::Black.into(),
                                            orientation == DisplayOrientation::UpsideDown,
                                            screen_size,
                                        );
                                        gfx::msg(
                                            &mut display,
                                            &format!("{:?}", content),
                                            Point::new(0, 64),
                                            Mono::White.into(),
                                            Mono::Black.into(),
                                            orientation == DisplayOrientation::UpsideDown,
                                            screen_size,
                                        );
                                    }
                                }
                                Err(e) => {
                                    log::info!("{:?}", e);
                                    gfx::msg(
                                        &mut display,
                                        &format!("{:?}", e),
                                        Point::new(0, 0),
                                        Mono::White.into(),
                                        Mono::Black.into(),
                                        orientation == DisplayOrientation::UpsideDown,
                                        screen_size,
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
                            orientation == DisplayOrientation::UpsideDown,
                            screen_size,
                        );
                    }

                    display.draw().unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));

                    // clear the front buffer
                    display.clear();
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
                GfxOpcode::FilteredKeyboardListener => {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let kr = buffer.as_flat::<KeyboardRegistration, _>().unwrap();
                    match xns.request_connection_blocking(kr.server_name.as_str()) {
                        Ok(cid) => {
                            kbd_listeners
                                .push((cid, <u32 as From<u32>>::from(kr.listener_op_id.into()) as usize));
                        }
                        Err(e) => {
                            log::error!("couldn't connect to listener: {:?}", e);
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
                    if qr_request.is_none() {
                        log::trace!("***gfx flush*** redraw##");
                        if !dry_run {
                            display
                                .redraw()
                                .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                        }
                    }
                }
                GfxOpcode::Clear => {
                    if qr_request.is_none() {
                        display.clear();
                    }
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
                        display
                            .redraw()
                            .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::DrawBootLogo => {
                    if let Some(_scalar) = msg.body.scalar_message() {
                        display.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
                        display
                            .redraw()
                            .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
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
                    display.pop().unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        // ack the message if it's a blocking scalar
                        scalar.arg1 = 1;
                    }
                    // no failure if it's not
                }
                GfxOpcode::RenderQr => {
                    minigfx::handlers::render_qr(&mut display, screen_clip.into(), msg);
                }
                #[cfg(feature = "board-baosec")]
                GfxOpcode::PowerDown => {
                    display.stash();
                    display.powerdown();
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        // ack the message if it's a blocking scalar
                        scalar.arg1 = 1;
                    }
                }
                #[cfg(feature = "board-baosec")]
                GfxOpcode::PowerUp => {
                    // safety: this is safe because we call init() a prescribed delay after power-up
                    unsafe { display.powerup() };
                    tt.sleep_ms(5).ok();
                    display.init().unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                    display.pop().unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        // ack the message if it's a blocking scalar
                        scalar.arg1 = 1;
                    }
                }
                #[cfg(feature = "board-baosec")]
                GfxOpcode::BaosecBitmap => {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let bitmap = buffer.to_original::<BaosecBitmap, _>().unwrap();
                    display.render_bitmap(bitmap);
                }
                #[cfg(feature = "board-baosec")]
                GfxOpcode::BaosecBitmapDiffuse => {
                    use rand::Rng;
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let bitmap = buffer.to_original::<BaosecBitmap, _>().unwrap();
                    display
                        .render_bitmap_diffuse(&bitmap, 10, rand::thread_rng().gen::<u64>())
                        .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                }
                #[cfg(feature = "board-baosec")]
                GfxOpcode::Brightness => {
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        let brightness = scalar.arg1.min(255) as u8;
                        display
                            .brightness(brightness)
                            .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                    } else if let Some(scalar) = msg.body.scalar_message() {
                        let brightness = scalar.arg1.min(255) as u8;
                        display
                            .brightness(brightness)
                            .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                    }
                }
                #[cfg(feature = "board-baosec")]
                GfxOpcode::FlipScreen => {
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        log::debug!("gfx flip");
                        if scalar.arg1 != 0 {
                            orientation = DisplayOrientation::UpsideDown;
                        } else {
                            orientation = DisplayOrientation::Normal;
                        }
                        if qr_request.is_none() {
                            display
                                .flip_vertical(scalar.arg1 != 0)
                                .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                            if !dry_run {
                                display
                                    .redraw()
                                    .unwrap_or_else(|_| display_timeout_handler(&udma_global, &mut display));
                            }
                        }
                    }
                }
                #[cfg(feature = "board-baosec")]
                GfxOpcode::DryRun => {
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        dry_run = scalar.arg1 != 0;
                    }
                }
                GfxOpcode::Quit => {
                    log::info!("refusing to quit, this operation is not supported on this platform!");
                }
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
}

#[allow(unused_variables)]
fn display_timeout_handler(udma_global: &UdmaGlobal, display: &mut Oled128x128) {
    log::info!("resetting display spim block");
    #[cfg(feature = "board-baosec")]
    {
        udma_global.reset(PeriphId::from(bao1x_hal::board::get_display_pins().0));
        display.reinit_spi();
    }
}

/// This is actually "infalliable" in the sense that if we can't initialize the
/// display, we diverge and reboot.
fn retry_display_op<F, R, E>(udma: &UdmaGlobal, display: &mut Oled128x128, mut op: F) -> Result<R, E>
where
    F: FnMut(&mut Oled128x128) -> Result<R, E>,
    E: core::fmt::Debug,
{
    let mut attempts = 0;
    loop {
        match op(display) {
            Ok(val) => return Ok(val),
            Err(e) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
                attempts += 1;
                if attempts >= MAX_RETRIES {
                    log::warn!("Display seems stuck, rebooting whole chip.");
                    let xns = xous_names::XousNames::new().unwrap();
                    let susres = susres::Susres::new_without_hook(&xns).unwrap();
                    susres.reboot(true).ok();
                }
                log::warn!(
                    "Display op failed (attempt {}/{}), resetting SPI block... {:?}",
                    attempts,
                    MAX_RETRIES,
                    e
                );
                display_timeout_handler(udma, display);
            }
        }
    }
}
