use std::collections::HashMap;

use graphics_server::*;

use crate::api::CanvasType;
use crate::contexts::MISC_CONTEXT_DEFAULT_TRUST;
use crate::Canvas;
use crate::{LayoutApi, LayoutBehavior};
const TRUST_OFFSET: u8 = 16;

#[derive(Debug, Copy, Clone)]
pub(crate) struct ModalLayout {
    pub modal: Gid,
    modal_y_pad: i16,
    _modal_x_pad: i16,
    modal_min_height: i16,
    screensize: Point,
    _height: i16,
    _modal_y_max: i16,
}
impl ModalLayout {
    pub fn init(
        gfx: &graphics_server::Gfx,
        trng: &trng::Trng,
        canvases: &mut HashMap<Gid, Canvas>,
    ) -> Result<ModalLayout, xous::Error> {
        let screensize = gfx.screen_size().expect("Couldn't get screen size");
        // get the height of various text regions to compute the layout
        let height: i16 = gfx.glyph_height_hint(gam::SYSTEM_STYLE).expect("couldn't get glyph height") as i16;

        const MODAL_Y_PAD: i16 = 80;
        const MODAL_X_PAD: i16 = 20;
        // base trust - 1 so that status bar can always ride on top
        let modal_canvas = Canvas::new(
            Rectangle::new_coords(
                MODAL_X_PAD,
                MODAL_Y_PAD,
                screensize.x - MODAL_X_PAD,
                crate::api::MODAL_Y_MAX,
            ),
            MISC_CONTEXT_DEFAULT_TRUST - TRUST_OFFSET,
            &trng,
            None,
            CanvasType::Modal,
        )
        .expect("couldn't create modal canvas");
        let modal_gid = modal_canvas.gid();
        canvases.insert(modal_canvas.gid(), modal_canvas);

        Ok(ModalLayout {
            modal: modal_gid,
            modal_y_pad: MODAL_Y_PAD,
            _modal_x_pad: MODAL_X_PAD,
            modal_min_height: height,
            screensize,
            _height: screensize.y - MODAL_Y_PAD, /* start with the "maximum" size, and shrink down once
                                                  * items are known */
            _modal_y_max: crate::api::MODAL_Y_MAX,
        })
    }
}
impl LayoutApi for ModalLayout {
    fn behavior(&self) -> LayoutBehavior { LayoutBehavior::Alert }

    fn clear(
        &self,
        gfx: &graphics_server::Gfx,
        canvases: &mut HashMap<Gid, Canvas>,
    ) -> Result<(), xous::Error> {
        let modal_canvas = canvases.get(&self.modal).expect("couldn't find modal canvas");

        let mut rect = modal_canvas.clip_rect();
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
        let modal_canvas = canvases.get_mut(&self.modal).expect("couldn't find modal canvas");
        let orig_rect = modal_canvas.clip_rect();

        let mut height: i16 = if new_height < self.modal_min_height {
            self.modal_min_height + self.modal_y_pad
        } else {
            new_height + self.modal_y_pad
        };
        if height > self.screensize.y - self.modal_y_pad {
            height = self.screensize.y - self.modal_y_pad;
        }
        let mut modal_clip_rect =
            Rectangle::new_coords(orig_rect.tl().x, self.modal_y_pad, orig_rect.br().x, height);
        modal_clip_rect.style =
            DrawStyle { fill_color: Some(PixelColor::Dark), stroke_color: None, stroke_width: 0 };
        modal_canvas.set_clip(modal_clip_rect);
        // gfx.draw_rectangle(menu_clip_rect).expect("can't clear menu");
        Ok(modal_clip_rect.br)
    }

    fn get_gids(&self) -> Vec<crate::api::GidRecord> {
        vec![crate::api::GidRecord { gid: self.modal, canvas_type: CanvasType::Modal }]
    }

    fn set_visibility_state(&mut self, onscreen: bool, canvases: &mut HashMap<Gid, Canvas>) {
        let modal_canvas = canvases.get_mut(&self.modal).expect("couldn't find modal canvas");
        log::debug!("modal box entering set_visibilty_state {}->{}", modal_canvas.is_onscreen(), onscreen);
        modal_canvas.set_onscreen(onscreen);
    }
}
