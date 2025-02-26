use core::fmt::Write;

use blitstr2::GlyphStyle;
use num_traits::*;
use ux_api::minigfx::*;
use ux_api::service::api::*;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum TestType {
    BusyAnimation = 9,
    BoundingBox = 10,
    LowerRight = 11,
    LowerLeft = 12,
    TopLeft = 13,
    TopRight = 14,
    Overflow = 15,
    Insert = 16,
    End = 17,
}
const TEST_STYLE: GlyphStyle = GlyphStyle::Tall;
pub fn tests() {
    let _ = std::thread::spawn({
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let gfx = graphics_server::Gfx::new(&xns).unwrap();
            let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

            for index in TestType::BusyAnimation.to_usize().unwrap()..TestType::End.to_usize().unwrap() {
                // pause between each tests
                ticktimer.sleep_ms(1000).unwrap();
                // draw a black screen
                let screensize = gfx.screen_size().expect("Couldn't get screen size");
                let blackout = Rectangle::new_with_style(
                    Point::new(0, 0),
                    screensize,
                    DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1),
                );
                gfx.draw_rectangle(blackout).unwrap();
                gfx.flush().unwrap();

                // start the test
                ticktimer.sleep_ms(1000).unwrap();
                let clipping_area = Rectangle::new_coords(50, 50, 290, 450);

                let text_bounds = Rectangle::new_coords(10, 10, 240, 300);

                //let mut checkbound = clipping_area.clone(); // this checks against the final clipping area.
                let mut checkbound = text_bounds.clone(); // this is just around the bounds specified by the TV
                checkbound.translate(clipping_area.tl());
                checkbound.margin_out(Point::new(1, 1));
                checkbound.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
                gfx.draw_rectangle(checkbound).unwrap();
                gfx.flush().unwrap();

                match FromPrimitive::from_usize(index) {
                    Some(TestType::BusyAnimation) => {
                        let anim_rect = Rectangle::new_coords(10, 10, 240, 30);
                        let mut tv =
                            TextView::new(Gid::new([0, 0, 0, 0]), TextBounds::BoundingBox(anim_rect));
                        tv.clip_rect = Some(clipping_area);
                        tv.style = GlyphStyle::Small;
                        tv.ellipsis = false;
                        write!(tv, "Test of busy animation").unwrap();
                        tv.insertion = None;
                        tv.draw_border = false;
                        tv.busy_animation_state = Some(0);
                        for y in 0..30 {
                            gfx.draw_rectangle(checkbound).unwrap();
                            tv.bounds_hint =
                                TextBounds::BoundingBox(Rectangle::new_coords(10, 10 + y, 240, 30 + y));
                            for _ in 0..5 {
                                gfx.draw_textview(&mut tv).unwrap();
                                gfx.flush().unwrap();
                                ticktimer.sleep_ms(100).unwrap();
                            }
                        }
                    }
                    Some(TestType::BoundingBox) => {
                        let mut tv =
                            TextView::new(Gid::new([0, 0, 0, 0]), TextBounds::BoundingBox(text_bounds));
                        tv.clip_rect = Some(clipping_area);
                        tv.style = TEST_STYLE;
                        tv.ellipsis = true;
                        tv.rounded_border = Some(4);
                        write!(tv, "This is a test of basic word wrapping 自动换行 inside a 😃 bounding box.\nThis should be a new line.\n\nTwo new lines.\nNew line\n with a leading space.\nDone.").unwrap();
                        write!(tv, "Let's add more text😃 until it overflows this is just another test with https://github.com/samblenny/blitstr2/commit/bb7d4ab6a2d8913dcb520895a3c242c933413aae more words and words and words and whatever.").unwrap();
                        log::info!("rendering: {:?}", tv);
                        tv.insertion = None;
                        gfx.draw_textview(&mut tv).unwrap();
                        gfx.flush().unwrap();
                        log::info!("rendered: {:?}", tv);
                        ticktimer.sleep_ms(1000).unwrap();
                    }
                    Some(TestType::LowerRight) => {
                        let mut tv = TextView::new(
                            Gid::new([0, 0, 0, 0]),
                            TextBounds::GrowableFromBr(
                                Point::new(text_bounds.br().x - 4, text_bounds.br().y - 4),
                                ((text_bounds.br().x - text_bounds.tl().x) / 2) as u16,
                            ),
                        );
                        tv.clip_rect = Some(clipping_area);
                        tv.style = TEST_STYLE;
                        tv.ellipsis = true;
                        tv.rounded_border = Some(8);
                        write!(tv, "This is a test of basic word😃 wrapping 自动换行 inside a bounding box.\nThis should be a new line.\n\nTwo new lines.\nNew line\n with a leading space.\nDone.").unwrap();
                        log::info!("rendering: {:?}", tv);
                        tv.insertion = Some(0);
                        gfx.draw_textview(&mut tv).unwrap();
                        gfx.flush().unwrap();
                        log::info!("rendered: {:?}", tv);
                        ticktimer.sleep_ms(1000).unwrap();
                    }
                    Some(TestType::LowerLeft) => {
                        let mut tv = TextView::new(
                            Gid::new([0, 0, 0, 0]),
                            TextBounds::GrowableFromBl(
                                Point::new(text_bounds.tl().x + 4, text_bounds.br().y - 4),
                                ((text_bounds.br().x - text_bounds.tl().x) / 2) as u16,
                            ),
                        );
                        tv.clip_rect = Some(clipping_area);
                        tv.style = TEST_STYLE;
                        tv.ellipsis = true;
                        tv.rounded_border = None;
                        tv.invert = true;
                        write!(tv, "This is a test of basic word😃 wrapping 自动换行 inside a bounding box.\nThis should be a new line.\n\nTwo new lines.\nNew line\n with a leading space.\nDone.").unwrap();
                        log::info!("rendering: {:?}", tv);
                        tv.insertion = Some(3);
                        gfx.draw_textview(&mut tv).unwrap();
                        gfx.flush().unwrap();
                        log::info!("rendered: {:?}", tv);
                        ticktimer.sleep_ms(1000).unwrap();
                    }
                    Some(TestType::TopLeft) => {
                        let mut tv = TextView::new(
                            Gid::new([0, 0, 0, 0]),
                            TextBounds::GrowableFromTl(
                                Point::new(text_bounds.tl().x + 4, text_bounds.tl().y + 4),
                                ((text_bounds.br().x - text_bounds.tl().x) / 2) as u16 + 20,
                            ),
                        );
                        tv.clip_rect = Some(clipping_area);
                        tv.style = TEST_STYLE;
                        tv.ellipsis = false;
                        tv.rounded_border = Some(12);
                        write!(tv, "This is a test of basic word😃 wrapping 自动换行 inside a bounding box.\nThis should be a new line.\n\nTwo new lines.\nNew line\n with a leading space.\nDone.").unwrap();
                        log::info!("rendering: {:?}", tv);
                        tv.insertion = Some(4);
                        gfx.draw_textview(&mut tv).unwrap();
                        gfx.flush().unwrap();
                        log::info!("rendered: {:?}", tv);
                        ticktimer.sleep_ms(1000).unwrap();
                    }
                    Some(TestType::TopRight) => {
                        let mut tv = TextView::new(
                            Gid::new([0, 0, 0, 0]),
                            TextBounds::GrowableFromTr(
                                Point::new(text_bounds.tr().x - 4, text_bounds.tl().y + 4),
                                ((text_bounds.br().x - text_bounds.tl().x) / 2) as u16 + 30,
                            ),
                        );
                        tv.clip_rect = Some(clipping_area);
                        tv.style = TEST_STYLE;
                        tv.ellipsis = false;
                        tv.rounded_border = None;
                        write!(tv, "This is a test of basic word😃 wrapping 自动换行 inside a bounding box.\nThis should be a new line.\n\nTwo new lines.\nNew line\n with a leading space.\nDone.").unwrap();
                        log::info!("rendering: {:?}", tv);
                        tv.insertion = Some(5);
                        gfx.draw_textview(&mut tv).unwrap();
                        gfx.flush().unwrap();
                        log::info!("rendered: {:?}", tv);
                        ticktimer.sleep_ms(1000).unwrap();
                    }
                    Some(TestType::Overflow) => {
                        //gfx.draw_rectangle(blackout).unwrap();
                        //gfx.flush().unwrap();

                        for x_off in 0..32 {
                            let clipping_area = Rectangle::new_coords(100, 100, 200, 200);
                            let text_bounds = Rectangle::new_coords(-20 + x_off, -20 + x_off, 150, 150);

                            let mut checkbound = clipping_area.clone(); // this checks against the final clipping area.
                            checkbound.margin_out(Point::new(1, 1));
                            checkbound.style = DrawStyle::new(PixelColor::Light, PixelColor::Dark, 1);
                            gfx.draw_rectangle(checkbound).unwrap();
                            gfx.flush().unwrap();

                            let mut tv =
                                TextView::new(Gid::new([0, 0, 0, 0]), TextBounds::BoundingBox(text_bounds));
                            tv.clip_rect = Some(clipping_area);
                            tv.style = TEST_STYLE;
                            tv.ellipsis = false;
                            tv.rounded_border = None;
                            write!(tv, "This is a test of basic word wrapping 自动换行 inside a bounding box.\nThis should be a new line.\n\nTwo new lines.\nNew line\n with a leading space.\nDone.").unwrap();
                            log::info!("rendering: {:?}", tv);
                            tv.insertion = Some(x_off as i32 + 10);
                            gfx.draw_textview(&mut tv).unwrap();
                            gfx.flush().unwrap();
                            log::info!("rendered: {:?}", tv);
                            ticktimer.sleep_ms(200).unwrap();
                        }
                    }
                    Some(TestType::Insert) => {
                        gfx.draw_rectangle(blackout).unwrap();
                        gfx.flush().unwrap();
                        gfx.draw_rectangle(checkbound).unwrap();
                        gfx.flush().unwrap();

                        let mut tv =
                            TextView::new(Gid::new([0, 0, 0, 0]), TextBounds::BoundingBox(text_bounds));
                        tv.clip_rect = Some(clipping_area);
                        tv.style = TEST_STYLE;
                        tv.ellipsis = true;
                        tv.rounded_border = Some(4);
                        write!(tv, "This is a test of basic 自动换行 inside\na\n\n😃 bounding box.\nThis should be a new line.\n\nTwo new lines.\nNew line\n with a leading space.\nDone.").unwrap();
                        write!(tv, "Let's add more text😃 until it overflows this is just another test with https://github.com/samblenny/blitstr2/commit/bb7d4ab6a2d8913dcb520895a3c242c933413aae more words and words and words and whatever.").unwrap();
                        for i in 20..60 {
                            log::info!("insertion point at {},{:?}", i, tv.to_str().chars().skip(i).next());
                            tv.insertion = Some(i as i32);
                            gfx.draw_textview(&mut tv).unwrap();
                            gfx.flush().unwrap();
                            ticktimer.sleep_ms(250).unwrap();
                        }
                    }

                    _ => {}
                }
            }
        }
    });
}
