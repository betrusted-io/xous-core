use core::fmt::Write;
use std::sync::{Arc, Mutex};

use blitstr2::GlyphStyle;
use ux_api::minigfx::*;
use ux_api::service::api::Gid;
use ux_api::service::gfx::Gfx;
use ux_api::widgets::ScrollableList;
use xous::CID;

use crate::ItemLists;
use crate::VaultMode;

pub enum NavDir {
    Up,
    Down,
    PageUp,
    PageDown,
}

/// Centralizes tunable UI parameters for TOTP
struct TotpLayout {}
impl TotpLayout {
    pub fn totp_box() -> RoundedRectangle {
        RoundedRectangle::new(Rectangle::new(Point::new(0, 0), Point::new(127, 40)), 0)
    }

    /// Vertical margin for the font because the centering algorithm also aligns-top, and we want a little
    /// more verticale space for aesthetic reasons than the centering algorithm gives by default.
    pub fn totp_font_vmargin() -> Point { Point::new(0, 4) }

    pub fn totp_margin() -> Point { Point::new(10, 0) }

    pub fn totp_font() -> GlyphStyle { GlyphStyle::ExtraLarge }

    pub fn timer_box() -> Rectangle { Rectangle::new(Point::new(0, 40), Point::new(127, 50)) }

    pub fn list_box() -> Rectangle { Rectangle::new(Point::new(0, 50), Point::new(127, 127)) }

    pub fn list_font() -> GlyphStyle { GlyphStyle::Bold }
}

pub struct VaultUi {
    main_cid: CID,
    gfx: Gfx,
    totp_list: ScrollableList,
    item_lists: Arc<Mutex<ItemLists>>,

    /// totp redraw state
    totp_code: Option<String>,
    last_epoch: u64,
}

impl VaultUi {
    pub fn new(xns: &xous_names::XousNames, cid: xous::CID, item_lists: Arc<Mutex<ItemLists>>) -> Self {
        let mut totp_list = ScrollableList::default()
            .set_margin(TotpLayout::totp_margin())
            .pane_size(TotpLayout::list_box())
            .style(TotpLayout::list_font());
        for i in 0..6 {
            totp_list.add_item(0, &format!("example {}", i));
        }
        Self {
            main_cid: cid,
            gfx: Gfx::new(&xns).unwrap(),
            totp_list,
            item_lists,
            totp_code: None,
            last_epoch: crate::totp::get_current_unix_time().expect("couldn't get current time") / 30,
        }
    }

    /// Clear the entire screen.
    pub fn clear_area(&self) { self.gfx.clear().ok(); }

    /// Redraw the text view onto the screen.
    pub fn redraw_totp(&mut self) {
        self.gfx.clear().ok();

        // decorative box around code
        let mut totp_box = TotpLayout::totp_box();
        totp_box.border.style = DrawStyle::new(PixelColor::Dark, PixelColor::Light, 1);
        self.gfx.draw_rounded_rectangle(totp_box).ok();

        // the TOTP code
        let mut tv = TextView::new(
            Gid::dummy(),
            TextBounds::CenteredTop(
                TotpLayout::totp_box().border.translate_chain(TotpLayout::totp_font_vmargin()),
            ),
        );
        tv.invert = true;
        tv.margin = Point::new(0, 0);
        tv.style = TotpLayout::totp_font();
        tv.draw_border = false;

        match &self.totp_code {
            Some(code) => {
                write!(tv, "{}", code).ok();
            }
            _ => {
                write!(tv, "******").ok();
            }
        }
        self.gfx.draw_textview(&mut tv).expect("couldn't draw text");

        // list of codes to pick from
        self.totp_list.draw(TotpLayout::timer_box().br().y);

        // draw the timer element
        let mut timer_box = TotpLayout::timer_box();
        timer_box.style = DrawStyle::new(PixelColor::Dark, PixelColor::Light, 1);
        self.gfx.draw_rectangle(timer_box).ok();

        // draw the duration bar
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .expect("couldn't get time as millis");

        // manage the epoch as well
        let epoch = (current_time / (30 * 1000)) as u64;
        if self.last_epoch != epoch {
            self.last_epoch = epoch;
            if !self.item_lists.lock().unwrap().is_db_empty(VaultMode::Totp) {
                match crate::totp::db_str_to_code(
                    &self.item_lists.lock().unwrap().selected_extra(VaultMode::Totp),
                ) {
                    Ok(s) => self.totp_code = Some(s),
                    _ => self.totp_code = None,
                }
            }
        }

        let mut timer_remaining = TotpLayout::timer_box();
        let delta = (current_time - (self.last_epoch as u128 * 30 * 1000)) as isize;
        let width = timer_remaining.width() as isize;
        let delta_width = (delta * width * 128) / (30 * 128 * 1000);
        timer_remaining.br = Point::new(width - delta_width, timer_remaining.br().y);
        timer_remaining.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
        self.gfx.draw_rectangle(timer_remaining).ok();

        self.gfx.flush().ok();
    }
}
