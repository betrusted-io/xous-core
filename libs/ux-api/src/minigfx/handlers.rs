// This file contains the portable portions of graphics opcode implementations.

use core::ops::Add;

use blitstr2::*;
use xous_ipc::Buffer;

use super::FrameBuffer;
use super::*;
use crate::wordwrap::*;

pub fn draw_clip_object<T: FrameBuffer>(display: &mut T, msg: &mut xous::envelope::Envelope) {
    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
    let obj = buffer.to_original::<ClipObject, _>().unwrap();
    log::trace!("DrawClipObject {:?}", obj);
    match obj.obj {
        ClipObjectType::Line(line) => {
            op::line(display, line, Some(obj.clip), false);
        }
        ClipObjectType::XorLine(line) => {
            op::line(display, line, Some(obj.clip), true);
        }
        ClipObjectType::Circ(circ) => {
            op::circle(display, circ, Some(obj.clip));
        }
        ClipObjectType::Rect(rect) => {
            op::rectangle(display, rect, Some(obj.clip), false);
        }
        ClipObjectType::RoundRect(rr) => {
            op::rounded_rectangle(display, rr, Some(obj.clip));
        }
        #[cfg(feature = "ditherpunk")]
        ClipObjectType::Tile(tile) => {
            op::tile(display, tile, Some(obj.clip));
        }
    }
}

pub fn draw_clip_object_list<T: FrameBuffer>(display: &mut T, msg: &mut xous::envelope::Envelope) {
    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
    let list_ipc = buffer.to_original::<ClipObjectList, _>().unwrap();
    for maybe_item in list_ipc.list.iter() {
        if let Some(obj) = maybe_item {
            match obj.obj {
                ClipObjectType::Line(line) => {
                    op::line(display, line, Some(obj.clip), false);
                }
                ClipObjectType::XorLine(line) => {
                    op::line(display, line, Some(obj.clip), true);
                }
                ClipObjectType::Circ(circ) => {
                    op::circle(display, circ, Some(obj.clip));
                }
                ClipObjectType::Rect(rect) => {
                    op::rectangle(display, rect, Some(obj.clip), false);
                }
                ClipObjectType::RoundRect(rr) => {
                    op::rounded_rectangle(display, rr, Some(obj.clip));
                }
                #[cfg(feature = "ditherpunk")]
                ClipObjectType::Tile(tile) => {
                    op::tile(display, tile, Some(obj.clip));
                }
            }
        } else {
            // stop at the first None entry -- if the sender packed the list with a hole in
            // it, that's their bad
            break;
        }
    }
}

pub fn line<T: FrameBuffer>(
    display: &mut T,
    screen_clip: Option<Rectangle>,
    msg: &mut xous::envelope::Envelope,
) {
    if let Some(scalar) = msg.body.scalar_message() {
        let p1 = scalar.arg1;
        let p2 = scalar.arg2;
        let style = scalar.arg3;
        let l = Line::new_with_style(Point::from(p1), Point::from(p2), DrawStyle::from(style));
        op::line(display, l, screen_clip, false);
    } else {
        panic!("Incorrect message type");
    }
}

pub fn rectangle<T: FrameBuffer>(
    display: &mut T,
    screen_clip: Option<Rectangle>,
    msg: &mut xous::envelope::Envelope,
) {
    if let Some(scalar) = msg.body.scalar_message() {
        let tl = scalar.arg1;
        let br = scalar.arg2;
        let style = scalar.arg3;
        let r = Rectangle::new_with_style(Point::from(tl), Point::from(br), DrawStyle::from(style));
        op::rectangle(display, r, screen_clip.into(), false);
    } else {
        panic!("Incorrect message type");
    }
}

pub fn rounded_rectangle<T: FrameBuffer>(
    display: &mut T,
    screen_clip: Option<Rectangle>,
    msg: &mut xous::envelope::Envelope,
) {
    if let Some(scalar) = msg.body.scalar_message() {
        let tl = scalar.arg1;
        let br = scalar.arg2;
        let style = scalar.arg3;
        let r = scalar.arg4;
        let rr = RoundedRectangle::new(
            Rectangle::new_with_style(Point::from(tl), Point::from(br), DrawStyle::from(style)),
            r as _,
        );
        op::rounded_rectangle(display, rr, screen_clip.into());
    } else {
        panic!("Incorrect message type");
    }
}

#[cfg(feature = "ditherpunk")]
pub fn tile<T: FrameBuffer>(
    display: &mut T,
    screen_clip: Option<Rectangle>,
    msg: &mut xous::envelope::Envelope,
) {
    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
    let bm = buffer.to_original::<Tile, _>().unwrap();
    op::tile(display, bm, screen_clip.into());
}

