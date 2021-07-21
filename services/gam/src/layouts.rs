use crate::Canvas;

use heapless::FnvIndexMap;
use blitstr_ref as blitstr;
use blitstr::GlyphStyle;
use graphics_server::*;

use crate::{LayoutApi, LayoutBehavior};

#[derive(Debug, Copy, Clone)]
// GIDs of canvases that are used the "Chat" layout.
pub(crate) struct ChatLayout {
    // a set of GIDs to track the elements of the chat layout
    pub content: Gid,
    pub predictive: Gid,
    pub input: Gid,

    // my internal bookkeeping records. Allow input area to grow into content area
    min_content_height: i16,
    min_input_height: i16,
    screensize: Point,
    small_height: i16,
    regular_height: i16,
    visible: bool,
}
impl ChatLayout {
    // pass in the status canvas so we can size around it, but we can't draw on it
    pub fn init(gfx: &graphics_server::Gfx, trng: &trng::Trng, base_trust: u8,
        status_canvas: &Canvas, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<ChatLayout, xous::Error> {
        let screensize = gfx.screen_size().expect("Couldn't get screen size");
        // get the height of various text regions to compute the layout
        let small_height: i16 = gfx.glyph_height_hint(GlyphStyle::Small).expect("couldn't get glyph height") as i16;
        let regular_height: i16 = gfx.glyph_height_hint(GlyphStyle::Regular).expect("couldn't get glyph height") as i16;
        let margin = 4;

        let checked_base_trust = if base_trust < 4 {
            4
        } else {
            base_trust
        };

        // allocate canvases in structures, and record their GID for future reference
        // base trust - 2 so that main menu + status bar always ride on top
        let predictive_canvas = Canvas::new(
            Rectangle::new_coords(0, screensize.y - regular_height - margin*2, screensize.x, screensize.y),
            checked_base_trust - 2,
            &trng, None
        ).expect("couldn't create predictive text canvas");
        canvases.insert(predictive_canvas.gid(), predictive_canvas).expect("couldn't store predictive canvas");

        let min_input_height = regular_height + margin*2;
        let input_canvas = Canvas::new(
            Rectangle::new_v_stack(predictive_canvas.clip_rect(), -min_input_height),
         checked_base_trust - 2, &trng, None
        ).expect("couldn't create input text canvas");
        canvases.insert(input_canvas.gid(), input_canvas).expect("couldn't store input canvas");

        let content_canvas = Canvas::new(
            Rectangle::new_v_span(status_canvas.clip_rect(), input_canvas.clip_rect()),
            checked_base_trust / 2, &trng, None
        ).expect("couldn't create content canvas");
        canvases.insert(content_canvas.gid(), content_canvas).expect("can't store content canvas");

        Ok(ChatLayout {
            content: content_canvas.gid(),
            predictive: predictive_canvas.gid(),
            input: input_canvas.gid(),
            min_content_height: 64,
            min_input_height,
            screensize,
            small_height,
            regular_height,
            visible: true,
        })
    }
}
impl LayoutApi for ChatLayout {
    fn behavior(&self) -> LayoutBehavior {
        LayoutBehavior::App
    }
    fn clear(&self, gfx: &graphics_server::Gfx, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<(), xous::Error> {
        let input_canvas = canvases.get(&self.input).expect("couldn't find input canvas");
        let content_canvas = canvases.get(&self.content).expect("couldn't find content canvas");
        let predictive_canvas = canvases.get(&self.predictive).expect("couldn't find predictive canvas");

        let mut rect = content_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        gfx.draw_rectangle(rect).expect("can't clear canvas");

        let mut rect = predictive_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        gfx.draw_rectangle(rect).expect("can't clear canvas");

        let mut rect = input_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        gfx.draw_rectangle(rect).expect("can't clear canvas");
        Ok(())
    }
    fn resize_height(&mut self, gfx: &graphics_server::Gfx, new_height: i16, status_canvas: &Canvas, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<Point, xous::Error> {
        let input_canvas = canvases.get(&self.input).expect("couldn't find input canvas");
        let predictive_canvas = canvases.get(&self.predictive).expect("couldn't find predictive canvas");

        let height: i16 = if new_height < self.min_input_height {
            self.min_input_height
        } else {
            new_height
        };
        let mut new_input_rect = Rectangle::new_v_stack(predictive_canvas.clip_rect(), -height);
        let mut new_content_rect = Rectangle::new_v_span(status_canvas.clip_rect(), new_input_rect);
        if (new_content_rect.br.y - new_content_rect.tl.y) > self.min_content_height {
            {
                let input_canvas_mut = canvases.get_mut(&self.input).expect("couldn't find input canvas");
                input_canvas_mut.set_clip(new_input_rect);
                new_input_rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
                gfx.draw_rectangle(new_input_rect).expect("can't clear canvas");
                    }
            {
                let content_canvas_mut = canvases.get_mut(&self.content).expect("couldn't find content canvas");
                content_canvas_mut.set_clip(new_content_rect);
                new_content_rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
                gfx.draw_rectangle(new_content_rect).expect("can't clear canvas");
            }
            // we resized to this new height
            Ok(new_content_rect.br)
        } else {
            // we didn't resize anything, height unchanged
            Ok(input_canvas.clip_rect().br)
        }
    }
    fn get_input_canvas(&self) -> Option<Gid> {
        Some(self.input)
    }
    fn get_prediction_canvas(&self) -> Option<Gid> {
        Some(self.predictive)
    }
    fn get_content_canvas(&self) -> Gid {
        self.content
    }
    fn set_visibility_state(&mut self, onscreen: bool, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) {
        log::debug!("chatlayout: set_visibility_state onscreen {}, self.visible {}", onscreen, self.visible);
        if onscreen == self.visible {
            log::trace!("chatlayout: no change to visibility, moving on");
            // nothing to do
            return
        }
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
        log::debug!("chatlayout: shifting canvas for input by {:?}", offscreen);
        let input_canvas = canvases.get_mut(&self.input).expect("couldn't find input canvas");
        input_canvas.set_clip(input_canvas.clip_rect().translate_chain(offscreen));

        let content_canvas = canvases.get_mut(&self.content).expect("couldn't find content canvas");
        content_canvas.set_clip(content_canvas.clip_rect().translate_chain(offscreen));

        let predictive_canvas = canvases.get_mut(&self.predictive).expect("couldn't find predictive canvas");
        predictive_canvas.set_clip(predictive_canvas.clip_rect().translate_chain(offscreen));

        self.visible = onscreen;
    }
}

// remember GIDs of the canvases for menus
#[derive(Debug, Copy, Clone)]
pub(crate) struct MenuLayout {
    pub menu: Gid,
    menu_y_pad: i16,
    menu_x_pad: i16,
    menu_min_height: i16,
    screensize: Point,
    height: i16,
    visible: bool,
}
impl MenuLayout {
    pub fn init(gfx: &graphics_server::Gfx, trng: &trng::Trng, base_trust: u8, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<MenuLayout, xous::Error> {
        let screensize = gfx.screen_size().expect("Couldn't get screen size");
        // get the height of various text regions to compute the layout
        let height: i16 = gfx.glyph_height_hint(GlyphStyle::Regular).expect("couldn't get glyph height") as i16;

        let checked_base_trust = if base_trust < 4 {
            4
        } else {
            base_trust
        };

        const MENU_Y_PAD: i16 = 100;
        const MENU_X_PAD: i16 = 35;
        // build for an initial size of 1 entry
        // base trust - 1 so that status bar can always ride on top
        let menu_canvas = Canvas::new(
            Rectangle::new_coords(MENU_X_PAD, MENU_Y_PAD, screensize.x - MENU_X_PAD, MENU_Y_PAD + height),
            checked_base_trust - 1, &trng, None
        ).expect("couldn't create menu canvas");
        canvases.insert(menu_canvas.gid(), menu_canvas).expect("can't store menu canvas");

        Ok(MenuLayout {
            menu: menu_canvas.gid(),
            menu_y_pad: MENU_Y_PAD,
            menu_x_pad: MENU_X_PAD,
            menu_min_height: height,
            screensize,
            height,
            visible: true,
        })
    }
}
impl LayoutApi for MenuLayout {
    fn behavior(&self) -> LayoutBehavior {
        LayoutBehavior::Alert
    }
    fn clear(&self, gfx: &graphics_server::Gfx, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<(), xous::Error> {
        let menu_canvas = canvases.get(&self.menu).expect("couldn't find menu canvas");

        let mut rect = menu_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        gfx.draw_rectangle(rect)
    }
    fn resize_height(&mut self, _gfx: &graphics_server::Gfx, new_height: i16, _status_canvas: &Canvas, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<Point, xous::Error> {
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
        let mut menu_clip_rect = Rectangle::new_coords(orig_rect.tl().x, self.menu_y_pad, orig_rect.br().x, height);
        menu_clip_rect.style = DrawStyle {fill_color: Some(PixelColor::Dark), stroke_color: None, stroke_width: 0,};
        menu_canvas.set_clip(menu_clip_rect);
        // gfx.draw_rectangle(menu_clip_rect).expect("can't clear menu");
        Ok(menu_clip_rect.br)
    }
    fn get_content_canvas(&self) -> Gid {
        self.menu
    }
    fn set_visibility_state(&mut self, onscreen: bool, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) {
        log::debug!("menu entering set_visibilty_state, self.visible {}, onscreen {}", self.visible, onscreen);
        if onscreen == self.visible {
            // nothing to do
            return
        }
        let menu_canvas = canvases.get_mut(&self.menu).expect("couldn't find menu canvas");

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
        log::debug!("moving menu by {:?}", offscreen);
        menu_canvas.set_clip(menu_canvas.clip_rect().translate_chain(offscreen));
        self.visible = onscreen;
    }
}


// remember GIDs of the canvases for menus
#[derive(Debug, Copy, Clone)]
pub(crate) struct ModalLayout {
    pub modal: Gid,
    modal_y_pad: i16,
    modal_x_pad: i16,
    modal_min_height: i16,
    screensize: Point,
    height: i16,
    visible: bool,
}
impl ModalLayout {
    pub fn init(gfx: &graphics_server::Gfx, trng: &trng::Trng, base_trust: u8, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<ModalLayout, xous::Error> {
        let screensize = gfx.screen_size().expect("Couldn't get screen size");
        // get the height of various text regions to compute the layout
        let height: i16 = gfx.glyph_height_hint(GlyphStyle::Regular).expect("couldn't get glyph height") as i16;

        let checked_base_trust = if base_trust < 4 {
            4
        } else {
            base_trust
        };

        const MODAL_Y_PAD: i16 = 100;
        const MODAL_X_PAD: i16 = 20;
        // base trust - 1 so that status bar can always ride on top
        let modal_canvas = Canvas::new(
            Rectangle::new_coords(MODAL_X_PAD, MODAL_Y_PAD, screensize.x - MODAL_X_PAD, MODAL_Y_PAD + height),
            checked_base_trust - 1, &trng, None
        ).expect("couldn't create modal canvas");
        canvases.insert(modal_canvas.gid(), modal_canvas).expect("can't store modal canvas");

        Ok(ModalLayout {
            modal: modal_canvas.gid(),
            modal_y_pad: MODAL_Y_PAD,
            modal_x_pad: MODAL_X_PAD,
            modal_min_height: height,
            screensize,
            height,
            visible: true,
        })
    }
}
impl LayoutApi for ModalLayout {
    fn behavior(&self) -> LayoutBehavior {
        LayoutBehavior::Alert
    }
    fn clear(&self, gfx: &graphics_server::Gfx, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<(), xous::Error> {
        let modal_canvas = canvases.get(&self.modal).expect("couldn't find modal canvas");

        let mut rect = modal_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        gfx.draw_rectangle(rect)
    }
    fn resize_height(&mut self, _gfx: &graphics_server::Gfx, new_height: i16, _status_canvas: &Canvas, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) -> Result<Point, xous::Error> {
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
        let mut modal_clip_rect = Rectangle::new_coords(orig_rect.tl().x, self.modal_y_pad, orig_rect.br().x, height);
        modal_clip_rect.style = DrawStyle {fill_color: Some(PixelColor::Dark), stroke_color: None, stroke_width: 0,};
        modal_canvas.set_clip(modal_clip_rect);
        // gfx.draw_rectangle(menu_clip_rect).expect("can't clear menu");
        Ok(modal_clip_rect.br)
    }
    fn get_content_canvas(&self) -> Gid {
        self.modal
    }
    fn set_visibility_state(&mut self, onscreen: bool, canvases: &mut FnvIndexMap<Gid, Canvas, {crate::MAX_CANVASES}>) {
        log::debug!("modal box entering set_visibilty_state, self.visible {}, onscreen {}", self.visible, onscreen);
        if onscreen == self.visible {
            // nothing to do
            return
        }
        let modal_canvas = canvases.get_mut(&self.modal).expect("couldn't find modal canvas");

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
        log::debug!("moving modal box by {:?}", offscreen);
        modal_canvas.set_clip(modal_canvas.clip_rect().translate_chain(offscreen));
        self.visible = onscreen;
    }
}