#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::{error, info};

mod api;
use api::Opcode;

use core::pin::Pin;
use rkyv::{archived_value, Unarchive, archived_value_mut};

mod backend;
use backend::XousDisplay;

mod op;

use core::convert::TryFrom;

mod logo;

use api::{DrawStyle, PixelColor, Rectangle, TextBounds, RoundedRectangle};
use blitstr_ref as blitstr;

mod fontmap;

fn draw_boot_logo(display: &mut XousDisplay) {
    display.blit_screen(logo::LOGO_MAP);
}

#[cfg(target_os = "none")]
fn map_fonts() {
    //info!("GFX: mapping fonts");
    // this maps an extra page if the total length happens to fall on a 4096-byte boundary, but this is ok
    // because the reserved area is much larger
    let fontlen: u32 = ((fontmap::FONT_TOTAL_LEN as u32) & 0xFFFF_F000) + 0x1000;
    //info!("GFX: requesting map of length 0x{:08x} at 0x{:08x}", fontlen, fontmap::FONT_BASE);
    let fontregion = xous::syscall::map_memory(
        xous::MemoryAddress::new(fontmap::FONT_BASE),
        None,
        fontlen as usize,
        xous::MemoryFlags::R,
    ).expect("GFX: couldn't map fonts");
    info!("GFX: font base at virtual 0x{:08x}, len of 0x{:08x}", usize::from(fontregion.addr), usize::from(fontregion.size));

    //info!("GFX: mapping regular font to 0x{:08x}", usize::from(fontregion.addr) + fontmap::REGULAR_OFFSET as usize);
    blitstr::map_font(blitstr::GlyphData::Emoji((usize::from(fontregion.addr) + fontmap::EMOJI_OFFSET) as usize));
    blitstr::map_font(blitstr::GlyphData::Hanzi((usize::from(fontregion.addr) + fontmap::HANZI_OFFSET) as usize));
    blitstr::map_font(blitstr::GlyphData::Regular((usize::from(fontregion.addr) + fontmap::REGULAR_OFFSET) as usize));
    blitstr::map_font(blitstr::GlyphData::Small((usize::from(fontregion.addr) + fontmap::SMALL_OFFSET) as usize));
    blitstr::map_font(blitstr::GlyphData::Bold((usize::from(fontregion.addr) + fontmap::BOLD_OFFSET) as usize));
}

