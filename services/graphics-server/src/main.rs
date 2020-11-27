#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::{info, error};

mod backend;
use backend::XousDisplay;

mod api;
use api::Opcode;

mod op;

use core::convert::TryFrom;

mod logo;

use blitstr;

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
    let mut current_glyph = blitstr::fonts::GlyphSet::Regular;
    let mut current_string_clip = blitstr::Rect::full_screen();
    let mut current_cursor = blitstr::Cursor::from_top_left_of(current_string_clip);

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
                    blitstr::clear_region(display.native_buffer(), blitstr::Rect::full_screen());
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
                    blitstr::clear_region(display.native_buffer(), blitstr::Rect::new(
                        rect.min.x as _,
                        rect.min.y as _,
                        rect.max.x as _,
                        rect.max.y as _,));
                }
                Opcode::String(s) => {
                    blitstr::paint_str(display.native_buffer(), current_string_clip.into(), &mut current_cursor, current_glyph.into(), s);
                }
                Opcode::SetGlyph(glyph) => {
                    current_glyph = glyph;
                }
                Opcode::SetCursor(c) => {
                    current_cursor = c;
                }
                Opcode::GetCursor => {
                    let pt: api::Point = api::Point::new(current_cursor.pt.x as i16, current_cursor.pt.y as i16);
                    xous::return_scalar2(
                        msg.sender,
                        pt.into(),
                        current_cursor.line_height,
                    )
                    .expect("GFX: could not return GetCursor request");
                }
                Opcode::SetStringClipping(r) => {
                    current_string_clip = r;
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
                        blitstr::fonts::glyph_to_arg(current_glyph),
                        blitstr::fonts::glyph_to_height(current_glyph),
                    )
                    .expect("GFX: could not return QueryGlyph request");
                }
            }
        } else {
            error!("Couldn't convert opcode");
        }
        display.update();
    }
}
