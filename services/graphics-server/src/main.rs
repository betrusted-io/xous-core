#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
#[macro_use]
mod debug;

use log::info;

mod backend;
use backend::XousDisplay;

mod api;
use api::Opcode;

mod op;
mod fonts;

use core::convert::TryFrom;

mod logo;

fn draw_boot_logo(display: &mut XousDisplay) {
    display.blit_screen(logo::LOGO_MAP);
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();

    // Create a new monochrome simulator display.
    let mut display = XousDisplay::new();

    draw_boot_logo(&mut display);

    let mut current_color = api::Color::from(0usize);
    let mut current_glyph = api::GlyphSet::Regular;
    let mut current_string_clip = op::ClipRegion::screen();

    display.redraw();

    let sid = xous::create_server(b"graphics-server ").unwrap();
    // info!("GFX: Server listening on address {:?}", sid);
    // ::debug_here::debug_here!();
    loop {
        let msg = xous::receive_message(sid).unwrap();
        // info!("GFX: Message: {:?}", msg);
        if let Ok(opcode) = Opcode::try_from(&msg.body) {
            // info!("GFX: Opcode: {:?}", opcode);
            match opcode {
                Opcode::Flush => {
                    display.update();
                    display.redraw();
                },
                Opcode::Clear(_color) => {
                    op::clear_region(display.native_buffer(), op::ClipRegion::screen());
                }
                Opcode::Line(start, end) => {
                    info!("GFX: Drawing line from {:?} to {:?}", start, end);
                    op::line(display.native_buffer(), start.x as _, start.y as _, end.x as _, end.y as _, if current_color.color == 0 { op::PixelColor::Off } else {op::PixelColor::On });
                }
                Opcode::Rectangle(start, end) => {
                    todo!();
                    // Line::new(start.into(), end.into())
                    //     .into_styled(current_style)
                    //     .draw(&mut display)
                    //     .ok();
                }
                Opcode::Circle(mid, radius) => {
                    info!("GFX: Drawing cicrle at {:?} radius {:?}", mid, radius);
                    op::circle(display.native_buffer(), mid.x as _, mid.y as _, radius as _, 0, op::PixelColor::On);
                }
                Opcode::Style(stroke_width, stroke_color, fill_color) => {
                    current_color = stroke_color;
                    // todo!();
                    // current_style.stroke_width = stroke_width;
                    // current_style.stroke_color = Some(if stroke_color.color == 0 {
                    //     BinaryColor::Off
                    // } else {
                    //     BinaryColor::On
                    // });
                    // current_style.fill_color = Some(if fill_color.color == 0 {
                    //     BinaryColor::Off
                    // } else {
                    //     BinaryColor::On
                    // });
                }
                Opcode::ClearRegion(rect) => {
                    op::clear_region(display.native_buffer(), op::ClipRegion {
                        x0: rect.x0 as _,
                        y0: rect.y0 as _,
                        x1: rect.x1 as _,
                        y1: rect.y1 as _,
                    });
                }
                Opcode::String(s) => {
                    match current_glyph {
                        api::GlyphSet::Small => op::string_small_left(display.native_buffer(), current_string_clip, s),
                        api::GlyphSet::Regular => op::string_regular_left(display.native_buffer(), current_string_clip, s),
                        api::GlyphSet::Bold => op::string_bold_left(display.native_buffer(), current_string_clip, s),
                    }
                }
                Opcode::SetGlyph(glyph) => {
                    current_glyph = glyph;
                }
                Opcode::SetStringClipping(r) => {
                    current_string_clip = op::ClipRegion::from(r);
                }
                Opcode::ScreenSize => {
                    xous::return_scalar2(
                        msg.sender,
                        336 as usize,
                        536 as usize,
                    )
                    .expect("GFX: couldn't return ScreenSize request");
                }
                Opcode::QueryGlyph => {
                    xous::return_scalar2(
                        msg.sender,
                        api::glyph_to_arg(current_glyph),
                        api::glyph_to_height(current_glyph),
                    )
                    .expect("GFX: could not return QueryGlyph request");
                }
            }
        } else {
            // info!("Couldn't convert opcode");
        }
        // if let Some(mem) = msg.body.memory() {
        //     xous::return_memory(msg.sender, *mem).expect("couldn't return message");
        // }
        display.update();
    }
}
