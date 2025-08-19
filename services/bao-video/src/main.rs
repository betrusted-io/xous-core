use ux_api::minigfx::*;

mod gfx;
mod homography;
mod modules;
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

#[cfg(feature = "b64-export")]
use base64::{Engine as _, engine::general_purpose};
use blitstr2::fontmap;
#[cfg(feature = "board-baosec")]
use cram_hal_service::{I2c, UdmaGlobal};
use cramium_api::*;
#[cfg(feature = "hosted-baosec")]
use cramium_emu::{
    camera::Gc2145,
    display::{MainThreadToken, Mono, Oled128x128, claim_main_thread},
    i2c::I2c,
    udma::UdmaGlobal,
};
// breadcrumb to future self:
//   - For GC0308 drivers, look in code/esp32-camera for sample code/constants
#[cfg(feature = "board-baosec")]
use cramium_hal::{
    gc2145::Gc2145,
    sh1107::{MainThreadToken, Mono, Oled128x128, claim_main_thread},
};
#[cfg(feature = "board-baosec")]
use num_traits::*;
#[cfg(not(feature = "hosted-baosec"))]
use utralib::utra;
use ux_api::minigfx::{self, FrameBuffer};
use ux_api::service::api::*;
use xous::MemoryRange;
use xous::sender::Sender;
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

// Next steps for performance improvement:
//
// Improve qr::mapping -> point_from_hv_lines such that we're not just deriving the HV
// lines from the the edges of the finder regions, we're also using the very edge of
// the whole QR code itself to guide the line. This will improve the intersection point
// so that we can accurately hit the "fourth corner". At the moment it's sort of a
// luck of the draw if the interpolation hits exactly right, or if we're roughly a module
// off from ideal, which causes the data around that point to be interpreted incorrectly.

#[cfg(feature = "b64-export")]
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

#[cfg(any(feature = "cramium-soc"))]
fn map_fonts() -> MemoryRange {
    log::trace!("mapping fonts");
    // this maps an extra page if the total length happens to fall on a 4096-byte boundary, but this is ok
    // because the reserved area is much larger
    let fontlen: u32 = ((fontmap::FONT_TOTAL_LEN as u32 + 8) & 0xFFFF_F000) + 0x1000;
    log::info!("requesting map of length 0x{:08x} at 0x{:08x}", fontlen, fontmap::FONT_BASE);
    let fontregion = xous::syscall::map_memory(
        xous::MemoryAddress::new(fontmap::FONT_BASE),
        None,
        fontlen as usize,
        xous::MemoryFlags::R,
    )
    .expect("couldn't map fonts");
    log::info!(
        "font base at virtual 0x{:08x}, len of 0x{:08x}",
        fontregion.as_ptr() as usize,
        fontregion.len()
    );

    log::debug!(
        "mapping tall font to 0x{:08x}",
        fontregion.as_ptr() as usize + fontmap::TALL_OFFSET as usize
    );
    blitstr2::fonts::bold::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::BOLD_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::emoji::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::EMOJI_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::mono::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::MONO_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::tall::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::TALL_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::regular::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::REGULAR_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::small::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::SMALL_OFFSET as usize) as u32, Ordering::SeqCst);

    fontregion
}

