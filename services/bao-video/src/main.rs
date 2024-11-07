use cramium_hal::iox::IoGpio;
use cramium_hal::iox::{IoxPort, IoxValue};
use cramium_hal::sh1107::{Mono, Oled128x128};
use cramium_hal::udma::PeriphId;
use cramium_hal::{minigfx::*, sh1107};
use nalgebra::QR;

mod gfx;
mod homography;
mod qr;

const QR_WIDTH: usize = 256;
const QR_HEIGHT: usize = 240;
const BW_THRESH: u8 = 128;

pub fn blit_to_display(sh1107: &mut Oled128x128, frame: &[u8], display_cleared: bool) {
    for (y, row) in frame.chunks(QR_WIDTH).enumerate() {
        if y & 1 == 0 {
            for (x, &pixval) in row.iter().enumerate() {
                if x & 1 == 0 {
                    if x < sh1107.dimensions().x as usize * 2
                        && y < sh1107.dimensions().y as usize * 2 - (gfx::CHAR_HEIGHT as usize + 1) * 2
                    {
                        let luminance = pixval & 0xff;
                        if luminance > BW_THRESH {
                            // flip on y to adjust for sensor orientation. Lower left is (0, 0)
                            // on the display.
                            sh1107.put_pixel(
                                Point::new(x as isize / 2, (sh1107.dimensions().y - 1) - (y as isize / 2)),
                                Mono::White.into(),
                            );
                        } else {
                            // optimization to avoid some computation if we're blitting to an already-black
                            // buffer
                            if !display_cleared {
                                sh1107.put_pixel(
                                    Point::new(
                                        x as isize / 2,
                                        (sh1107.dimensions().y - 1) - (y as isize / 2),
                                    ),
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

fn main() -> ! {
    let stack_size = 1 * 1024 * 1024;
    std::thread::Builder::new().stack_size(stack_size).spawn(wrapped_main).unwrap().join().unwrap()
}

fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    let tt = ticktimer::Ticktimer::new().unwrap();
    let xns = xous_names::XousNames::new().unwrap();

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
    let border = (cols - QR_WIDTH) / 2;
    cam.set_slicing((border, 0), (cols - border, QR_HEIGHT));
    log::info!("320x240 resolution setup with 256x240 slicing");

    let mut frames = 0;
    let mut frame = [0u8; QR_WIDTH * QR_HEIGHT];
    // while iox.get_gpio_pin_value(IoxPort::PB, 9) == IoxValue::High {}
    loop {
        cam.capture_async();

        let mut candidates = Vec::<Point>::new();
        log::info!("------------- SEARCH -----------");
        let finder_width = qr::find_finders(&mut candidates, &frame, BW_THRESH, QR_WIDTH) as isize;
        const CROSSHAIR_LEN: isize = 3;
        if candidates.len() == 3 {
            gfx::msg(&mut sh1107, "Aligning...", Point::new(0, 0), Mono::White.into(), Mono::Black.into());
            for c in candidates.iter() {
                log::info!("******    candidate: {}, {}    ******", c.x, c.y);
                // remap image to screen coordinates (it's 2:1)
                let mut c_screen = *c / 2;
                // flip coordinates to match the camera data
                c_screen = Point::new(c_screen.x, sh1107.dimensions().y - 1 - c_screen.y);
                qr::draw_crosshair(&mut sh1107, c_screen);
            }

            if let Some(mut qr_corners) = qr::QrCorners::from_finders(
                &candidates.try_into().unwrap(),
                Point::new(QR_WIDTH as isize, QR_HEIGHT as isize),
                // add a search margin on the finder width
                (finder_width + (qr::FINDER_SEARCH_MARGIN * finder_width) / (1 + 1 + 3 + 1 + 1)) as usize,
            ) {
                let dims = Point::new(QR_WIDTH as isize, QR_HEIGHT as isize);
                let mut il = qr::ImageRoi::new(&mut frame, dims, BW_THRESH);
                let (src, dst) = qr_corners.mapping(&mut il, qr::HOMOGRAPHY_MARGIN);
                for s in src.iter() {
                    if let Some(p) = s {
                        log::info!("src {:?}", p);
                        qr::draw_crosshair(&mut sh1107, *p / 2);
                    }
                }
                for d in dst.iter() {
                    if let Some(p) = d {
                        log::info!("dst {:?}", p);
                        qr::draw_crosshair(&mut sh1107, *p / 2);
                    }
                }

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

                            // apply homography to generate a new buffer for processing
                            let mut aligned = [0u8; QR_WIDTH * QR_HEIGHT];
                            // iterate through pixels and apply homography
                            for y in 0..dims.y {
                                for x in 0..dims.x {
                                    let (x_src, y_src) =
                                        homography::apply_fixp_homography(&h_inv_fp, (x as i32, y as i32));
                                    if (x_src as i32 >= 0)
                                        && ((x_src as i32) < dims.x as i32)
                                        && (y_src as i32 >= 0)
                                        && ((y_src as i32) < dims.y as i32)
                                    {
                                        // println!("{},{} -> {},{}", x_src as i32, y_src as i32, x, y);
                                        aligned[QR_WIDTH * y as usize + x as usize] =
                                            frame[QR_WIDTH * y_src as usize + x_src as usize];
                                    } else {
                                        aligned[QR_WIDTH * y as usize + x as usize] = 255;
                                    }
                                }
                            }
                            blit_to_display(&mut sh1107, &aligned, true);

                            let mut search_img =
                                rqrr::PreparedImage::prepare_from_greyscale(QR_WIDTH, QR_HEIGHT, |x, y| {
                                    aligned[y * QR_WIDTH + x]
                                });
                            let grids = search_img.detect_grids();
                            log::info!("grids len {}", grids.len());
                            let rawdata = grids[0].get_raw_data();
                            match rawdata {
                                Ok((md, rd)) => {
                                    log::info!("{:?}, {}:{:x?}", md, rd.len, &rd.data[..(rd.len / 8) + 1]);
                                }
                                Err(e) => {
                                    log::info!("Error: {:?}", e);
                                }
                            }
                            log::info!("{:?}", grids[0].decode());
                        }
                    }
                }
            }
        } else {
            // blit fb to sh1107
            blit_to_display(&mut sh1107, &frame, true);
            gfx::msg(&mut sh1107, "Searching...", Point::new(0, 0), Mono::White.into(), Mono::Black.into());
        }

        // swap the double buffer and update to the display
        sh1107.buffer_swap();
        sh1107.draw();

        // clear the front buffer
        sh1107.clear();

        // wait for the transfer to finish
        cam.capture_await(true);
        let fb: &[u32] = cam.rx_buf();

        // fb is non-cacheable, slow memory. If we stride through it in u16 chunks, we end
        // up fetching each location *twice*, because the native width of the bus is a u32
        // Stride through the slice as a u32, allowing us to make the most out of each slow
        // read from IFRAM, and unpack the values into fast SRAM.
        for (&u32src, u8dest) in fb.iter().zip(frame.chunks_mut(2)) {
            u8dest[0] = (u32src & 0xff) as u8;
            u8dest[1] = ((u32src >> 16) & 0xff) as u8;
        }
        frames += 1;
    }
}
