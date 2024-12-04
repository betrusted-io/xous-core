use cramium_hal::iox::{IoxPort, IoxValue};
use cramium_hal::minigfx::*;
use cramium_hal::sh1107::{Mono, Oled128x128};
use cramium_hal::udma::PeriphId;
use num_traits::ToPrimitive;
use utralib::utra;

mod gfx;
mod homography;
mod modules;
mod qr;

const IMAGE_WIDTH: usize = 256;
const IMAGE_HEIGHT: usize = 240;
const BW_THRESH: u8 = 128;

// Next steps for performance improvement:
//
// Improve qr::mapping -> point_from_hv_lines such that we're not just deriving the HV
// lines from the the edges of the finder regions, we're also using the very edge of
// the whole QR code itself to guide the line. This will improve the intersection point
// so that we can accurately hit the "fourth corner". At the moment it's sort of a
// luck of the draw if the interpolation hits exactly right, or if we're roughly a module
// off from ideal, which causes the data around that point to be interpreted incorrectly.

pub fn blit_to_display(sh1107: &mut Oled128x128, frame: &[u8], display_cleared: bool) {
    for (y, row) in frame.chunks(IMAGE_WIDTH).enumerate() {
        if y & 1 == 0 {
            for (x, &pixval) in row.iter().enumerate() {
                if x & 1 == 0 {
                    if x < sh1107.dimensions().x as usize * 2
                        && y < sh1107.dimensions().y as usize * 2 - (gfx::CHAR_HEIGHT as usize + 1) * 2
                    {
                        let luminance = pixval & 0xff;
                        if luminance > BW_THRESH {
                            sh1107.put_pixel(Point::new(x as isize / 2, y as isize / 2), Mono::White.into());
                        } else {
                            // optimization to avoid some computation if we're blitting to an already-black
                            // buffer
                            if !display_cleared {
                                sh1107.put_pixel(
                                    Point::new(x as isize / 2, y as isize / 2),
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
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum Opcode {
    CamIrq,
    InvalidCall,
}

#[repr(align(32))]
struct CamIrq {
    csr: utralib::CSR<u32>,
    cid: u32,
}

fn handle_irq(_irq_no: usize, arg: *mut usize) {
    let cam_irq: &mut CamIrq = unsafe { &mut *(arg as *mut CamIrq) };
    // clear the pending interrupt - assume it's just the camera for now
    let pending = cam_irq.csr.r(utra::irqarray8::EV_PENDING);
    cam_irq.csr.wo(utra::irqarray8::EV_PENDING, pending);

    // activate the handler
    xous::try_send_message(
        cam_irq.cid,
        xous::Message::new_scalar(Opcode::CamIrq.to_usize().unwrap(), pending as usize, 0, 0, 0),
    )
    .ok();
}

fn main() -> ! {
    let stack_size = 1 * 1024 * 1024;
    std::thread::Builder::new().stack_size(stack_size).spawn(wrapped_main).unwrap().join().unwrap()
}

fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name("bao video subsystem", None).expect("can't register server");

    let tt = ticktimer::Ticktimer::new().unwrap();

    let iox = cram_hal_service::IoxHal::new();
    let udma_global = cram_hal_service::UdmaGlobal::new();
    let mut i2c = cram_hal_service::I2c::new();

    let mut sh1107 = cramium_hal::sh1107::Oled128x128::new(cram_hal_service::PERCLK, &iox, &udma_global);
    sh1107.init();
    sh1107.buffer_swap();
    sh1107.draw();

    // setup camera pins
    let (cam_pdwn_bnk, cam_pdwn_pin) = cramium_hal::board::setup_ov2640_pins(&iox);
    // disable camera powerdown
    iox.set_gpio_pin_value(cam_pdwn_bnk, cam_pdwn_pin, cramium_hal::iox::IoxValue::Low);
    udma_global.udma_clock_config(PeriphId::Cam, true);
    // this is safe because we turned on the clocks before calling it
    let mut cam = unsafe { cramium_hal::ov2640::Ov2640::new().expect("couldn't allocate camera") };

    tt.sleep_ms(100).ok();

    let (pid, mid) = cam.read_id(&mut i2c);
    log::info!("Camera pid {:x}, mid {:x}", pid, mid);
    cam.init(&mut i2c, cramium_hal::ov2640::Resolution::Res320x240);
    cam.poke(&mut i2c, 0xFF, 0x00);
    cam.poke(&mut i2c, 0xDA, 0x01); // YUV LE
    tt.sleep_ms(1).ok();

    let (cols, _rows) = cam.resolution();
    let border = (cols - IMAGE_WIDTH) / 2;
    cam.set_slicing((border, 0), (cols - border, IMAGE_HEIGHT));
    log::info!("320x240 resolution setup with 256x240 slicing");

    #[cfg(feature = "decongest-udma")]
    log::info!("Decongest udma option enabled.");

    // register interrupt handler
    let irq = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::irqarray8::HW_IRQARRAY8_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map IRQ CSR range");
    let mut irq_csr = utralib::CSR::new(irq.as_mut_ptr() as *mut u32);
    irq_csr.wo(utra::irqarray8::EV_PENDING, 0xFFFF); // clear any pending interrupts

    let cid = xous::connect(sid).unwrap(); // self-connection always succeeds
    let cam_irq = CamIrq { csr: utralib::CSR::new(irq.as_mut_ptr() as *mut u32), cid };
    let irq_arg = &cam_irq as *const CamIrq as *mut usize;
    log::debug!("irq_arg: {:x}", irq_arg as usize);
    xous::claim_interrupt(utra::irqarray8::IRQARRAY8_IRQ, handle_irq, irq_arg).expect("couldn't claim IRQ8");
    // enable camera Rx IRQ
    irq_csr.wfo(utra::irqarray8::EV_ENABLE_CAM_RX, 1);

    // kick off the first acquisition synchronous to the frame sync. With NTO/DAR-704 fix we
    // could turn on the sync_sof field and skip this step.
    while iox.get_gpio_pin_value(IoxPort::PB, 9) == IoxValue::High {}
    cam.capture_async();

    let mut frames = 0;
    let mut frame = [0u8; IMAGE_WIDTH * IMAGE_HEIGHT];
    let mut decode_success;
    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode = num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(Opcode::InvalidCall);
        log::debug!("{:?}", opcode);
        match opcode {
            Opcode::CamIrq => {
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
                        frame[(IMAGE_HEIGHT - y_src - 1) * IMAGE_WIDTH + 2 * x_src] = (u32src & 0xff) as u8;
                        frame[(IMAGE_HEIGHT - y_src - 1) * IMAGE_WIDTH + 2 * x_src + 1] =
                            ((u32src >> 16) & 0xff) as u8;
                    }
                }
                frames += 1;

                let mut candidates = Vec::<Point>::new();
                decode_success = false;
                log::info!("------------- SEARCH {} -----------", frames);
                let finder_width = qr::find_finders(&mut candidates, &frame, BW_THRESH, IMAGE_WIDTH) as isize;
                if candidates.len() == 3 {
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
                        let mut il = qr::ImageRoi::new(&mut frame, dims, BW_THRESH);
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
                                                // println!("{},{} -> {},{}", x_src as i32, y_src as i32, x,
                                                // y);
                                                aligned[qr_corners.qr_pixels() * y as usize + x as usize] =
                                                    frame[IMAGE_WIDTH * y_src as usize + x_src as usize];
                                            } else {
                                                aligned[qr_corners.qr_pixels() * y as usize + x as usize] =
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
                            // this is just a perceptual trick to prevent users from shifting the position of
                            // the camera
                            for (dst, &src) in dst_line[qr::HOMOGRAPHY_MARGIN.abs() as usize..]
                                .iter_mut()
                                .zip(src_line.iter())
                            {
                                *dst = src;
                            }
                        }
                        blit_to_display(&mut sh1107, &frame, true);

                        // we now have a QR code in "canonical" orientation, with a
                        // known width in pixels
                        for &x in x_candidates.iter() {
                            log::info!("transformed finder location {:?}", x);
                        }

                        // Confirm that the finders coordinates are valid
                        let mut checked_candidates = Vec::<Point>::new();
                        let x_finder_width =
                            qr::find_finders(&mut checked_candidates, &aligned, BW_THRESH, qr_width as _)
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
                                BW_THRESH,
                            );
                            let grid = modules::stream_to_grid(
                                &qr,
                                qr_width,
                                modules,
                                qr::HOMOGRAPHY_MARGIN.abs() as usize,
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
                                        &mut sh1107,
                                        &format!("{:?}", meta),
                                        Point::new(0, 0),
                                        Mono::White.into(),
                                        Mono::Black.into(),
                                    );
                                    gfx::msg(
                                        &mut sh1107,
                                        &format!("{:?}", content),
                                        Point::new(0, 64),
                                        Mono::White.into(),
                                        Mono::Black.into(),
                                    );
                                }
                                Err(e) => {
                                    log::info!("{:?}", e);
                                    gfx::msg(
                                        &mut sh1107,
                                        &format!("{:?}", e),
                                        Point::new(0, 0),
                                        Mono::White.into(),
                                        Mono::Black.into(),
                                    );
                                }
                            }
                        } else {
                            log::info!("Transformed image did not survive sanity check!");
                            gfx::msg(
                                &mut sh1107,
                                "Hold device steady...",
                                Point::new(0, 0),
                                Mono::White.into(),
                                Mono::Black.into(),
                            );
                        }
                    } else {
                        blit_to_display(&mut sh1107, &frame, true);
                        for c in candidates_orig.iter() {
                            log::debug!("******    candidate: {}, {}    ******", c.x, c.y);
                            // remap image to screen coordinates (it's 2:1)
                            let c_screen = *c / 2;
                            // flip coordinates to match the camera data
                            // c_screen = Point::new(c_screen.x, sh1107.dimensions().y - 1 - c_screen.y);
                            qr::draw_crosshair(&mut sh1107, c_screen);
                        }
                        gfx::msg(
                            &mut sh1107,
                            "Align the QR code...",
                            Point::new(0, 0),
                            Mono::White.into(),
                            Mono::Black.into(),
                        );
                    }
                } else {
                    // blit raw camera fb to sh1107
                    blit_to_display(&mut sh1107, &frame, true);
                    gfx::msg(
                        &mut sh1107,
                        "Scan QR code...",
                        Point::new(0, 0),
                        Mono::White.into(),
                        Mono::Black.into(),
                    );
                }

                // swap the double buffer and update to the display
                sh1107.buffer_swap();
                sh1107.draw();
                if decode_success {
                    tt.sleep_ms(2000).ok();
                }

                // clear the front buffer
                sh1107.clear();

                // re-initiate the capture. This is done at the bottom of the loop because UDMA
                // congestion leads to system instability. When this problem is solved, we would
                // actually want to re-initiate the capture immediately (or leave it on continuous mode)
                // to allow capture to process concurrently with the code. However, there is a bug
                // in the SPIM block that prevents proper usage with high bus contention that should
                // be fixed in NTO.
                const TIMEOUT_MS: u64 = 100;
                #[cfg(feature = "decongest-udma")]
                {
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
            Opcode::InvalidCall => {
                log::error!("Invalid call to bao video server: {:?}", msg);
            }
        }
    }
}
