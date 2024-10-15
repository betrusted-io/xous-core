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
    for (i, b) in crate::platform::cramium::poweron_bt::LOGO_MAP.iter().enumerate() {
        for bit in 0..8 {
            if (b & (1 << (7 - bit))) != 0 {
                sh1107.put_pixel(((i % 16) * 8 + bit) as u8, (i / 16) as u8, true);
            }
        }
    }
}