#[cfg(not(target_os = "xous"))]
fn map_fonts() -> MemoryRange {
    // does nothing
    let fontlen: u32 = ((fontmap::FONT_TOTAL_LEN as u32 + 8) & 0xFFFF_F000) + 0x1000;
    let fontregion = xous::syscall::map_memory(None, None, fontlen as usize, xous::MemoryFlags::R)
        .expect("couldn't map dummy memory for fonts");

    fontregion
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

    let mut display = Oled128x128::new(main_thread_token, cramium_api::PERCLK, &iox, &udma_global);
    display.init();
    display.clear();
    display.draw();

    let fontregion = map_fonts();

    // ---- panic handler - set up early so we can see panics quickly
    // install the graphical panic handler. It won't catch really early panics, or panics in this crate,
    // but it'll do the job 90% of the time and it's way better than having none at all.
    let is_panic = Arc::new(AtomicBool::new(false));

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
        // setup camera pins
        let (cam_pdwn_bnk, cam_pdwn_pin) = cramium_hal::board::setup_camera_pins(&iox);
        // disable camera powerdown
        iox.set_gpio_pin_value(cam_pdwn_bnk, cam_pdwn_pin, IoxValue::Low);
    }
    udma_global.udma_clock_config(PeriphId::Cam, true);
    // this is safe because we turned on the clocks before calling it
    let mut cam = unsafe { Gc2145::new().expect("couldn't allocate camera") };

    tt.sleep_ms(100).ok();

    let (pid, mid) = cam.read_id(&mut i2c);
    log::info!("Camera pid {:x}, mid {:x}", pid, mid);
    cam.init(&mut i2c, cramium_api::camera::Resolution::Res320x240);
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

    // ---- boot logo
    display.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
    display.redraw();

    // ---- main loop variables
    let screen_clip = Rectangle::new(Point::new(0, 0), display.screen_size());
    let mut bulkread = BulkRead::default(); // holding buffer for bulk reads; wastes ~8k when not in use, but saves a lot of copy/init for each iteration of the read

    // this will kick the hardware into the QR code scanning routine automatically. Eventually
    // this needs to be turned into a call that can invoke and abort the QR code scanning.
    #[cfg(feature = "autotest")]
    {
        cam.capture_async();
    }

    #[cfg(feature = "no-gam")]
    let modals = modals::Modals::new(&xns).unwrap();
    let mut modal_queue = VecDeque::<Sender>::new();
    let mut frames = 0;
    let mut frame = [0u8; IMAGE_WIDTH * IMAGE_HEIGHT];
    #[cfg(feature = "b64-export")]
    let mut original = [0u8; IMAGE_WIDTH * IMAGE_HEIGHT];
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
                    let finder_width =
                        qr::find_finders(&mut candidates, &frame, bw_thresh, IMAGE_WIDTH) as isize;
                    if candidates.len() == 3 {
                        #[cfg(feature = "b64-export")]
                        original.copy_from_slice(&frame); // make a backup copy for diagnostics
                        let candidates_orig = candidates.clone();
                        let mut x_candidates: [Point; 3] = [Point::new(0, 0); 3];
                        // apply homography to generate a new buffer for processing
                        let mut aligned = Vec::new();
                        let mut qr_pixels: Option<usize> = None;
                        if let Some(mut qr_corners) = qr::QrCorners::from_finders(
                            &candidates.try_into().unwrap(),
                            Point::new(IMAGE_WIDTH as isize, IMAGE_HEIGHT as isize),
                            // add a search margin on the finder width
                            (finder_width + (qr::FINDER_SEARCH_MARGIN * finder_width) / (1 + 1 + 3 + 1 + 1))
                                as usize,
                        ) {
                            let dims = Point::new(IMAGE_WIDTH as isize, IMAGE_HEIGHT as isize);
                            let mut il = qr::ImageRoi::new(&mut frame, dims, bw_thresh);
                            let (src, dst) = qr_corners.mapping(&mut il, qr::HOMOGRAPHY_MARGIN);
                            let mut src_f: [(f32, f32); 4] = [(0.0, 0.0); 4];
                            let mut dst_f: [(f32, f32); 4] = [(0.0, 0.0); 4];
                            let mut all_found = true;
                            for (s, s_f32) in src.iter().zip(src_f.iter_mut()) {
                                if let Some(p) = s {
                                    *s_f32 = p.to_f32();
                                } else {
                                    all_found = false;
                                }
                            }
                            for (d, d_f32) in dst.iter().zip(dst_f.iter_mut()) {
                                if let Some(p) = d {
                                    *d_f32 = p.to_f32();
                                } else {
                                    all_found = false;
                                }
                            }

                            if all_found {
                                if let Some(h) = homography::find_homography(src_f, dst_f) {
                                    if let Some(h_inv) = h.try_inverse() {
                                        log::info!("{:?}", h_inv);
                                        let h_inv_fp = homography::matrix3_to_fixp(h_inv);
                                        log::info!("{:?}", h_inv_fp);

                                        aligned = vec![0u8; qr_corners.qr_pixels() * qr_corners.qr_pixels()];
                                        // iterate through pixels and apply homography
                                        for y in 0..qr_corners.qr_pixels() {
                                            for x in 0..qr_corners.qr_pixels() {
                                                let (x_src, y_src) = homography::apply_fixp_homography(
                                                    &h_inv_fp,
                                                    (x as i32, y as i32),
                                                );
                                                if (x_src as i32 >= 0)
                                                    && ((x_src as i32) < dims.x as i32)
                                                    && (y_src as i32 >= 0)
                                                    && ((y_src as i32) < dims.y as i32)
                                                {
                                                    // println!("{},{} -> {},{}", x_src as i32, y_src as i32,
                                                    // x,
                                                    // y);
                                                    aligned
                                                        [qr_corners.qr_pixels() * y as usize + x as usize] =
                                                        frame[IMAGE_WIDTH * y_src as usize + x_src as usize];
                                                } else {
                                                    aligned
                                                        [qr_corners.qr_pixels() * y as usize + x as usize] =
                                                        255;
                                                }
                                            }
                                        }

                                        // we can also know the location of the finders by transforming them
                                        let h_fp = homography::matrix3_to_fixp(h);
                                        for (i, &c) in candidates_orig.iter().enumerate() {
                                            let (x, y) = homography::apply_fixp_homography(
                                                &h_fp,
                                                (c.x as i32, c.y as i32),
                                            );
                                            x_candidates[i] = Point::new(x as isize, y as isize);
                                        }
                                        qr_pixels = Some(qr_corners.qr_pixels());
                                    }
                                }
                            }
                        }

                        if let Some(qr_width) = qr_pixels {
                            // show the transformed/aligned frame
                            frame.fill(255);
                            for (dst_line, src_line) in
                                frame.chunks_mut(IMAGE_WIDTH).zip(aligned.chunks(qr_width))
                            {
                                // "center up" the QR in the middle by the estimated margin
                                // this is just a perceptual trick to prevent users from shifting the position
                                // of the camera
                                for (dst, &src) in dst_line[qr::HOMOGRAPHY_MARGIN.abs() as usize..]
                                    .iter_mut()
                                    .zip(src_line.iter())
                                {
                                    *dst = src;
                                }
                            }
                            blit_to_display(&mut display, &frame, true, &mut bw_thresh);

                            // we now have a QR code in "canonical" orientation, with a
                            // known width in pixels
                            for &x in x_candidates.iter() {
                                log::info!("transformed finder location {:?}", x);
                            }

                            // Confirm that the finders coordinates are valid
                            let mut checked_candidates = Vec::<Point>::new();
                            let x_finder_width =
                                qr::find_finders(&mut checked_candidates, &aligned, bw_thresh, qr_width as _)
                                    as isize;
                            log::info!("x_finder width: {}", x_finder_width);

                            // check that the new coordinates are within delta pixels of the original
                            const XFORM_DELTA: isize = 2;
                            let mut deltas = Vec::<Point>::new();
                            for c in checked_candidates {
                                log::info!("x_point: {:?}", c);
                                for &xformed in x_candidates.iter() {
                                    let delta = xformed - c;
                                    log::info!("delta: {:?}", delta);
                                    if delta.x.abs() <= XFORM_DELTA && delta.y.abs() <= XFORM_DELTA {
                                        deltas.push(delta);
                                    }
                                }
                            }
                            if deltas.len() == 3 {
                                let (version, modules) = qr::guess_code_version(
                                    x_finder_width as usize,
                                    (qr_width as isize + qr::HOMOGRAPHY_MARGIN * 2) as usize,
                                );

                                log::info!("image dims: {}", qr_width);
                                log::info!("guessed version: {}, modules: {}", version, modules);
                                log::info!(
                                    "QR symbol width in pixels: {}",
                                    qr_width - 2 * (qr::HOMOGRAPHY_MARGIN.abs() as usize)
                                );

                                let qr = qr::ImageRoi::new(
                                    &mut aligned,
                                    Point::new(qr_width as _, qr_width as _),
                                    bw_thresh,
                                );
                                let grid = modules::stream_to_grid(
                                    &qr,
                                    qr_width,
                                    modules,
                                    qr::HOMOGRAPHY_MARGIN.abs() as usize,
                                    bw_thresh,
                                );

                                println!("grid len {}", grid.len());
                                for y in 0..modules {
                                    for x in 0..modules {
                                        if grid[y * modules + x] {
                                            print!("X");
                                        } else {
                                            print!(" ");
                                        }
                                    }
                                    println!(" {:2}", y);
                                }
                                #[cfg(feature = "b64-export")]
                                {
                                    println!("begin orig base 64");
                                    for block in original.chunks(16384) {
                                        let encoded = encode_base64(block);
                                        println!("{}", encoded);
                                    }
                                    println!("end orig base 64");
                                    println!("begin base 64");
                                    for block in frame.chunks(16384) {
                                        let encoded = encode_base64(block);
                                        println!("{}", encoded);
                                    }
                                    println!("end base 64");
                                }

                                let simple = rqrr::SimpleGrid::from_func(modules, |x, y| {
                                    grid[(modules - 1) - x + y * modules]
                                });
                                let grid = rqrr::Grid::new(simple);
                                match grid.decode() {
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
                            } else {
                                log::info!("Transformed image did not survive sanity check!");
                                #[cfg(feature = "b64-export")]
                                {
                                    println!("begin orig base 64");
                                    for block in original.chunks(16384) {
                                        let encoded = encode_base64(block);
                                        println!("{}", encoded);
                                    }
                                    println!("end orig base 64");
                                    println!("begin base 64");
                                    for block in frame.chunks(16384) {
                                        let encoded = encode_base64(block);
                                        println!("{}", encoded);
                                    }
                                    println!("end base 64");
                                }

                                gfx::msg(
                                    &mut display,
                                    "Hold device steady...",
                                    Point::new(0, 0),
                                    Mono::White.into(),
                                    Mono::Black.into(),
                                );
                            }
                        } else {
                            blit_to_display(&mut display, &frame, true, &mut bw_thresh);
                            for c in candidates_orig.iter() {
                                log::debug!("******    candidate: {}, {}    ******", c.x, c.y);
                                // remap image to screen coordinates (it's 2:1)
                                let c_screen = *c / 2;
                                // flip coordinates to match the camera data
                                // c_screen = Point::new(c_screen.x, display.dimensions().y - 1 - c_screen.y);
                                qr::draw_crosshair(&mut display, c_screen);
                            }
                            gfx::msg(
                                &mut display,
                                "Align the QR code...",
                                Point::new(0, 0),
                                Mono::White.into(),
                                Mono::Black.into(),
                            );
                        }
                    } else {
                        // blit raw camera fb to display
                        blit_to_display(&mut display, &frame, true, &mut bw_thresh);
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
                        tt.sleep_ms(2000).ok();
                    }

                    // clear the front buffer
                    display.clear();

                    // re-initiate the capture. This is done at the bottom of the loop because UDMA
                    // congestion leads to system instability. When this problem is solved, we would
                    // actually want to re-initiate the capture immediately (or leave it on continuous mode)
                    // to allow capture to process concurrently with the code. However, there is a bug
                    // in the SPIM block that prevents proper usage with high bus contention that should
                    // be fixed in NTO.
                    #[cfg(feature = "decongest-udma")]
                    {
                        const TIMEOUT_MS: u64 = 100;
                        let start = tt.elapsed_ms();
                        let mut now = tt.elapsed_ms();
                        // this is required because if we initiate the capture in the middle
                        // of a frame, we get an offset result. This should be fixed by DAR-704
                        // on NTO if the pull request is accepted; in which case, we can just rely
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
                    use cramium_hal::udma::Udma;
                    while cam.udma_busy(cramium_hal::udma::Bank::Rx) && ((now - start) < TIMEOUT_MS) {
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
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        bulkread.from_offset = 0;
                        scalar.arg1 = 0;
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::BulkReadFonts => {
                    // this also needs to reflect in root-keys/src/implementation.rs @ sign_loader()
                    let fontlen = fontmap::FONT_TOTAL_LEN as u32
                        + 16  // minver
                        + 16  // current ver
                        + 8; // sig ver + len
                    let mut buf =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    //let mut bulkread = buf.as_flat::<BulkRead, _>().unwrap(); // try to skip the copy/init
                    // step by using a persistent structure Safety: `u8` contains no
                    // undefined values
                    let fontslice = unsafe { fontregion.as_slice::<u8>() };
                    assert!(fontlen <= fontslice.len() as u32);
                    if bulkread.from_offset >= fontlen {
                        log::error!("BulkReadFonts attempt to read out of bound on the font area; ignoring!");
                        continue;
                    }
                    let readlen = if bulkread.from_offset + bulkread.buf.len() as u32 > fontlen {
                        // returns what is readable of the last bit; anything longer than the fontlen is
                        // undefined/invalid
                        fontlen as usize - bulkread.from_offset as usize
                    } else {
                        bulkread.buf.len()
                    };
                    for (&src, dst) in fontslice
                        [bulkread.from_offset as usize..bulkread.from_offset as usize + readlen]
                        .iter()
                        .zip(bulkread.buf.iter_mut())
                    {
                        *dst = src;
                    }
                    bulkread.len = readlen as u32;
                    bulkread.from_offset += readlen as u32;
                    buf.replace(bulkread).unwrap();
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
