use std::collections::HashMap;

use graphics_server::*;

use crate::contexts::MISC_CONTEXT_DEFAULT_TRUST;
use crate::Canvas;
use crate::{LayoutApi, LayoutBehavior};
const TRUST_OFFSET: u8 = 0;

#[derive(Debug, Copy, Clone)]
pub(crate) struct MenuLayout {
    pub menu: Gid,
    menu_y_pad: i16,
    _menu_x_pad: i16,
    menu_min_height: i16,
    screensize: Point,
    _height: i16,
}
impl MenuLayout {
    pub fn init(
        gfx: &graphics_server::Gfx,
        trng: &trng::Trng,
        canvases: &mut HashMap<Gid, Canvas>,
    ) -> Result<MenuLayout, xous::Error> {
        let screensize = gfx.screen_size().expect("Couldn't get screen size");
        // get the height of various text regions to compute the layout
        let height: i16 = gfx.glyph_height_hint(gam::SYSTEM_STYLE).expect("couldn't get glyph height") as i16;

        const MENU_Y_PAD: i16 = 100;
        const MENU_X_PAD: i16 = 35;
        // build for an initial size of 1 entry
        let menu_canvas = Canvas::new(
            Rectangle::new_coords(MENU_X_PAD, MENU_Y_PAD, screensize.x - MENU_X_PAD, MENU_Y_PAD + height),
            MISC_CONTEXT_DEFAULT_TRUST - TRUST_OFFSET,
            &trng,
            None,
            crate::api::CanvasType::Menu,
        )
        .expect("couldn't create menu canvas");
        let gid = menu_canvas.gid();
        canvases.insert(menu_canvas.gid(), menu_canvas);

        Ok(MenuLayout {
            menu: gid,
            menu_y_pad: MENU_Y_PAD,
            _menu_x_pad: MENU_X_PAD,
            menu_min_height: height,
            screensize,
            _height: height, // start with "minimum" size and grow up as items are added
        })
    }
}
impl LayoutApi for MenuLayout {
    fn behavior(&self) -> LayoutBehavior { LayoutBehavior::Alert }

    fn clear(
        &self,
        gfx: &graphics_server::Gfx,
        canvases: &mut HashMap<Gid, Canvas>,
    ) -> Result<(), xous::Error> {
        let menu_canvas = canvases.get(&self.menu).expect("couldn't find menu canvas");

        let mut rect = menu_canvas.clip_rect();
        rect.style = DrawStyle { fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0 };
        gfx.draw_rectangle(rect)
    }

    fn resize_height(
        &mut self,
        _gfx: &graphics_server::Gfx,
        new_height: i16,
        _status_canvas: &Rectangle,
        canvases: &mut HashMap<Gid, Canvas>,
    ) -> Result<Point, xous::Error> {
        let menu_canvas = canvases.get_mut(&self.menu).expect("couldn't find menu canvas");
        let orig_rect = menu_canvas.clip_rect();

        let mut height: i16 = if new_height < self.menu_min_height {
            self.menu_min_height + self.menu_y_pad
        } else {
            new_height + self.menu_y_pad
        };
        if height > self.screensize.y - self.menu_y_pad {
            height = self.screensize.y - self.menu_y_pad;
        }
        let mut menu_clip_rect =
            Rectangle::new_coords(orig_rect.tl().x, self.menu_y_pad, orig_rect.br().x, height);
        menu_clip_rect.style =
            DrawStyle { fill_color: Some(PixelColor::Dark), stroke_color: None, stroke_width: 0 };
        menu_canvas.set_clip(menu_clip_rect);
        // gfx.draw_rectangle(menu_clip_rect).expect("can't clear menu");
        Ok(menu_clip_rect.br)
    }

    fn get_gids(&self) -> Vec<crate::api::GidRecord> {
        vec![crate::api::GidRecord { gid: self.menu, canvas_type: crate::api::CanvasType::Menu }]
    }

    fn set_visibility_state(&mut self, onscreen: bool, canvases: &mut HashMap<Gid, Canvas>) {
        let menu_canvas = canvases.get_mut(&self.menu).expect("couldn't find menu canvas");
        log::debug!("menu entering set_visibilty_state, {}->{}", menu_canvas.is_onscreen(), onscreen);
        menu_canvas.set_onscreen(onscreen);
    }
}
