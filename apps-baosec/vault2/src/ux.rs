use core::fmt::Write as TextViewWrite;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use blitstr2::GlyphStyle;
use ux_api::minigfx::*;
use ux_api::service::api::Gid;
use ux_api::service::gfx::Gfx;
use ux_api::widgets::ScrollableList;
use xous::CID;

use crate::*;

pub const DEFAULT_FONT: GlyphStyle = GlyphStyle::Bold;
pub const FONT_LIST: [&'static str; 6] = ["regular", "tall", "mono", "bold", "large", "small"];
pub fn name_to_style(name: &str) -> Option<GlyphStyle> {
    match name {
        "regular" => Some(GlyphStyle::Regular),
        "tall" => Some(GlyphStyle::Tall),
        "mono" => Some(GlyphStyle::Monospace),
        "cjk" => Some(GlyphStyle::Cjk),
        "bold" => Some(GlyphStyle::Bold),
        "large" => Some(GlyphStyle::Large),
        "small" => Some(GlyphStyle::Small),
        _ => None,
    }
}
fn style_to_name(style: &GlyphStyle) -> String {
    match style {
        GlyphStyle::Regular => "regular".to_string(),
        GlyphStyle::Monospace => "mono".to_string(),
        GlyphStyle::Cjk => "cjk".to_string(),
        GlyphStyle::Bold => "bold".to_string(),
        GlyphStyle::Large => "large".to_string(),
        GlyphStyle::Small => "small".to_string(),
        GlyphStyle::Tall => "tall".to_string(),
        _ => "regular".to_string(),
    }
}
const VAULT_CONFIG_DICT: &'static str = "vault.config";
const VAULT_CONFIG_KEY_FONT: &'static str = "fontstyle";

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
    mode: Arc<Mutex<VaultMode>>,

    /// totp redraw state
    totp_code: Option<String>,
    last_epoch: u64,

    pddb: RefCell<Pddb>,
    item_height: isize,
}

impl VaultUi {
    pub fn new(
        xns: &xous_names::XousNames,
        cid: xous::CID,
        item_lists: Arc<Mutex<ItemLists>>,
        mode: Arc<Mutex<VaultMode>>,
    ) -> Self {
        let pddb = pddb::Pddb::new();
        let mut totp_list = ScrollableList::default();
        totp_list
            .set_margin(TotpLayout::totp_margin())
            .pane_size(TotpLayout::list_box())
            .style(TotpLayout::list_font());
        for i in 0..6 {
            totp_list.add_item(0, &format!("example {}", i));
        }
        let gfx = Gfx::new(&xns).unwrap();
        let style = DEFAULT_FONT;
        let glyph_height = gfx.glyph_height_hint(style).unwrap() as isize;
        let height = gfx.screen_size().unwrap().y;
        Self {
            main_cid: cid,
            gfx,
            totp_list,
            item_lists,
            mode,
            totp_code: None,
            last_epoch: crate::totp::get_current_unix_time().expect("couldn't get current time") / 30,
            pddb: RefCell::new(pddb),
            item_height: height / glyph_height,
        }
    }

    pub(crate) fn store_glyph_style(&mut self, style: GlyphStyle) {
        self.pddb
            .borrow()
            .delete_key(VAULT_CONFIG_DICT, VAULT_CONFIG_KEY_FONT, Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS))
            .expect("couldn't delete previous setting");

        match self.pddb.borrow().get(
            VAULT_CONFIG_DICT,
            VAULT_CONFIG_KEY_FONT,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS),
            true,
            true,
            Some(32),
            Some(vault2::basis_change),
        ) {
            Ok(mut style_key) => {
                style_key.write(style_to_name(&style).as_bytes()).ok();
            }
            _ => panic!("PDDB access erorr"),
        };
        self.pddb.borrow().sync().ok();
    }

    pub(crate) fn apply_glyph_style(&mut self) {
        let style = match self.pddb.borrow().get(
            VAULT_CONFIG_DICT,
            VAULT_CONFIG_KEY_FONT,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS),
            true,
            true,
            Some(32),
            Some(vault2::basis_change),
        ) {
            Ok(mut style_key) => {
                let mut name_bytes = Vec::<u8>::new();
                match style_key.read_to_end(&mut name_bytes) {
                    Ok(_len) => {
                        log::debug!(
                            "name_bytes: {:?} {:?}",
                            name_bytes,
                            String::from_utf8(name_bytes.to_vec())
                        );
                        name_to_style(&String::from_utf8(name_bytes).unwrap_or("bold".to_string()))
                            .unwrap_or(GlyphStyle::Bold)
                    }
                    Err(_) => GlyphStyle::Bold,
                }
            }
            _ => {
                log::warn!("PDDB access error reading default glyph size");
                GlyphStyle::Bold
            }
        };
        self.totp_list.style(style);
        let glyph_height = self.gfx.glyph_height_hint(style).unwrap();
        self.item_height = glyph_height as isize + 2; // +2 because of the border width
        self.item_lists
            .lock()
            .unwrap()
            .set_items_per_screen(self.gfx.screen_size().unwrap().y / self.item_height);
    }

    /// Clear the entire screen.
    pub fn clear_area(&self) { self.gfx.clear().ok(); }

    /// Redraw the text view onto the screen.
    pub fn redraw(&mut self) {
        // to reduce locking thrash, we cache a copy of the current mode at the top of redraw.
        let mode_at_entry = (*self.mode.lock().unwrap()).clone();

        self.gfx.clear().ok();

        match mode_at_entry {
            VaultMode::Totp => {
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
            }
            VaultMode::Password => {
                /*
                Password UI --

                Home menu:

                > Search by QR code
                > Search by text entry
                > Enter new by QR code
                > Enter new by text entry

                Prefix entered:

                ---------------------
                | Selected Domain   |
                | Selected Username |
                ---------------------
                > Domain short 1
                > Domain short 2
                > Domain short 3
                > Domain short 4
                > search prefix <

                - Scrolling down to search prefix and hitting home brings up keyboard UI
                - Left-right would page through prefixed entries
                - Up down would page through just the current page of entries (allowing selection of search prefix region)
                - Selecting search prefix brings up the alphabetic entry UI
                - Pressing in on scroll button raises a new menu for "autotype password" + "delete passsword" + "edit password" options
                - Pressing on circle middle button just does autotype

                 */
                todo!()
            }
        }
        self.gfx.flush().ok();
    }

    pub(crate) fn nav(&mut self, dir: NavDir) {
        self.item_lists.lock().unwrap().nav((*self.mode.lock().unwrap()).clone(), dir);
    }

    pub(crate) fn filter(&mut self, criteria: &String) {
        self.item_lists.lock().unwrap().filter(self.mode.lock().unwrap().clone(), criteria);
    }

    pub(crate) fn selected_entry(&self) -> Option<SelectedEntry> {
        let mode = (*self.mode.lock().unwrap()).clone();
        self.item_lists.lock().unwrap().selected_entry(mode)
    }
}
