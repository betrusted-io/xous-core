use core::fmt::Write as TextViewWrite;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use blitstr2::GlyphStyle;
use ux_api::minigfx::*;
use ux_api::service::api::Gid;
use ux_api::service::gfx::Gfx;
use ux_api::widgets::ScrollableList;
use xous::CID;

use crate::storage::{ContentKind, Manager};
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
    style: GlyphStyle,
    storage_manager: Manager,
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
        totp_list.set_autoflush(false);

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
            style,
            storage_manager: Manager::new(xns),
        }
    }

    pub(crate) fn refresh_totp(&mut self) {
        let mut locked_lists = self.item_lists.lock().unwrap();
        let full_list = locked_lists.full_list(VaultMode::Totp);
        self.totp_list.clear();
        for item in full_list {
            self.totp_list.add_item(0, &item.name());
        }
    }

    pub(crate) fn update_selected_totp_code(&mut self) {
        if self.totp_list.len() > 0 {
            let selected = self.totp_list.get_selected();
            let mut locked_lists = self.item_lists.lock().unwrap();
            let full_list = locked_lists.full_list(VaultMode::Totp);
            if let Some(selected_item) = full_list.iter().find(|item| item.name() == selected) {
                match crate::totp::db_str_to_code(&selected_item.extra) {
                    Ok(s) => self.totp_code = Some(s),
                    _ => self.totp_code = None,
                }
            }
        }
    }

    pub(crate) fn basis_change(&mut self) {
        self.item_lists.lock().unwrap().clear_all();
        self.totp_list.clear();
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
        self.item_lists.lock().unwrap().set_items_per_screen(
            (self.gfx.screen_size().unwrap().y - 2 * self.item_height) / self.item_height,
        );
        self.style = style;
    }

    /// Clear the entire screen.
    pub fn clear_area(&self) { self.gfx.clear().ok(); }

    /// Redraw the text view onto the screen.
    pub fn redraw(&mut self) {
        // to reduce locking thrash, we cache a copy of the current mode at the top of redraw.
        let mode_at_entry = (*self.mode.lock().unwrap()).clone();

        self.clear_area();

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

                if self.totp_code.is_none() && self.totp_list.len() > 0 {
                    // this handles initial population of the field
                    self.update_selected_totp_code();
                }

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
                let mut object_list = ObjectList::new();
                let mut timer_box = TotpLayout::timer_box();
                timer_box.style = DrawStyle::new(PixelColor::Dark, PixelColor::Light, 1);
                object_list.push(ClipObjectType::Rect(timer_box)).unwrap();

                // draw the duration bar
                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .map(|duration| duration.as_millis())
                    .expect("couldn't get time as millis");

                // manage the epoch as well
                let epoch = (current_time / (30 * 1000)) as u64;
                if self.last_epoch != epoch {
                    self.last_epoch = epoch;
                    self.update_selected_totp_code();
                }

                let mut timer_remaining = TotpLayout::timer_box();
                let delta = (current_time - (self.last_epoch as u128 * 30 * 1000)) as isize;
                let width = timer_remaining.width() as isize;
                let delta_width = (delta * width * 128) / (30 * 128 * 1000);
                timer_remaining.br = Point::new(width - delta_width, timer_remaining.br().y);
                timer_remaining.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 1);
                object_list.push(ClipObjectType::Rect(timer_remaining)).unwrap();
                self.gfx.draw_object_list(object_list).unwrap();
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
                let screensize = self.gfx.screen_size().unwrap();
                // handle empty database case
                if self.item_lists.lock().unwrap().filter_len(VaultMode::Password) == 0 {
                    log::debug!("no items");
                    let mut box_text = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredBot(Rectangle::new(
                            Point::new(0, screensize.y / 2),
                            Point::new(screensize.x, screensize.y / 2 + self.item_height),
                        )),
                    );
                    box_text.draw_border = false;
                    box_text.clear_area = true;
                    box_text.style = self.style;
                    write!(box_text, "{}", t!("vault.no_items", locales::LANG)).ok();
                    self.gfx.draw_textview(&mut box_text).expect("couldn't post empty notification");
                    self.gfx.flush().ok();
                    return;
                }
                let mut insert_at = 0;
                if let Some(entry) = self.item_lists.lock().unwrap().selected_entry(VaultMode::Password) {
                    log::debug!("rendering entry {:?}", entry);
                    // draw more data about the selected item
                    let guid = entry.key_guid.as_str();
                    let pw: storage::PasswordRecord =
                        match self.storage_manager.get_record(&ContentKind::Password, guid) {
                            Ok(record) => record,
                            Err(error) => {
                                log::error!("internal error rendering password: {:?}", error);
                                self.gfx.flush().ok();
                                return;
                            }
                        };
                    let mut box_text = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredBot(Rectangle::new(
                            Point::new(0, insert_at),
                            Point::new(screensize.x, insert_at + self.item_height),
                        )),
                    );
                    insert_at += self.item_height;
                    box_text.draw_border = false;
                    box_text.clear_area = false;
                    box_text.ellipsis = true;
                    box_text.style = self.style;
                    box_text.invert = true;
                    // line 1
                    write!(box_text, "{}", &pw.description).ok();
                    self.gfx.draw_textview(&mut box_text).unwrap();
                    // line 2
                    box_text.bounds_hint = TextBounds::CenteredBot(Rectangle::new(
                        Point::new(0, insert_at),
                        Point::new(screensize.x, insert_at + self.item_height),
                    ));
                    insert_at += self.item_height;
                    box_text.clear_str();
                    write!(box_text, "{}", &pw.username).ok();
                    self.gfx.draw_textview(&mut box_text).ok();
                    // draw a rectangle around the top area
                    self.gfx
                        .draw_rectangle(Rectangle::new_coords_with_style(
                            0,
                            0,
                            screensize.x,
                            self.item_height * 2,
                            DrawStyle {
                                fill_color: None,
                                stroke_color: Some(PixelColor::Light),
                                stroke_width: 2,
                            },
                        ))
                        .ok();
                } else {
                    // draw a rectangle around the top area
                    self.gfx
                        .draw_rectangle(Rectangle::new_coords_with_style(
                            0,
                            0,
                            screensize.x,
                            self.item_height * 2,
                            DrawStyle {
                                fill_color: Some(PixelColor::Dark),
                                stroke_color: Some(PixelColor::Light),
                                stroke_width: 2,
                            },
                        ))
                        .ok();
                    log::error!("Couldn't retrieve password info to render top area");
                    insert_at = self.item_height * 2;
                };
                // ---- draw list body area ----
                let selected = self.item_lists.lock().unwrap().selected_index(VaultMode::Password);
                let mut guarded_list = self.item_lists.lock().unwrap();
                let current_page = guarded_list.selected_page(VaultMode::Password);
                log::debug!("current_page len {}", current_page.len());
                for (index, item) in current_page.iter_mut().enumerate() {
                    if insert_at - 1 > screensize.y - self.item_height {
                        // -1 because of the overlapping border
                        break;
                    }
                    log::debug!("drawing {}", item.name());
                    let mut box_text = TextView::new(
                        Gid::dummy(),
                        TextBounds::BoundingBox(Rectangle::new(
                            Point::new(0, insert_at),
                            Point::new(screensize.x, insert_at + self.item_height),
                        )),
                    );
                    box_text.draw_border = false;
                    box_text.rounded_border = None;
                    box_text.clear_area = false;
                    box_text.style = self.style;
                    box_text.ellipsis = true;
                    if index == selected {
                        box_text.invert = false;
                    } else {
                        box_text.invert = true;
                    }
                    write!(box_text, "{}", item.name()).ok();
                    // do a dry run to get the final bounding box
                    box_text.set_dry_run(true);
                    self.gfx.draw_textview(&mut box_text).expect("couldn't post list item");
                    if index == selected {
                        let mut r = box_text.bounds_computed.unwrap();
                        r.style = DrawStyle {
                            fill_color: Some(PixelColor::Light),
                            stroke_color: None,
                            stroke_width: 0,
                        };
                        self.gfx.draw_rectangle(r).ok();
                    }
                    // now draw for reals
                    box_text.set_dry_run(false);
                    self.gfx.draw_textview(&mut box_text).expect("couldn't post list item");

                    insert_at += self.item_height;
                }
            }
        }
        self.gfx.flush().ok();
    }

    pub(crate) fn nav(&mut self, dir: NavDir) {
        let mode_at_entry = (*self.mode.lock().unwrap()).clone();
        match mode_at_entry {
            VaultMode::Password => {
                self.item_lists.lock().unwrap().nav((*self.mode.lock().unwrap()).clone(), dir);
            }
            VaultMode::Totp => {
                match dir {
                    NavDir::Up => {
                        self.totp_list.key_action('↑');
                    }
                    NavDir::Down => {
                        self.totp_list.key_action('↓');
                    }
                    _ => unimplemented!(),
                }
                self.totp_code = None;
            }
        }
    }

    pub(crate) fn filter(&mut self, criteria: &String) {
        self.item_lists.lock().unwrap().filter(self.mode.lock().unwrap().clone(), criteria);
    }

    pub(crate) fn selected_entry(&self) -> Option<SelectedEntry> {
        let mode = (*self.mode.lock().unwrap()).clone();
        self.item_lists.lock().unwrap().selected_entry(mode)
    }
}
