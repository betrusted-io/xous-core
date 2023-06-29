use std::collections::HashMap;
use graphics_server::*;

use crate::api::CanvasType;
use crate::{Canvas, LayoutApi, LayoutBehavior};
use crate::contexts::MISC_CONTEXT_DEFAULT_TRUST;
const TRUST_OFFSET: u8 = 4;

#[derive(Debug, Copy, Clone)]
pub(crate) struct Framebuffer {
    pub gid: Gid,
    _screensize: Point,
}
impl Framebuffer {
    pub fn init(
        gfx: &graphics_server::Gfx,
        trng: &trng::Trng,
        status_cliprect: &Rectangle,
        canvases: &mut HashMap<Gid, Canvas>
    ) -> Result<Framebuffer, xous::Error> {
        let screensize = gfx.screen_size().expect("Couldn't get screen size");

        // trust level must be lower than "modals" otherwise a modal can't draw over the content
        // dividing by 2 does the trick
        let fb_canvas = Canvas::new(
            Rectangle::new(Point::new(0, status_cliprect.br().y + 1), screensize),
            (MISC_CONTEXT_DEFAULT_TRUST - TRUST_OFFSET) / 2, &trng, None, crate::api::CanvasType::Framebuffer
        ).expect("couldn't create modal canvas");
        let fb_gid = fb_canvas.gid();
        canvases.insert(fb_canvas.gid(), fb_canvas);

        Ok(Framebuffer {
            gid: fb_gid,
            _screensize: screensize,
        })
    }
}
impl LayoutApi for Framebuffer {
    fn behavior(&self) -> LayoutBehavior {
        LayoutBehavior::App
    }
    fn clear(&self, gfx: &graphics_server::Gfx, canvases: &mut HashMap<Gid, Canvas>) -> Result<(), xous::Error> {
        let fb_canvas = canvases.get(&self.gid).expect("couldn't find my canvas");

        let mut rect = fb_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        gfx.draw_rectangle(rect)
    }
    fn resize_height(&mut self, _gfx: &graphics_server::Gfx, new_height: i16, _status_canvas: &Rectangle, canvases: &mut HashMap<Gid, Canvas>) -> Result<Point, xous::Error> {
        let fb_canvas = canvases.get_mut(&self.gid).expect("couldn't find my canvas");
        let orig_rect = fb_canvas.clip_rect();

        let mut fb_clip_rect = Rectangle::new_coords(orig_rect.tl().x, 0, orig_rect.br().x, new_height);
        fb_clip_rect.style = DrawStyle {fill_color: Some(PixelColor::Dark), stroke_color: None, stroke_width: 0,};
        fb_canvas.set_clip(fb_clip_rect);
        Ok(fb_clip_rect.br)
    }
    fn get_gids(&self) ->Vec<crate::api::GidRecord> {
        vec![
            crate::api::GidRecord {
                gid: self.gid,
                canvas_type: CanvasType::Framebuffer
            }
        ]
    }
    fn set_visibility_state(&mut self, onscreen: bool, canvases: &mut HashMap<Gid, Canvas>) {
        let fb_canvas = canvases.get_mut(&self.gid).expect("couldn't find my canvas");
        log::debug!("raw fb entering set_visibilty_state, {}->{}", fb_canvas.is_onscreen(), onscreen);
        fb_canvas.set_onscreen(onscreen);
    }
}