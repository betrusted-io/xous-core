use std::cell::RefCell;
use std::convert::TryFrom;
use std::fmt::Write;
use std::io::{Read, Write as FsWrite};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::{Arc, Mutex};

use gam::{GlyphStyle, MenuItem, MenuMatic, MenuPayload};
use graphics_server::{DrawStyle, Gid, PixelColor, Point, Rectangle, TextView};
use locales::t;
use num_traits::*;
use pddb::Pddb;
use usb_device_xous::UsbDeviceType;
use vault::{VaultOp, utc_now};

use crate::actions::ActionOp;
use crate::totp::{TotpAlgorithm, TotpEntry, generate_totp_code, get_current_unix_time};
use crate::{ItemLists, SelectedEntry, VaultMode};

pub enum NavDir {
    Up,
    Down,
    PageUp,
    PageDown,
}

#[allow(dead_code)]
pub struct VaultUx {
    /// the content area
    content: Gid,
    gam: gam::Gam,

    /// screensize of the content area
    screensize: Point,
    margin: Point, // margin to edge of canvas

    /// our security token for making changes to our record on the GAM
    token: [u32; 4],

    /// current operation mode
    mode: Arc<Mutex<VaultMode>>,
    title_dirty: bool,
    action_active: Arc<AtomicBool>,

    /// list of all items to be displayed
    item_lists: Arc<Mutex<ItemLists>>,
    /// last filter query, so we can re-use it when mode is changed
    last_query: String,

    /// pddb handle
    pddb: RefCell<Pddb>,

    /// current font style
    style: GlyphStyle,
    item_height: i16,
    items_per_screen: i16,

    /// menu manager
    menu_mgr: MenuMatic,
    main_conn: xous::CID,
    actions_conn: xous::CID,

    /// usb interface
    usb_dev: usb_device_xous::UsbHid,
    usb_type: UsbDeviceType,

    /// totp redraw state
    last_epoch: u64,
    current_time: u64,
}