pub fn circle<T: FrameBuffer>(
    display: &mut T,
    screen_clip: Option<Rectangle>,
    msg: &mut xous::envelope::Envelope,
) {
    if let Some(scalar) = msg.body.scalar_message() {
        let center = scalar.arg1;
        let radius = scalar.arg2;
        let style = scalar.arg3;
        let c = Circle::new_with_style(Point::from(center), radius as _, DrawStyle::from(style));
        op::circle(display, c, screen_clip.into());
    } else {
        panic!("Incorrect message type");
    }
}

pub fn query_glyph_props(msg: &mut xous::envelope::Envelope) {
    if let Some(scalar) = msg.body.scalar_message_mut() {
        let style = scalar.arg1;
        let glyph = GlyphStyle::from(style);

        scalar.arg1 = glyph.into();
        scalar.arg2 = glyph_to_height_hint(glyph);
    } else {
        panic!("Incorrect message type");
    }
}

pub fn draw_text_view<T: FrameBuffer>(display: &mut T, msg: &mut xous::envelope::Envelope) {
    let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
    let mut tv = buffer.to_original::<TextView, _>().unwrap();

    if tv.clip_rect.is_none() {
        return;
    } // if no clipping rectangle is specified, nothing to draw

    // this is the clipping rectangle of the canvas in screen coordinates
    let clip_rect = tv.clip_rect.unwrap();
    // this is the translation vector to and from screen space
    let screen_offset: Point = tv.clip_rect.unwrap().tl;

    let typeset_extent = match tv.bounds_hint {
        TextBounds::BoundingBox(r) => {
            Point::new(r.br().x - r.tl().x - tv.margin.x * 2, r.br().y - r.tl().y - tv.margin.y * 2)
        }
        TextBounds::GrowableFromBr(br, width) => {
            Point::new(width as isize - tv.margin.x * 2, br.y - tv.margin.y * 2)
        }
        TextBounds::GrowableFromBl(bl, width) => {
            Point::new(width as isize - tv.margin.x * 2, bl.y - tv.margin.y * 2)
        }
        TextBounds::GrowableFromTl(tl, width) => Point::new(
            width as isize - tv.margin.x * 2,
            (clip_rect.br().y - clip_rect.tl().y - tl.y) - tv.margin.y * 2,
        ),
        TextBounds::GrowableFromTr(tr, width) => Point::new(
            width as isize - tv.margin.x * 2,
            (clip_rect.br().y - clip_rect.tl().y - tr.y) - tv.margin.y * 2,
        ),
        TextBounds::CenteredTop(r) => {
            Point::new(r.br().x - r.tl().x - tv.margin.x * 2, r.br().y - r.tl().y - tv.margin.y * 2)
        }
        TextBounds::CenteredBot(r) => {
            Point::new(r.br().x - r.tl().x - tv.margin.x * 2, r.br().y - r.tl().y - tv.margin.y * 2)
        }
    };
    let mut typesetter = Typesetter::setup(
        tv.to_str(),
        &typeset_extent,
        &tv.style,
        if let Some(i) = tv.insertion { Some(i as usize) } else { None },
    );
    let composition =
        typesetter.typeset(if tv.ellipsis { OverflowStrategy::Ellipsis } else { OverflowStrategy::Abort });

    let composition_top_left = match tv.bounds_hint {
        TextBounds::BoundingBox(r) => r.tl().add(tv.margin),
        TextBounds::GrowableFromBr(br, _width) => Point::new(
            br.x - (composition.bb_width() as isize + tv.margin.x),
            br.y - (composition.bb_height() as isize + tv.margin.y),
        ),
        TextBounds::GrowableFromBl(bl, _width) => {
            Point::new(bl.x + tv.margin.x, bl.y - (composition.bb_height() as isize + tv.margin.y))
        }
        TextBounds::GrowableFromTl(tl, _width) => tl.add(tv.margin),
        TextBounds::GrowableFromTr(tr, _width) => {
            Point::new(tr.x - (composition.bb_width() as isize + tv.margin.x), tr.y + tv.margin.y)
        }
        TextBounds::CenteredTop(r) => {
            if r.width() as isize > composition.bb_width() {
                r.tl().add(Point::new((r.width() as isize - composition.bb_width()) / 2, 0))
            } else {
                r.tl().add(tv.margin)
            }
        }
        TextBounds::CenteredBot(r) => {
            if r.width() as isize > composition.bb_width() {
                r.tl().add(Point::new(
                    (r.width() as isize - composition.bb_width()) / 2,
                    if (r.height() as isize) > (composition.bb_height() + tv.margin.y) {
                        (r.height() as isize) - (composition.bb_height() + tv.margin.y)
                    } else {
                        0
                    },
                ))
            } else {
                r.tl().add(tv.margin)
            }
        }
    }
    .add(screen_offset);

    // compute the clear rectangle -- the border is already in screen coordinates, just add
    // the margin around it
    let mut clear_rect = match tv.bounds_hint {
        TextBounds::BoundingBox(mut r) => {
            r.translate(screen_offset);
            r
        }
        _ => {
            if tv.busy_animation_state.is_some() {
                // we want to clear the entire potentially drawable region, not just the dirty
                // box if we're doing a busy animation.
                let r = Rectangle::new(
                    Point::new(screen_offset.x, composition_top_left.y),
                    Point::new(screen_offset.x, composition_top_left.y)
                        .add(Point::new(typeset_extent.x, composition.bb_height())),
                );
                r
            } else {
                // composition_top_left already had a screen_offset added when it was
                // computed. just margin it out
                let mut r = Rectangle::new(
                    composition_top_left,
                    composition_top_left
                        .add(Point::new(composition.bb_width() as _, composition.bb_height() as _)),
                );
                r.margin_out(tv.margin);
                r
            }
        }
    };

    log::trace!("clip_rect: {:?}", clip_rect);
    log::trace!("composition_top_left: {:?}", composition_top_left);
    log::trace!("clear_rect: {:?}", clear_rect);
    // draw the bubble/border and/or clear the background area
    let bordercolor = if tv.draw_border { Some(PixelColor::Dark) } else { None };
    let borderwidth: isize = if tv.draw_border { tv.border_width as isize } else { 0 };
    let fillcolor = if tv.clear_area || tv.invert {
        if tv.invert { Some(PixelColor::Dark) } else { Some(PixelColor::Light) }
    } else {
        None
    };

    clear_rect.style =
        DrawStyle { fill_color: fillcolor, stroke_color: bordercolor, stroke_width: borderwidth };
    if !tv.dry_run() {
        if tv.rounded_border.is_some() {
            op::rounded_rectangle(
                display,
                RoundedRectangle::new(clear_rect, tv.rounded_border.unwrap() as _),
                tv.clip_rect,
            );
        } else {
            op::rectangle(display, clear_rect, tv.clip_rect, false);
        }
    }
    // for now, if we're in braille mode, emit all text to the debug log so we can see it
    //if cfg!(feature = "braille") {
    //   log::info!("{}", tv);
    //}

    if !tv.dry_run() {
        // note: make the clip rect `tv.clip_rect.unwrap()` if you want to debug wordwrapping
        // artifacts; otherwise smallest_rect masks some problems
        let smallest_rect = clear_rect
            .clip_with(tv.clip_rect.unwrap())
            .unwrap_or(Rectangle::new(Point::new(0, 0), Point::new(0, 0)));
        composition.render(display, composition_top_left, tv.invert, smallest_rect);
    }

    // run the busy animation
    if let Some(state) = tv.busy_animation_state {
        let total_width = typeset_extent.x as isize;
        if total_width > op::BUSY_ANIMATION_RECT_WIDTH * 2 {
            let step = state as isize % (op::BUSY_ANIMATION_RECT_WIDTH * 2);
            for offset in (0..(total_width + op::BUSY_ANIMATION_RECT_WIDTH * 2))
                .step_by((op::BUSY_ANIMATION_RECT_WIDTH * 2) as usize)
            {
                let left_x = offset + step + composition_top_left.x;
                if offset == 0
                    && (step >= op::BUSY_ANIMATION_RECT_WIDTH)
                    && (step < op::BUSY_ANIMATION_RECT_WIDTH * 2)
                {
                    // handle the truncated "left" rectangle
                    let mut trunc_rect = Rectangle::new(
                        Point::new(composition_top_left.x as isize, clear_rect.tl().y),
                        Point::new(
                            (step + composition_top_left.x - op::BUSY_ANIMATION_RECT_WIDTH) as isize,
                            clear_rect.br().y,
                        ),
                    );
                    trunc_rect.style = DrawStyle {
                        fill_color: Some(PixelColor::Light),
                        stroke_color: None,
                        stroke_width: 0,
                    };
                    op::rectangle(display, trunc_rect, tv.clip_rect, true);
                } // the "right" rectangle is handled by the clipping mask
                let mut xor_rect = Rectangle::new(
                    Point::new(left_x as isize, clear_rect.tl().y),
                    Point::new((left_x + op::BUSY_ANIMATION_RECT_WIDTH) as isize, clear_rect.br().y),
                );
                xor_rect.style =
                    DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 };
                op::rectangle(display, xor_rect, tv.clip_rect, true);
            }
        } else {
            // don't do the animation, this could be abused to create inverted text
        }
        tv.busy_animation_state = Some(state + 2);
    }

    // type mismatch for now, replace this with a simple equals once we sort that out
    tv.cursor.pt.x = composition.final_cursor().pt.x;
    tv.cursor.pt.y = composition.final_cursor().pt.y;
    tv.cursor.line_height = composition.final_cursor().line_height;
    tv.overflow = Some(composition.final_overflow());

    tv.bounds_computed = Some(clear_rect);
    log::trace!("cursor ret {:?}, bounds ret {:?}", tv.cursor, tv.bounds_computed);
    // pack our data back into the buffer to return
    buffer.replace(tv).unwrap();
}
