use cramium_hal::iox::{IoGpio, IoSetup};
use cramium_hal::udma::UdmaGlobalConfig;

pub fn show_logo<T>(perclk_freq: u32, udma_global: &dyn UdmaGlobalConfig, iox: &T)
where
    T: IoSetup + IoGpio,
{
    // test the display SPI interface
    // safety: this is called exactly once on boot
    let mut sh1107 = cramium_hal::sh1107::Oled128x128::new(perclk_freq, iox, udma_global);
    sh1107.init();
    {
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
    sh1107.buffer_swap();
    sh1107.draw();
}
