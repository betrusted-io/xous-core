use cramium_hal::sh1107::Oled128x128;

pub fn show_logo(sh1107: &mut Oled128x128) {
    /* // pattern test for debugging pixel orientations - done with this now, I think?
    let buf = sh1107.buffer_mut();
    crate::println!("oled test");
    for (i, chunk) in buf.chunks_mut(128).enumerate() {
        for (j, pixel) in chunk.iter_mut().enumerate() {
            // *pixel = (i as u8) << 5 + j as u8;
            if j % 2 == 0 {
                *pixel = 0x55 + i as u8;
            } else {
                *pixel = 0xAA + i as u8;
            }
        }
    }
    crate::println!("oled test end");
    */
    sh1107.buffer_mut().fill(0);
    sh1107.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
}
