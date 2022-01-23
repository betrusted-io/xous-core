use std::collections::HashMap;
use graphics_server::*;

use crate::{Canvas, LayoutApi, LayoutBehavior};

#[derive(Debug, Copy, Clone)]
pub(crate) struct Framebuffer {
    pub gid: Gid,
    screensize: Point,
    visible: bool,
}
impl Framebuffer {
    pub fn init(gfx: &graphics_server::Gfx, trng: &trng::Trng, base_trust: u8, canvases: &mut HashMap<Gid, Canvas>) -> Result<Framebuffer, xous::Error> {
        let screensize = gfx.screen_size().expect("Couldn't get screen size");

        let checked_base_trust = if base_trust < 3 {
            3
        } else {
            base_trust
        };

        let fb_canvas = Canvas::new(
            Rectangle::new_coords(0, 0, screensize.x, screensize.y),
            checked_base_trust, &trng, None
        ).expect("couldn't create modal canvas");
        canvases.insert(fb_canvas.gid(), fb_canvas);

        Ok(Framebuffer {
            gid: fb_canvas.gid(),
            screensize,
            visible: true,
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
    fn resize_height(&mut self, _gfx: &graphics_server::Gfx, new_height: i16, _status_canvas: &Canvas, canvases: &mut HashMap<Gid, Canvas>) -> Result<Point, xous::Error> {
        let fb_canvas = canvases.get_mut(&self.gid).expect("couldn't find my canvas");
        let orig_rect = fb_canvas.clip_rect();

        let mut fb_clip_rect = Rectangle::new_coords(orig_rect.tl().x, 0, orig_rect.br().x, new_height);
        fb_clip_rect.style = DrawStyle {fill_color: Some(PixelColor::Dark), stroke_color: None, stroke_width: 0,};
        fb_canvas.set_clip(fb_clip_rect);
        Ok(fb_clip_rect.br)
    }
    fn get_content_canvas(&self) -> Gid {
        self.gid
    }
    fn set_visibility_state(&mut self, onscreen: bool, canvases: &mut HashMap<Gid, Canvas>) {
        log::debug!("raw fb entering set_visibilty_state, self.visible {}, onscreen {}", self.visible, onscreen);
        if onscreen == self.visible {
            // nothing to do
            return
        }
        let fb_canvas = canvases.get_mut(&self.gid).expect("couldn't find my canvas");

        let offscreen = if !onscreen && self.visible {
            // move canvases off-screen
            Point::new(self.screensize.x*2, 0)
        } else if onscreen && !self.visible {
            // undo the off-screen move
            Point::new(-self.screensize.x*2, 0)
        } else {
            // should actually never reach this because of the identity check at the very top
            Point::new(0, 0)
        };
        fb_canvas.set_clip(fb_canvas.clip_rect().translate_chain(offscreen));
        self.visible = onscreen;
        log::debug!("moving raw framebuffer box by {:?}, final state: {:?}", offscreen, fb_canvas);
    }
}