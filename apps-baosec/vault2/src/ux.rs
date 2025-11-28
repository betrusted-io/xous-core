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

const FAST_SCROLL_DELAY_MS: u64 = 1300;
const KEYUP_DELAY_MS: u64 = 100;
/// How many elements to skip through on fast scroll
const PAGE_INCREMENT: usize = 6;

pub const DEFAULT_FONT: GlyphStyle = GlyphStyle::Regular;
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
    Autotype,
    Reserved,
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

    pub fn list_font() -> GlyphStyle { GlyphStyle::Regular }
}

pub struct VaultUi {
    main_cid: CID,
    gfx: Gfx,
    display_list: ScrollableList,
    item_lists: Arc<Mutex<ItemLists>>,
    mode: Arc<Mutex<VaultMode>>,

    /// totp redraw state
    totp_code: Option<String>,
    last_epoch: u64,

    pddb: RefCell<Pddb>,
    item_height: isize,
    style: GlyphStyle,
    storage_manager: Manager,

    usb_dev: usb_bao1x::UsbHid,
    last_key_time: u64,
    start_hold_time: u64,
    tt: ticktimer_server::Ticktimer,
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

        let tt = ticktimer_server::Ticktimer::new().unwrap();
        let now = tt.elapsed_ms();
        let gfx = Gfx::new(&xns).unwrap();
        let style = DEFAULT_FONT;
        let glyph_height = gfx.glyph_height_hint(style).unwrap() as isize;
        let height = gfx.screen_size().unwrap().y;
        Self {
            main_cid: cid,
            gfx,
            display_list: totp_list,
            item_lists,
            mode,
            totp_code: None,
            last_epoch: crate::totp::get_current_unix_time().expect("couldn't get current time") / 30,
            pddb: RefCell::new(pddb),
            item_height: height / glyph_height,
            style,
            storage_manager: Manager::new(xns),
            usb_dev: usb_bao1x::UsbHid::new(),
            tt,
            last_key_time: now,
            start_hold_time: now,
        }
    }

    pub(crate) fn refresh_draw_list(&mut self) {
        let mode = { (*self.mode.lock().unwrap()).clone() };

        let mut locked_lists = if let Ok(g) = self.item_lists.try_lock() {
            g
        } else {
            log::warn!("Couldn't get lock in refresh_draw_list; aborting the refresh");
            return;
        };
        let full_list = locked_lists.full_list(mode);
        self.display_list.clear();
        for item in full_list {
            self.display_list.add_item(0, &item.name());
        }
    }

    pub(crate) fn update_selected_totp_code(&mut self) -> Option<String> {
        if *self.mode.lock().unwrap() != VaultMode::Totp {
            return None;
        }
        if self.display_list.len() > 0 {
            let selected = self.display_list.get_selected();
            let mut locked_lists = self.item_lists.lock().unwrap();
            let full_list = locked_lists.full_list(VaultMode::Totp);
            if let Some(selected_item) = full_list.iter().find(|item| item.name() == selected) {
                match crate::totp::db_str_to_code(&selected_item.extra) {
                    Ok(s) => {
                        self.totp_code = Some(s.clone());
                        Some(s)
                    }
                    _ => {
                        self.totp_code = None;
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    pub(crate) fn get_selected_item(&self) -> Option<ListItem> {
        let mode = *self.mode.lock().unwrap();
        if self.display_list.len() > 0 {
            let selected = self.display_list.get_selected();
            let mut locked_lists = self.item_lists.lock().unwrap();
            let full_list = locked_lists.full_list(mode);
            full_list.iter().find(|&item| item.name() == selected).cloned()
        } else {
            None
        }
    }

    pub(crate) fn selected_entry(&self) -> Option<SelectedEntry> {
        let mode = *self.mode.lock().unwrap();
        if let Some(li) = self.get_selected_item() {
            let name = li.name().to_owned();
            Some(SelectedEntry { key_guid: li.guid, description: name, mode })
        } else {
            None
        }
    }

    pub(crate) fn basis_change(&mut self) {
        self.item_lists.lock().unwrap().clear_all();
        self.display_list.clear();
    }

    pub(crate) fn store_glyph_style(&mut self, style: GlyphStyle) {
        self.pddb
            .borrow()
            .delete_key(VAULT_CONFIG_DICT, VAULT_CONFIG_KEY_FONT, Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS))
            .ok();

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
                        name_to_style(&String::from_utf8(name_bytes).unwrap_or("regular".to_string()))
                            .unwrap_or(GlyphStyle::Regular)
                    }
                    Err(_) => GlyphStyle::Regular,
                }
            }
            _ => {
                log::warn!("PDDB access error reading default glyph size");
                GlyphStyle::Regular
            }
        };
        self.display_list.style(style);
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

                if self.totp_code.is_none() && self.display_list.len() > 0 {
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
                self.display_list.draw(TotpLayout::timer_box().br().y);

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
                    box_text.invert = true;
                    box_text.style = self.style;
                    write!(box_text, "{}", t!("vault.no_items", locales::LANG)).ok();
                    self.gfx.draw_textview(&mut box_text).expect("couldn't post empty notification");
                    self.gfx.flush().ok();
                    return;
                }

                // ---- draw the top "detail info" about the selected password ----
                let mut insert_at = 0;
                if let Some(entry) = self.get_selected_item() {
                    log::debug!("rendering entry {:?}", entry);
                    // draw more data about the selected item
                    let mut box_text = TextView::new(
                        Gid::dummy(),
                        TextBounds::CenteredTop(Rectangle::new(
                            Point::new(0, insert_at),
                            Point::new(screensize.x, insert_at + self.item_height * 3),
                        )),
                    );
                    box_text.draw_border = false;
                    box_text.clear_area = false;
                    box_text.ellipsis = true;
                    box_text.style = self.style;
                    box_text.invert = true;
                    // line 1
                    write!(box_text, "{} {}", &entry.name(), &entry.extra).ok();
                    self.gfx.draw_textview(&mut box_text).unwrap();
                    insert_at += box_text.bounds_computed.unwrap().height() as isize;
                } else {
                    // draw just the empty rectangle around the top area if nothing is selected
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
                self.display_list.draw(insert_at);
            }
        }
        self.gfx.flush().ok();
    }

    /// Returns `true` if in longpress state. Only call this once per key hit input.
    pub(crate) fn manage_longpress(&mut self) -> bool {
        let now = self.tt.elapsed_ms();
        if now - self.last_key_time > KEYUP_DELAY_MS {
            self.start_hold_time = now;
        }
        self.last_key_time = now;
        now - self.start_hold_time > FAST_SCROLL_DELAY_MS
    }

    pub(crate) fn nav(&mut self, dir: NavDir) {
        let mode_at_entry = (*self.mode.lock().unwrap()).clone();
        match mode_at_entry {
            VaultMode::Password => {
                let increment = if self.manage_longpress() { PAGE_INCREMENT } else { 1 };
                match dir {
                    NavDir::Up => {
                        for _ in 0..increment {
                            self.display_list.key_action('↑');
                        }
                    }
                    NavDir::Down => {
                        for _ in 0..increment {
                            self.display_list.key_action('↓');
                        }
                    }
                    NavDir::Autotype => {
                        if let Some(item) = self.get_selected_item() {
                            // print any errors within this function as a panic at this line
                            self.handle_autotype(item.guid, false).unwrap();
                        }
                    }
                    NavDir::Reserved => {
                        // tbd
                    }
                }
            }
            VaultMode::Totp => {
                match dir {
                    NavDir::Up => {
                        self.display_list.key_action('↑');
                    }
                    NavDir::Down => {
                        self.display_list.key_action('↓');
                    }
                    NavDir::Autotype => {
                        if let Some(code) = self.update_selected_totp_code() {
                            // ignore USB errors while sending code
                            self.usb_dev.send_str(&code).ok();
                        }
                    }
                    NavDir::Reserved => {
                        // tbd
                    }
                }
                self.totp_code = None;
            }
        }
    }

    pub(crate) fn filter(&mut self, criteria: &String) {
        self.item_lists.lock().unwrap().filter(self.mode.lock().unwrap().clone(), criteria);
    }

    pub(crate) fn handle_autotype(&mut self, guid: String, type_username: bool) -> Result<(), String> {
        // we re-fetch the entry for autotype, because the PDDB could have unmounted a basis.
        let atime = utc_now().timestamp() as u64;
        let pddb_binding = self.pddb.borrow();

        let mut record = pddb_binding
            .get(vault2::VAULT_PASSWORD_DICT, &guid, None, false, false, None, Some(vault2::basis_change))
            .map_err(|e| format!("couldn't access key {}: {:?}", guid, e))?;
        let mut data = Vec::<u8>::new();
        record.read_to_end(&mut data).map_err(|_| format!("Couldn't access key {}", guid))?;
        let mut pw = crate::storage::PasswordRecord::try_from(data)
            .map_err(|_| format!("Couldn't deserialize {}", guid))?;
        let to_type = if type_username { &pw.username } else { &pw.password };
        self.usb_dev.send_str(to_type).ok(); // ignore USB errors
        pw.count += 1;
        pw.atime = atime;

        // this get determines which basis the key is in
        let app_data = pddb_binding
            .get(vault2::VAULT_PASSWORD_DICT, &guid, None, true, true, Some(256), Some(vault2::basis_change))
            .map_err(|e| format!("error updating key atime: {:?}", e))?;
        let basis = app_data.attributes().map_err(|_| "couldn't get attributes")?.basis;

        // delete the old key
        pddb_binding
            .delete_key(vault2::VAULT_PASSWORD_DICT, &guid, Some(&basis))
            .map_err(|_| "Couldn't delete previous pw entry")?;

        // write the new key in
        let mut record = pddb_binding
            .get(
                vault2::VAULT_PASSWORD_DICT,
                &guid,
                Some(&basis),
                false,
                true,
                Some(vault2::VAULT_ALLOC_HINT),
                Some(vault2::basis_change),
            )
            .map_err(|e| format!("couldn't update key {}: {:?}", guid, e))?;
        let ser: Vec<u8> = crate::storage::PasswordRecord::into(pw);
        record.write(&ser).map_err(|e| format!("couldn't update key {}: {:?}", guid, e))?;

        self.pddb.borrow().sync().ok();
        Ok(())
    }
}
