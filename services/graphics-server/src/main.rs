#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::{error, info};

mod backend;
use backend::XousDisplay;

mod api;
use api::Opcode;

mod op;

use core::convert::TryFrom;

mod logo;

use api::{DrawStyle, PixelColor, Rectangle};
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

    let mut current_glyph = blitstr::GlyphStyle::Regular;
    let mut current_string_clip = blitstr::ClipRect::full_screen();
    let mut current_cursor = blitstr::Cursor::from_top_left_of(current_string_clip);

    let sid = xous_names::register_name(xous::names::SERVER_NAME_GFX).expect("GFX: can't register server");
    info!("GFX: Server listening on address {:?}", sid);

    display.redraw();
    loop {
        let msg = xous::receive_message(sid).unwrap();
        // info!("GFX: Message: {:?}", msg);
        if let Ok(opcode) = Opcode::try_from(&msg.body) {
            // info!("GFX: Opcode: {:?}", opcode);
            match opcode {
                Opcode::Flush => {
                    display.update();
                    display.redraw();
                }
                Opcode::Clear => {
                    let mut r = Rectangle::full_screen();
                    r.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 0);
                    op::rectangle(display.native_buffer(), r)
                }
                Opcode::Line(l) => {
                    op::line(display.native_buffer(), l);
                }
                Opcode::Rectangle(r) => {
                    op::rectangle(display.native_buffer(), r);
                }
                Opcode::Circle(c) => {
                    op::circle(display.native_buffer(), c);
                }
                Opcode::String(s) => {
                    blitstr::paint_str(
                        display.native_buffer(),
                        current_string_clip.into(),
                        &mut current_cursor,
                        current_glyph.into(),
                        s,
                        false,
                    );
                }
                Opcode::StringXor(s) => {
                    blitstr::paint_str(
                        display.native_buffer(),
                        current_string_clip.into(),
                        &mut current_cursor,
                        current_glyph.into(),
                        s,
                        true,
                    );
                }
                Opcode::SetGlyphStyle(glyph) => {
                    current_glyph = glyph;
                }
                Opcode::SetCursor(c) => {
                    current_cursor = c;
                }
                Opcode::GetCursor => {
                    let pt: api::Point =
                        api::Point::new(current_cursor.pt.x as i16, current_cursor.pt.y as i16);
                    xous::return_scalar2(msg.sender, pt.into(), current_cursor.line_height)
                        .expect("GFX: could not return GetCursor request");
                }
                Opcode::SetStringClipping(r) => {
                    current_string_clip = r;
                }
                Opcode::ScreenSize => {
                    xous::return_scalar2(msg.sender, 336 as usize, 536 as usize)
                        .expect("GFX: couldn't return ScreenSize request");
                }
                Opcode::QueryGlyphStyle => {
                    xous::return_scalar2(
                        msg.sender,
                        current_glyph.into(),
                        blitstr::glyph_to_height_hint(current_glyph),
                    )
                    .expect("GFX: could not return QueryGlyph request");
                }
                Opcode::QueryGlyphProps(glyph) => {
                    xous::return_scalar2(
                        msg.sender,
                        glyph.into(),
                        blitstr::glyph_to_height_hint(glyph),
                    )
                    .expect("GFX: could not return QueryGlyphProps request");
                }
            }
        } else {
            error!("Couldn't convert opcode");
        }
        display.update();
    }
}