#[cfg(not(target_os = "none"))]
fn map_fonts() {
    // does nothing
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    info!("GFX: my PID is {}", xous::process::id());

    // Create a new monochrome simulator display.
    let mut display = XousDisplay::new();

    draw_boot_logo(&mut display);

    map_fonts();

    let mut current_glyph = blitstr::GlyphStyle::Regular;
    let mut current_string_clip = blitstr::ClipRect::full_screen();
    let mut current_cursor = blitstr::Cursor::from_top_left_of(current_string_clip);

    let sid = xous_names::register_name(xous::names::SERVER_NAME_GFX).expect("GFX: can't register server");
    info!("GFX: Server listening on address {:?}", sid);

    if false {
        // leave this test case around
        // for some reason, the top right quadrant draws an extra pixel inside the fill area
        // when a fill color of "Light" is specified. However, if `None` fill is specified, it
        // works correctly. This is really puzzling, because the test for filled drawing happens
        // after the test for border drawing.
        use api::Point;
        let mut r = Rectangle::new(Point::new(20, 200), Point::new(151, 301));
        r.style = DrawStyle {
            fill_color: Some(PixelColor::Light),
            stroke_color: Some(PixelColor::Dark),
            stroke_width: 1,
        };
        let rr = RoundedRectangle::new(r, 16);
        op::rounded_rectangle(display.native_buffer(), rr);
    }

    display.redraw();
    loop {
        let msg = xous::receive_message(sid).unwrap();
        //info!("GFX: Message: {:?}", msg);
        if let xous::Message::Borrow(m) = &msg.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<api::Opcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<api::Opcode>::String(rkyv_s) => {
                    let s: xous::String<4096> = rkyv_s.unarchive();
                    //info!("GFX: unarchived string: {:?}", s);
                    blitstr::paint_str(
                        display.native_buffer(),
                        current_string_clip.into(),
                        &mut current_cursor,
                        current_glyph.into(),
                        s.as_str().unwrap(),
                        false,
                        blitstr::xor_char
                    );
                    //info!("GFX: string painted");
                },
                rkyv::Archived::<api::Opcode>::StringXor(rkyv_s) => {
                    let s: xous::String<4096> = rkyv_s.unarchive();
                    blitstr::paint_str(
                        display.native_buffer(),
                        current_string_clip.into(),
                        &mut current_cursor,
                        current_glyph.into(),
                        s.as_str().unwrap(),
                        true,
                        blitstr::xor_char
                    );
                },
                _ => panic!("GFX: invalid response from server -- corruption occurred in MemoryMessage")
            };
        } else if let xous::Message::MutableBorrow(m) = &msg.body {
            let mut buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let value = unsafe {
                archived_value_mut::<api::Opcode>(Pin::new(buf.as_mut()), m.id as usize)
            };
            //use rkyv::Write;
            //let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
            let debugtv: bool = false;
            match &*value {
                rkyv::Archived::<api::Opcode>::DrawTextView(rtv) => {
                    let mut tv = rtv.unarchive();

                    let paintfn = if tv.dry_run {
                        if debugtv { info!("GFX(TV): doing dry run"); }
                        blitstr::simulate_char
                    } else {
                        if debugtv { info!("GFX(TV): drawing"); }
                        blitstr::xor_char
                    };

                    /*
                    1. figure out text bounds
                    2. clear background, if requested
                    3. draw surrounding rectangle, if requested
                    4. draw text
                    */
                    // first compute the bounding box, if it isn't computed
                    if tv.bounds_computed.is_none() {
                        match tv.bounds_hint {
                            TextBounds::BoundingBox(r) => {
                                tv.bounds_computed = Some(r);
                            },
                            TextBounds::GrowableFromBr(_br, _width) => {
                                unimplemented!()
                            },
                            TextBounds::GrowableFromTl(_tl, _width) => {
                                unimplemented!()
                            },
                            TextBounds::GrowableFromBl(_bl, _width) => {
                                unimplemented!()
                            }
                        }
                    }
                    if debugtv { info!("GFX(TV): computed bounds {:?}", tv.bounds_computed); }

                    // clear the bounding box if requested
                    let mut clear_rect = tv.bounds_computed.unwrap();
                    clear_rect.translate(tv.clip_rect.unwrap().tl);
                    let bordercolor = if tv.draw_border {
                        Some(PixelColor::Dark)
                    } else {
                        None
                    };
                    let borderwidth: i16 = if tv.draw_border {
                        1
                    } else {
                        0
                    };
                    let fillcolor = if tv.clear_area {
                        Some(PixelColor::Light)
                    } else {
                        None
                    };

                    clear_rect.style = DrawStyle {
                        fill_color: fillcolor,
                        stroke_color: bordercolor,
                        stroke_width: borderwidth,
                    };
                    if tv.rounded_border.is_some() {
                        op::rounded_rectangle(display.native_buffer(),
                           RoundedRectangle::new(clear_rect, tv.rounded_border.unwrap() as _));
                    } else {
                        if debugtv { info!("GFX(TV): clearing rectangle {:?}", clear_rect); }
                        op::rectangle(display.native_buffer(), clear_rect);
                    }


                    // this is the actual draw operation
                    clear_rect.translate(tv.margin);

                    /////// TODO: I think we need to clip the clear_rect.tl.x/y to within the screen area for this to be valid?
                    let mut ref_cursor = blitstr::Cursor{
                        pt: blitstr::Pt{x: clear_rect.tl.x as u32, y: clear_rect.tl.y as u32},
                        line_height: 0,
                    };
                    if debugtv { info!("GFX(TV): paint_str with {:?} | {:?} | {:?} | {:?}", clear_rect, ref_cursor, tv.style, tv.text); }
                    blitstr::paint_str(
                        display.native_buffer(),
                        clear_rect.into(),
                        &mut ref_cursor,
                        tv.style.into(),
                        tv.text.as_str().unwrap(),
                        false,
                        paintfn
                    );
                    tv.cursor = ref_cursor;
                },
                _ => panic!("GFX: invalid mutable borrow message"),
            }
        } else if let Ok(opcode) = Opcode::try_from(&msg.body) {
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
                Opcode::RoundedRectangle(rr) => {
                    op::rounded_rectangle(display.native_buffer(), rr);
                }
                Opcode::Circle(c) => {
                    op::circle(display.native_buffer(), c);
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
                    xous::return_scalar2(msg.sender, pt.into(), current_cursor.line_height as usize)
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
                /*
                Opcode::TextView(tv) => {
                    info!("GFX: got draw of '{:?}'", tv);
                    op::textview(display.native_buffer(), tv);
                }*/
                _ => panic!("GFX: received opcode scalar that is not handled")
            }
        } else {
            error!("GFX: Couldn't convert opcode");
        }
        display.update();
    }
}