pub const DEFAULT_FONT: GlyphStyle = GlyphStyle::Regular;
pub const FONT_LIST: [&'static str; 7] = ["regular", "tall", "mono", "cjk", "bold", "large", "small"];
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

const TITLE_HEIGHT: i16 = 26;
const VAULT_CONFIG_DICT: &'static str = "vault.config";
const VAULT_CONFIG_KEY_FONT: &'static str = "fontstyle";

impl VaultUx {
    pub(crate) fn new(
        token: [u32; 4],
        xns: &xous_names::XousNames,
        sid: xous::SID,
        menu_mgr: MenuMatic,
        actions_conn: xous::CID,
        mode: Arc<Mutex<VaultMode>>,
        item_lists: Arc<Mutex<ItemLists>>,
        action_active: Arc<AtomicBool>,
    ) -> Self {
        let gam = gam::Gam::new(xns).expect("can't connect to GAM");

        let content = gam.request_content_canvas(token).expect("couldn't get content canvas");
        let screensize = gam.get_canvas_bounds(content).expect("couldn't get dimensions of content canvas");
        let margin = Point::new(4, 4);

        let pddb = pddb::Pddb::new();
        // temporary style setting, this will get over-ridden after init
        let style = GlyphStyle::Regular;
        let available_height = screensize.y - TITLE_HEIGHT;
        let glyph_height = gam.glyph_height_hint(style).unwrap();
        let item_height = (glyph_height * 2) as i16 + margin.y * 2 + 2; // +2 because of the border width
        let items_per_screen = available_height / item_height;
        item_lists.lock().unwrap().set_items_per_screen(items_per_screen);

        let current_time = get_current_unix_time().unwrap_or(0);

        VaultUx {
            content,
            gam,
            screensize,
            margin,
            token,
            mode,
            title_dirty: true,
            item_lists,
            pddb: RefCell::new(pddb),
            style,
            item_height,
            items_per_screen,
            menu_mgr,
            main_conn: xous::connect(sid).unwrap(),
            actions_conn,
            action_active,
            usb_dev: usb_device_xous::UsbHid::new(),
            last_epoch: current_time / 30,
            current_time,
            last_query: String::new(),
            usb_type: UsbDeviceType::FidoKbd,
        }
    }

    pub(crate) fn basis_change(&mut self) { self.item_lists.lock().unwrap().clear_all(); }

    pub(crate) fn update_mode(&mut self) {
        self.title_dirty = true;
        {
            let mut guarded_list = self.item_lists.lock().unwrap();
            guarded_list.clear_filter();
            let query = self.last_query.to_string();
            guarded_list.filter(self.mode.lock().unwrap().clone(), &query);
        }
        self.swap_submenu();
    }

    /*
    Entry               Users
    -------------------------------------
    - autotype          pw  totp
    - add new           pw  totp
    - edit              pw  totp    fido
    - delete            pw  totp    fido
    - change font       pw  totp    fido
    - unlock basis      pw  totp    fido
    - list/lock basis   pw  totp    fido
    - close             pw  totp    fido
    */
    pub fn swap_submenu(&mut self) {
        // always call delete on the potential optional items, to return us to a known state
        self.menu_mgr.delete_item(t!("vault.menu_autotype", locales::LANG));
        self.menu_mgr.delete_item(t!("vault.menu_autotype_username", locales::LANG));
        self.menu_mgr.delete_item(t!("vault.menu_addnew", locales::LANG));
        match *self.mode.lock().unwrap() {
            VaultMode::Fido => (),
            VaultMode::Password | VaultMode::Totp => {
                self.menu_mgr.insert_item(
                    MenuItem {
                        name: String::from(t!("vault.menu_addnew", locales::LANG)),
                        action_conn: Some(self.actions_conn),
                        action_opcode: ActionOp::MenuAddnew.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    },
                    0,
                );
                self.menu_mgr.insert_item(
                    MenuItem {
                        name: String::from(t!("vault.menu_autotype_username", locales::LANG)),
                        action_conn: Some(self.main_conn),
                        action_opcode: VaultOp::MenuAutotype.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar([1, 0, 0, 0]),
                        close_on_select: true,
                    },
                    0,
                );
                self.menu_mgr.insert_item(
                    MenuItem {
                        name: String::from(t!("vault.menu_autotype", locales::LANG)),
                        action_conn: Some(self.main_conn),
                        action_opcode: VaultOp::MenuAutotype.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    },
                    0,
                );
            }
        }
    }

    pub(crate) fn get_glyph_style(&mut self) {
        let style = match self.pddb.borrow().get(
            VAULT_CONFIG_DICT,
            VAULT_CONFIG_KEY_FONT,
            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS),
            true,
            true,
            Some(32),
            Some(vault::basis_change),
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
        if self.style != style {
            // force redraw of all the items
            self.title_dirty = true;
            self.item_lists.lock().unwrap().mark_all_dirty();
            self.style = style;
        }
        let available_height = self.screensize.y - TITLE_HEIGHT;
        let glyph_height = self.gam.glyph_height_hint(self.style).unwrap();
        self.item_height = (glyph_height * 2) as i16 + self.margin.y * 2 + 2; // +2 because of the border width
        self.items_per_screen = available_height / self.item_height;
        self.item_lists.lock().unwrap().set_items_per_screen(self.items_per_screen);
    }

    pub(crate) fn set_glyph_style(&mut self, style: GlyphStyle) {
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
            Some(vault::basis_change),
        ) {
            Ok(mut style_key) => {
                style_key.write(style_to_name(&style).as_bytes()).ok();
            }
            _ => panic!("PDDB access erorr"),
        };
        self.pddb.borrow().sync().ok();
        self.get_glyph_style();
    }

    pub(crate) fn nav(&mut self, dir: NavDir) {
        self.item_lists.lock().unwrap().nav((*self.mode.lock().unwrap()).clone(), dir);
    }

    /// accept a new input string
    pub(crate) fn input(&mut self, line: &str) -> Result<(), xous::Error> {
        self.title_dirty = true;
        let owned_line = line.to_owned();
        self.filter(&owned_line);
        self.last_query = owned_line;
        Ok(())
    }

    fn clear_area(&mut self) {
        let items_height = self.items_per_screen * self.item_height;
        let mut insert_at = 1 + self.screensize.y - items_height; // +1 to get the border to overlap at the bottom
        let mode_cache = (*self.mode.lock().unwrap()).clone();

        if self.item_lists.lock().unwrap().filter_len(mode_cache) == 0
            || self.action_active.load(AtomicOrdering::SeqCst)
        {
            // no items in list case -- just blank the whole area
            self.gam
                .draw_rectangle(
                    self.content,
                    Rectangle::new_with_style(
                        Point::new(0, 0),
                        self.screensize,
                        DrawStyle {
                            fill_color: Some(PixelColor::Light),
                            stroke_color: None,
                            stroke_width: 0,
                        },
                    ),
                )
                .expect("can't clear content area");
            self.title_dirty = true; // just blanked the whole area, have to redraw the title.
            return;
        } else if self.title_dirty && self.item_lists.lock().unwrap().filter_len(mode_cache) != 0 {
            // handle the title region separately
            self.gam
                .draw_rectangle(
                    self.content,
                    Rectangle::new_with_style(
                        Point::new(0, 0),
                        Point::new(self.screensize.x, insert_at - 1),
                        DrawStyle {
                            fill_color: Some(PixelColor::Light),
                            stroke_color: None,
                            stroke_width: 0,
                        },
                    ),
                )
                .expect("can't clear content area");
        }
        // iterate through every item to figure out the extent of the "dirty" area
        let mut dirty_tl: Option<Point> = None;
        let mut dirty_br: Option<Point> = None;

        let mut guarded_list = self.item_lists.lock().unwrap();
        let current_page = guarded_list.selected_page(mode_cache);
        for item in current_page.iter() {
            if item.dirty && dirty_tl.is_none() {
                // start the dirty area
                dirty_tl = Some(Point::new(0, insert_at));
            }
            if !item.dirty && dirty_tl.is_some() && dirty_br.is_none() {
                // end the dirty area
                dirty_br = Some(Point::new(self.screensize.y, insert_at));
            }
            if let Some(tl) = dirty_tl {
                if let Some(br) = dirty_br {
                    // start & end found: now draw a rectangle over it
                    self.gam
                        .draw_rectangle(
                            self.content,
                            Rectangle::new_with_style(
                                tl,
                                br,
                                DrawStyle {
                                    fill_color: Some(PixelColor::Light),
                                    stroke_color: None,
                                    stroke_width: 0,
                                },
                            ),
                        )
                        .expect("can't clear content area");
                    // reset the search
                    dirty_tl = None;
                    dirty_br = None;
                }
            }
            insert_at += self.item_height;
        }
        if let Some(tl) = dirty_tl {
            // handle the case that we were dirty all the way to the bottom
            self.gam
                .draw_rectangle(
                    self.content,
                    Rectangle::new_with_style(
                        tl,
                        self.screensize,
                        DrawStyle {
                            fill_color: Some(PixelColor::Light),
                            stroke_color: None,
                            stroke_width: 0,
                        },
                    ),
                )
                .expect("can't clear content area");
        } else if dirty_tl.is_none() && dirty_br.is_none() {
            // this is the case that nothing was selected on the list, and there is "blank space" below
            // the list because the list is shorter than the total screen size. We clear this because the
            // space can be defaced eg. after a menu pops up.
            self.gam
                .draw_rectangle(
                    self.content,
                    Rectangle::new_with_style(
                        Point::new(0, insert_at + 1),
                        self.screensize,
                        DrawStyle {
                            fill_color: Some(PixelColor::Light),
                            stroke_color: None,
                            stroke_width: 0,
                        },
                    ),
                )
                .expect("can't clear content area");
        }
    }

    pub(crate) fn redraw(&mut self) -> Result<(), xous::Error> {
        // to reduce locking thrash, we cache a copy of the current mode at the top of redraw.
        // this could lead to some race conditions that lead to awkward problems, we'll see, but it
        // is probably worth the performance improvement.
        let mode_at_entry = (*self.mode.lock().unwrap()).clone();

        if mode_at_entry == VaultMode::Totp {
            // always redraw the title in TOTP mode
            self.title_dirty = true;
            // always grab the current time, regardless of the mode? saves an extra call to get time...
            self.current_time = get_current_unix_time().unwrap_or(0);
            // duration bar is hard-coded to once every 30 seconds, even if the keys may not change that
            // often.
            let epoch = self.current_time / 30;
            if self.last_epoch != epoch {
                self.last_epoch = epoch;
                // force a redraw of all the items if the epoch has changed
                self.item_lists.lock().unwrap().mark_all_dirty();
            }
        }
        self.clear_area();

        // ---- draw title area ----
        if self.title_dirty || self.action_active.load(AtomicOrdering::SeqCst) {
            let mut title_text = TextView::new(
                self.content,
                graphics_server::TextBounds::CenteredTop(Rectangle::new(
                    Point::new(self.margin.x, 0),
                    Point::new(self.screensize.x - self.margin.x, TITLE_HEIGHT),
                )),
            );
            title_text.draw_border = false;
            title_text.clear_area = true;
            title_text.style = GlyphStyle::Large;
            match mode_at_entry {
                VaultMode::Fido => write!(title_text, "FIDO").ok(),
                VaultMode::Totp => write!(title_text, "⏳1234").ok(),
                VaultMode::Password => write!(title_text, "🔐****").ok(),
            };
            self.gam.post_textview(&mut title_text).expect("couldn't post title");
            if mode_at_entry == VaultMode::Totp {
                const BAR_HEIGHT: i16 = 5;
                const BAR_GAP: i16 = -10;
                // draw the duration bar
                let delta = (self.current_time - (self.last_epoch * 30)) as i32;
                let width = (self.screensize.x - (self.margin.x * 2)) as i32;
                let delta_width = (delta * width * 100) / (30 * 100);
                self.gam
                    .draw_rectangle(
                        self.content,
                        Rectangle {
                            tl: Point::new(self.margin.x, TITLE_HEIGHT - (BAR_HEIGHT + BAR_GAP)),
                            br: Point::new(
                                self.screensize.x - self.margin.x - delta_width as i16,
                                TITLE_HEIGHT - BAR_GAP,
                            ),
                            style: DrawStyle {
                                fill_color: Some(PixelColor::Dark),
                                stroke_color: None,
                                stroke_width: 0,
                            },
                        },
                    )
                    .ok();
            }
            self.title_dirty = false;
        }
        if self.action_active.load(AtomicOrdering::SeqCst) {
            // don't redraw the list in the back when menus are active above
            return Ok(());
        }

        // line up the list to justify to the bottom of the screen, based on the actual font requested
        let items_height = self.items_per_screen * self.item_height;
        let mut insert_at = 1 + self.screensize.y - items_height; // +1 to get the border to overlap at the bottom

        if self.item_lists.lock().unwrap().filter_len(mode_at_entry) == 0 {
            let mut box_text = TextView::new(
                self.content,
                graphics_server::TextBounds::CenteredBot(Rectangle::new(
                    Point::new(0, insert_at),
                    Point::new(self.screensize.x, insert_at + self.item_height),
                )),
            );
            box_text.draw_border = false;
            box_text.clear_area = true;
            box_text.style = self.style;
            box_text.margin = self.margin;
            write!(box_text, "{}", t!("vault.no_items", locales::LANG)).ok();
            self.gam.post_textview(&mut box_text).expect("couldn't post empty notification");
            return Ok(());
        }
        // ---- draw list body area ----
        let selected = self.item_lists.lock().unwrap().selected_index(mode_at_entry);
        let mut guarded_list = self.item_lists.lock().unwrap();
        let current_page = guarded_list.selected_page(mode_at_entry);
        log::debug!("current_page len {}", current_page.len());
        for (index, item) in current_page.iter_mut().enumerate() {
            if insert_at - 1 > self.screensize.y - self.item_height {
                // -1 because of the overlapping border
                break;
            }
            if item.dirty {
                log::debug!("drawing {}", item.name());
                let mut box_text = TextView::new(
                    self.content,
                    graphics_server::TextBounds::BoundingBox(Rectangle::new(
                        Point::new(0, insert_at),
                        Point::new(self.screensize.x, insert_at + self.item_height),
                    )),
                );
                box_text.draw_border = true;
                box_text.rounded_border = None;
                box_text.clear_area = true;
                box_text.style = self.style;
                box_text.margin = self.margin;
                if index == selected {
                    box_text.border_width = 4;
                }
                match mode_at_entry {
                    VaultMode::Fido | VaultMode::Password => {
                        write!(box_text, "{}\n{}", item.name(), item.extra).ok();
                    }
                    VaultMode::Totp => {
                        let fields = item.extra.split(':').collect::<Vec<&str>>();
                        if fields.len() == 5 {
                            let shared_secret =
                                base32::decode(base32::Alphabet::RFC4648 { padding: false }, fields[0])
                                    .unwrap_or(vec![]);
                            let digit_count = u8::from_str_radix(fields[1], 10).unwrap_or(6);
                            let step_seconds = u64::from_str_radix(fields[2], 10).unwrap_or(30);
                            let algorithm =
                                TotpAlgorithm::try_from(fields[3]).unwrap_or(TotpAlgorithm::HmacSha1);
                            let is_hotp = fields[4].to_uppercase() == "HOTP";
                            let totp = TotpEntry {
                                step_seconds: if !is_hotp { step_seconds } else { 1 }, /* step_seconds is
                                                                                        * re-used by hotp
                                                                                        * as the code. */
                                shared_secret,
                                digit_count,
                                algorithm,
                            };
                            if !is_hotp {
                                let code = generate_totp_code(get_current_unix_time().unwrap_or(0), &totp)
                                    .unwrap_or(t!("vault.error.record_error", locales::LANG).to_string());
                                // why code on top? because the item.name can be very long, and it can wrap
                                // which would cause the code to become
                                // hidden.
                                write!(box_text, "{}\n{}", code, item.name()).ok();
                            } else {
                                let code = generate_totp_code(step_seconds, &totp)
                                    .unwrap_or(t!("vault.error.record_error", locales::LANG).to_string());
                                // why code on top? because the item.name can be very long, and it can wrap
                                // which would cause the code to become
                                // hidden.
                                write!(box_text, "HOTP {}\n{}", code, item.name()).ok();
                            }
                        } else {
                            write!(box_text, "{}", t!("vault.error.record_error", locales::LANG)).ok();
                        }
                    }
                }
                self.gam.post_textview(&mut box_text).expect("couldn't post list item");
                item.dirty = false;
            }

            insert_at += self.item_height;
        }

        log::trace!("vault app redraw##");
        self.gam.redraw().expect("couldn't redraw screen");
        Ok(())
    }

    pub(crate) fn raise_menu(&mut self) {
        self.title_dirty = true;
        self.gam.raise_menu(gam::APP_MENU_0_VAULT).expect("couldn't raise our submenu");
        log::debug!("raised menu");
    }

    pub(crate) fn change_focus_to(&mut self, _state: &gam::FocusState) { self.title_dirty = true; }

    pub(crate) fn filter(&mut self, criteria: &String) {
        self.item_lists.lock().unwrap().filter(self.mode.lock().unwrap().clone(), criteria);
    }

    pub(crate) fn set_autotype_delay_ms(&self, rate: usize) { self.usb_dev.set_autotype_delay_ms(rate); }

    pub(crate) fn autotype(&mut self, type_username: bool) -> Result<(), xous::Error> {
        let mode_cache = (*self.mode.lock().unwrap()).clone();
        match mode_cache {
            VaultMode::Password => {
                let entry = self.item_lists.lock().unwrap().selected_guid(mode_cache);
                // we re-fetch the entry for autotype, because the PDDB could have unmounted a basis.
                let atime = utc_now().timestamp() as u64;
                let updated_pw = match self.pddb.borrow().get(
                    vault::VAULT_PASSWORD_DICT,
                    &entry,
                    None,
                    false,
                    false,
                    None,
                    Some(vault::basis_change),
                ) {
                    Ok(mut record) => {
                        let mut data = Vec::<u8>::new();
                        match record.read_to_end(&mut data) {
                            Ok(_len) => {
                                if let Some(mut pw) = crate::storage::PasswordRecord::try_from(data).ok() {
                                    let to_type = if type_username { &pw.username } else { &pw.password };
                                    match self.usb_dev.send_str(to_type) {
                                        Ok(_) => {
                                            pw.count += 1;
                                            pw.atime = atime;
                                            pw
                                        }
                                        Err(e) => {
                                            log::error!("couldn't autotype: {:?}", e);
                                            return Err(xous::Error::UseBeforeInit);
                                        }
                                    }
                                } else {
                                    log::error!("couldn't deserialize {}", entry);
                                    return Err(xous::Error::InvalidString);
                                }
                            }
                            Err(e) => {
                                log::error!("couldn't access key {}: {:?}", entry, e);
                                return Err(xous::Error::ProcessNotFound);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("couldn't access key {}: {:?}", entry, e);
                        return Err(xous::Error::ProcessNotFound);
                    }
                };

                // this get determines which basis the key is in
                let basis = match self.pddb.borrow().get(
                    vault::VAULT_PASSWORD_DICT,
                    &entry,
                    None,
                    true,
                    true,
                    Some(256),
                    Some(vault::basis_change),
                ) {
                    Ok(app_data) => {
                        let attr = app_data.attributes().expect("couldn't get attributes");
                        attr.basis
                    }
                    Err(e) => {
                        log::error!("error updating key atime: {:?}", e);
                        return Err(xous::Error::InternalError);
                    }
                };

                match self.pddb.borrow().delete_key(vault::VAULT_PASSWORD_DICT, &entry, Some(&basis)) {
                    Ok(_) => {}
                    Err(_e) => {
                        return Err(xous::Error::InternalError);
                    }
                }
                match self.pddb.borrow().get(
                    vault::VAULT_PASSWORD_DICT,
                    &entry,
                    Some(&basis),
                    false,
                    true,
                    Some(vault::VAULT_ALLOC_HINT),
                    Some(vault::basis_change),
                ) {
                    Ok(mut record) => {
                        let ser: Vec<u8> = crate::storage::PasswordRecord::into(updated_pw);
                        match record.write(&ser) {
                            Ok(_) => {}
                            Err(e) => {
                                log::error!("couldn't update key {}: {:?}", entry, e);
                                return Err(xous::Error::OutOfMemory);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("couldn't update key {}: {:?}", entry, e);
                        return Err(xous::Error::OutOfMemory);
                    }
                }
                self.pddb.borrow().sync().ok();
                // force a redraw of the record as the access count updated
                self.item_lists.lock().unwrap().selected_update_atime(mode_cache, atime);
            }
            VaultMode::Totp => {
                let extra = self.item_lists.lock().unwrap().selected_extra(mode_cache);
                let fields = extra.split(':').collect::<Vec<&str>>();
                if fields.len() == 5 {
                    let shared_secret =
                        base32::decode(base32::Alphabet::RFC4648 { padding: false }, fields[0])
                            .unwrap_or(vec![]);
                    let digit_count = u8::from_str_radix(fields[1], 10).unwrap_or(6);
                    let step_seconds = u64::from_str_radix(fields[2], 10).unwrap_or(30);
                    let algorithm = TotpAlgorithm::try_from(fields[3]).unwrap_or(TotpAlgorithm::HmacSha1);
                    let is_hotp = fields[4].to_uppercase() == "HOTP";
                    let totp = TotpEntry {
                        step_seconds: if !is_hotp { step_seconds } else { 1 }, /* step_seconds is re-used
                                                                                * by hotp as the code. */
                        shared_secret,
                        digit_count,
                        algorithm,
                    };
                    let code = if !is_hotp {
                        generate_totp_code(get_current_unix_time().unwrap_or(0), &totp)
                            .unwrap_or(t!("vault.error.record_error", locales::LANG).to_string())
                    } else {
                        generate_totp_code(step_seconds, &totp)
                            .unwrap_or(t!("vault.error.record_error", locales::LANG).to_string())
                    };
                    match self.usb_dev.send_str(&code) {
                        Ok(_) => {
                            if is_hotp {
                                // update the count once the HOTP has been typed successfully
                                let entry = self.item_lists.lock().unwrap().selected_guid(mode_cache);

                                // this get determines which basis the key is in
                                let (basis, hotp_rec) = match self.pddb.borrow().get(
                                    vault::VAULT_TOTP_DICT,
                                    &entry,
                                    None,
                                    true,
                                    true,
                                    Some(256),
                                    Some(vault::basis_change),
                                ) {
                                    Ok(mut app_data) => {
                                        let attr = app_data.attributes().expect("couldn't get attributes");
                                        let len = app_data.attributes().unwrap().len;
                                        let mut data = Vec::<u8>::with_capacity(len);
                                        data.resize(len, 0);
                                        match app_data.read_exact(&mut data) {
                                            Ok(_len) => {
                                                if let Some(mut totp_rec) =
                                                    crate::storage::TotpRecord::try_from(data).ok()
                                                {
                                                    totp_rec.timestep += 1;
                                                    (attr.basis, totp_rec)
                                                } else {
                                                    log::error!("Couldn't deserialize HOTP: {:?}", entry);
                                                    return Err(xous::Error::InternalError);
                                                }
                                            }
                                            Err(e) => {
                                                log::error!("Couldn't access HOTP key: {:?}", e);
                                                return Err(xous::Error::InternalError);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("error updating HOTP count: {:?}", e);
                                        return Err(xous::Error::InternalError);
                                    }
                                };
                                // remove the old entry, specifically only in the most recently open basis.
                                match self.pddb.borrow().delete_key(
                                    vault::VAULT_TOTP_DICT,
                                    &entry,
                                    Some(&basis),
                                ) {
                                    Ok(_) => {}
                                    Err(_e) => {
                                        return Err(xous::Error::InternalError);
                                    }
                                }
                                // update the "extra" field, because the timestep field has been altered
                                self.item_lists.lock().unwrap().selected_update_extra(
                                    mode_cache,
                                    format!(
                                        "{}:{}:{}:{}:{}",
                                        hotp_rec.secret,
                                        hotp_rec.digits,
                                        hotp_rec.timestep,
                                        hotp_rec.algorithm,
                                        if hotp_rec.is_hotp { "HOTP" } else { "TOTP" }
                                    ),
                                );
                                // now write to disk
                                match self.pddb.borrow().get(
                                    vault::VAULT_TOTP_DICT,
                                    &entry,
                                    Some(&basis),
                                    false,
                                    true,
                                    Some(vault::VAULT_ALLOC_HINT),
                                    Some(vault::basis_change),
                                ) {
                                    Ok(mut record) => {
                                        let ser: Vec<u8> = crate::storage::TotpRecord::into(hotp_rec);
                                        match record.write(&ser) {
                                            Ok(_) => {}
                                            Err(e) => {
                                                log::error!("couldn't update key {}: {:?}", entry, e);
                                                return Err(xous::Error::OutOfMemory);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("couldn't update key {}: {:?}", entry, e);
                                        return Err(xous::Error::OutOfMemory);
                                    }
                                }
                                self.pddb.borrow().sync().ok();
                            }
                        }
                        _ => (),
                    };
                }
            }
            _ => log::error!("Illegal state! we shouldn't be having an autotype on {:?}", mode_cache),
        }
        Ok(())
    }

    pub(crate) fn selected_entry(&self) -> Option<SelectedEntry> {
        let mode = (*self.mode.lock().unwrap()).clone();
        self.item_lists.lock().unwrap().selected_entry(mode)
    }

    pub(crate) fn ensure_hid(&self) {
        self.usb_dev.ensure_core(self.usb_type).unwrap();
        self.usb_dev.restrict_debug_access(true).unwrap();
    }

    /// In readout mode, the keyboard composite function has to be turned off, because
    /// Windows will block any programs that try to talk to USB devices that contain a keyboard
    /// (ostensibly to prevent keyboard loggers).
    pub(crate) fn readout_mode(&mut self, enabled: bool) {
        if enabled {
            self.usb_type = UsbDeviceType::Fido;
        } else {
            self.usb_type = UsbDeviceType::FidoKbd;
        }
        self.usb_dev.ensure_core(self.usb_type).unwrap();
    }
}
