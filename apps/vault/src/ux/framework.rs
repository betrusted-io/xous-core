use crate::*;
use crate::totp::{TotpAlgorithm, generate_totp_code};
use gam::{UxRegistration, GlyphStyle, MenuMatic, MenuItem, MenuPayload};
use graphics_server::{Gid, Point, Rectangle, DrawStyle, PixelColor, TextView};
use std::fmt::Write;
use pddb::Pddb;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::io::{Read, Write as FsWrite};
use actions::ActionOp;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::convert::TryFrom;

/// Display list for items. "name" is the key by which the list is sorted.
/// "extra" is more information about the item, which should not be part of the sort.
pub(crate) struct ListItem {
    pub(crate) name: String,
    pub(crate) extra: String,
    pub(crate) dirty: bool,
    /// this is the name of the key used to refer to the item
    pub(crate) guid: String,
}
impl ListItem {
    pub fn clone(&self) -> ListItem {
        ListItem { name: self.name.to_string(), extra: self.extra.to_string(), dirty: self.dirty, guid: self.guid.to_string() }
    }
}
impl Ord for ListItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}
impl PartialOrd for ListItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for ListItem {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for ListItem {}

pub(crate) enum NavDir {
    Up,
    Down,
    PageUp,
    PageDown,
}

#[allow(dead_code)]
pub(crate) struct VaultUx {
    /// the content area
    content: Gid,
    gam: gam::Gam,

    /// screensize of the content area
    screensize: Point,
    margin: Point, // margin to edge of canvas

    /// our security token for making changes to our record on the GAM
    token: [u32; 4],

    /// current operation mode
    mode: Arc::<Mutex::<VaultMode>>,
    title_dirty: bool,
    action_active: Arc::<AtomicBool>,

    /// list of all items to be displayed
    item_list: Arc::<Mutex::<Vec::<ListItem>>>,
    /// list of items displayable after filtering
    filtered_list: Vec::<ListItem>,
    /// the index into the item_list that is selected
    selection_index: usize,

    /// pddb handle
    pddb: RefCell::<Pddb>,

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
}

pub(crate) const DEFAULT_FONT: GlyphStyle = GlyphStyle::Regular;
pub(crate) const FONT_LIST: [&'static str; 6] = [
    "regular", "mono", "cjk",
    "bold", "large", "small"
];
pub(crate) fn name_to_style(name: &str) -> Option<GlyphStyle> {
    match name {
        "regular" => Some(GlyphStyle::Regular),
        "mono" => Some(GlyphStyle::Monospace),
        "cjk" => Some(GlyphStyle::Cjk),
        "bold" => Some(GlyphStyle::Bold),
        "large" => Some(GlyphStyle::Large),
        "small" => Some(GlyphStyle::Small),
        _ => None
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
        _ => "regular".to_string(),
    }
}

const TITLE_HEIGHT: i16 = 26;
const VAULT_CONFIG_DICT: &'static str = "vault.config";
const VAULT_CONFIG_KEY_FONT: &'static str = "fontstyle";

impl VaultUx {
    pub(crate) fn new(
        xns: &xous_names::XousNames,
        sid: xous::SID,
        menu_mgr: MenuMatic,
        actions_conn: xous::CID,
        mode: Arc::<Mutex::<VaultMode>>,
        item_list: Arc::<Mutex::<Vec::<ListItem>>>,
        action_active: Arc::<AtomicBool>,
    ) -> Self {
        let gam = gam::Gam::new(xns).expect("can't connect to GAM");

        let app_name_ref = gam::APP_NAME_VAULT;
        let token = gam.register_ux(UxRegistration {
            app_name: xous_ipc::String::<128>::from_str(app_name_ref),
            ux_type: gam::UxType::Chat,
            predictor: Some(xous_ipc::String::<64>::from_str(icontray::SERVER_NAME_ICONTRAY)),
            listener: sid.to_array(), // note disclosure of our SID to the GAM -- the secret is now shared with the GAM!
            redraw_id: VaultOp::Redraw.to_u32().unwrap(),
            gotinput_id: Some(VaultOp::Line.to_u32().unwrap()),
            audioframe_id: None,
            rawkeys_id: None,
            focuschange_id: Some(VaultOp::ChangeFocus.to_u32().unwrap()),
        }).expect("couldn't register Ux context for repl").unwrap();

        let content = gam.request_content_canvas(token).expect("couldn't get content canvas");
        let screensize = gam.get_canvas_bounds(content).expect("couldn't get dimensions of content canvas");
        gam.toggle_menu_mode(token).expect("couldnt't toggle menu mode");
        let margin = Point::new(4, 4);

        let pddb = pddb::Pddb::new();
        // TODO: put some informative message asking to mount the PDDB if it's not mounted, right now you just get a blank screen.
        // TODO: also add routines to detect if time is not set up, and block initialization until that happens.
        pddb.is_mounted_blocking();
        // temporary style setting, this will get over-ridden after init
        let style = GlyphStyle::Regular;
        let available_height = screensize.y - TITLE_HEIGHT;
        let glyph_height = gam.glyph_height_hint(style).unwrap();
        let item_height = (glyph_height * 2) as i16 + margin.y * 2 + 2; // +2 because of the border width
        let items_per_screen = available_height / item_height;

        VaultUx {
            content,
            gam,
            screensize,
            margin,
            token,
            mode,
            title_dirty: true,
            item_list,
            selection_index: 0,
            filtered_list: Vec::new(),
            pddb: RefCell::new(pddb),
            style,
            item_height,
            items_per_screen,
            menu_mgr,
            main_conn: xous::connect(sid).unwrap(),
            actions_conn,
            action_active,
            usb_dev: usb_device_xous::UsbHid::new(),
        }
    }

    pub(crate) fn update_mode(&mut self) {
        self.title_dirty = true;
        self.filtered_list.clear();
        self.selection_index = 0;
        self.filter("");
        self.swap_submenu();
    }

    /*
    Entry               Users
    -------------------------------------
    - autotype          pw
    - add new           pw  totp
    - edit              pw  totp    fido
    - delete            pw  totp    fido
    - change font       pw  totp    fido
    - close             pw  totp    fido
    */
    pub fn swap_submenu(&mut self) {
        // always call delete on the potential optional items, to return us to a known state
        self.menu_mgr.delete_item(t!("vault.menu_autotype", xous::LANG));
        self.menu_mgr.delete_item(t!("vault.menu_addnew", xous::LANG));
        match *self.mode.lock().unwrap() {
            VaultMode::Fido => (),
            VaultMode::Totp => {
                self.menu_mgr.insert_item(
                    MenuItem {
                        name: xous_ipc::String::from_str(t!("vault.menu_addnew", xous::LANG)),
                        action_conn: Some(self.actions_conn),
                        action_opcode: ActionOp::MenuAddnew.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    },
                    0
                );
            },
            VaultMode::Password => {
                self.menu_mgr.insert_item(
                    MenuItem {
                        name: xous_ipc::String::from_str(t!("vault.menu_addnew", xous::LANG)),
                        action_conn: Some(self.actions_conn),
                        action_opcode: ActionOp::MenuAddnew.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    },
                    0
                );
                self.menu_mgr.insert_item(
                    MenuItem {
                        name: xous_ipc::String::from_str(t!("vault.menu_autotype", xous::LANG)),
                        action_conn: Some(self.main_conn),
                        action_opcode: VaultOp::MenuAutotype.to_u32().unwrap(),
                        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                        close_on_select: true,
                    },
                    0
                );
            }
        }
    }

    pub(crate) fn get_glyph_style(&mut self) {
        let style = match self.pddb.borrow().get(
            VAULT_CONFIG_DICT,
            VAULT_CONFIG_KEY_FONT,
            None, true, true,
            Some(32), Some(crate::basis_change)
        ) {
            Ok(mut style_key) => {
                let mut name_bytes = Vec::<u8>::new();
                match style_key.read_to_end(&mut name_bytes) {
                    Ok(_len) => {
                        log::info!("name_bytes: {:?} {:?}", name_bytes, String::from_utf8(name_bytes.to_vec()));
                        name_to_style(&String::from_utf8(name_bytes).unwrap_or("regular".to_string()))
                            .unwrap_or(GlyphStyle::Regular)
                    },
                    Err(_) => GlyphStyle::Regular
                }
            }
            _ => {
                log::warn!("PDDB access error reading default glyph size");
                GlyphStyle::Regular
            },
        };
        // force redraw of all the items
        self.title_dirty = true;
        for item in self.filtered_list.iter_mut() {
            item.dirty = true;
        }
        self.style = style;
        let available_height = self.screensize.y - TITLE_HEIGHT;
        let glyph_height = self.gam.glyph_height_hint(self.style).unwrap();
        self.item_height = (glyph_height * 2) as i16 + self.margin.y * 2 + 2; // +2 because of the border width
        self.items_per_screen = available_height / self.item_height;
    }
    pub(crate) fn set_glyph_style(&mut self, style: GlyphStyle) {
        self.pddb.borrow().delete_key(VAULT_CONFIG_DICT, VAULT_CONFIG_KEY_FONT, None)
        .expect("couldn't delete previous setting");

        match self.pddb.borrow().get(
            VAULT_CONFIG_DICT,
            VAULT_CONFIG_KEY_FONT,
            None, true, true,
            Some(32), Some(crate::basis_change)
        ) {
            Ok(mut style_key) => {
                style_key.write(style_to_name(&style).as_bytes()).ok();
            }
            _ => panic!("PDDB access erorr"),
        };
        self.pddb.borrow().sync().ok();
        self.get_glyph_style();
    }
    fn mark_as_dirty(&mut self, index: usize) {
        let list_len = self.filtered_list.len();
        self.filtered_list[index.min(list_len - 1)].dirty = true;
    }
    fn mark_screen_as_dirty(&mut self, index: usize) {
        let page = index as i16 / self.items_per_screen;
        let list_len = self.filtered_list.len();
        for item in self.filtered_list[
            ((page as usize) * self.items_per_screen as usize).min(list_len) ..
            ((1 + page as usize) * self.items_per_screen as usize).min(list_len)
        ].iter_mut() {
            item.dirty = true;
        }
    }
    pub(crate) fn nav(&mut self, dir: NavDir) {
        match dir {
            NavDir::Up => {
                if self.selection_index > 0 {
                    self.mark_as_dirty(self.selection_index);
                    self.selection_index -= 1;
                    self.mark_as_dirty(self.selection_index);
                }
            }
            NavDir::Down => {
                if self.selection_index < self.filtered_list.len() {
                    self.mark_as_dirty(self.selection_index);
                    self.selection_index += 1;
                    self.mark_as_dirty(self.selection_index);
                }
            }
            NavDir::PageUp => {
                if self.selection_index > self.items_per_screen as usize {
                    self.mark_screen_as_dirty(self.selection_index);
                    self.selection_index -= self.items_per_screen as usize;
                    self.mark_screen_as_dirty(self.selection_index);
                } else {
                    self.mark_as_dirty(self.selection_index);
                    self.selection_index = 0;
                    self.mark_as_dirty(self.selection_index);
                }
            }
            NavDir::PageDown => {
                if self.selection_index < self.filtered_list.len() - self.items_per_screen as usize {
                    self.mark_screen_as_dirty(self.selection_index);
                    self.selection_index += self.items_per_screen as usize;
                    self.mark_screen_as_dirty(self.selection_index);
                } else {
                    self.mark_as_dirty(self.selection_index);
                    self.selection_index = self.filtered_list.len() - 1;
                    self.mark_as_dirty(self.selection_index);
                }
            }
        }
    }
    /// accept a new input string
    pub(crate) fn input(&mut self, line: &str) -> Result<(), xous::Error> {
        self.title_dirty = true;
        self.filter(line);
        Ok(())
    }

    fn clear_area(&mut self) {
        let items_height = self.items_per_screen * self.item_height;
        let mut insert_at = 1 + self.screensize.y - items_height; // +1 to get the border to overlap at the bottom

        if self.filtered_list.len() == 0 || self.action_active.load(AtomicOrdering::SeqCst) {
            // no items in list case -- just blank the whole area
            self.gam.draw_rectangle(self.content,
                Rectangle::new_with_style(
                    Point::new(0, 0),
                    self.screensize,
                DrawStyle {
                    fill_color: Some(PixelColor::Light),
                    stroke_color: None,
                    stroke_width: 0
                }
            )).expect("can't clear content area");
            return;
        } else if self.title_dirty && self.filtered_list.len() != 0 {
            // handle the title region separately
            self.gam.draw_rectangle(self.content,
                Rectangle::new_with_style(
                    Point::new(0, 0),
                    Point::new(self.screensize.x, insert_at - 1),
                DrawStyle {
                    fill_color: Some(PixelColor::Light),
                    stroke_color: None,
                    stroke_width: 0
                }
            )).expect("can't clear content area");
        }
        // iterate through every item to figure out the extent of the "dirty" area
        let mut dirty_tl: Option<Point> = None;
        let mut dirty_br: Option<Point> = None;

        let page = self.selection_index as i16 / self.items_per_screen;
        let list_len = self.filtered_list.len();
        for item in self.filtered_list[
            ((page as usize) * self.items_per_screen as usize).min(list_len) ..
            ((1 + page as usize) * self.items_per_screen as usize).min(list_len)
        ].iter() {
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
                    self.gam.draw_rectangle(self.content,
                        Rectangle::new_with_style(
                            tl,
                            br,
                        DrawStyle {
                            fill_color: Some(PixelColor::Light),
                            stroke_color: None,
                            stroke_width: 0
                        }
                    )).expect("can't clear content area");
                    // reset the search
                    dirty_tl = None;
                    dirty_br = None;
                }
            }
            insert_at += self.item_height;
        }
        if let Some(tl) = dirty_tl {
            // handle the case that we were dirty all the way to the bottom
            self.gam.draw_rectangle(self.content,
                Rectangle::new_with_style(
                    tl,
                    self.screensize,
                DrawStyle {
                    fill_color: Some(PixelColor::Light),
                    stroke_color: None,
                    stroke_width: 0
                }
            )).expect("can't clear content area");
        } else if dirty_tl.is_none() && dirty_br.is_none() {
            // this is the case that nothing was selected on the list, and there is "blank space" below
            // the list because the list is shorter than the total screen size. We clear this because the
            // space can be defaced eg. after a menu pops up.
            self.gam.draw_rectangle(self.content,
                Rectangle::new_with_style(
                    Point::new(0, insert_at + 1),
                    self.screensize,
                DrawStyle {
                    fill_color: Some(PixelColor::Light),
                    stroke_color: None,
                    stroke_width: 0
                }
            )).expect("can't clear content area");
        }
    }

    pub(crate) fn redraw(&mut self) -> Result<(), xous::Error> {
        self.clear_area();
        // ---- draw title area ----
        if self.title_dirty || self.action_active.load(AtomicOrdering::SeqCst) {
            let mut title_text = TextView::new(self.content,
                graphics_server::TextBounds::CenteredTop(
                    Rectangle::new(
                        Point::new(self.margin.x, 0),
                        Point::new(self.screensize.x - self.margin.x, TITLE_HEIGHT)
                    )
                )
            );
            title_text.draw_border = false;
            title_text.clear_area = true;
            title_text.style = GlyphStyle::Large;
            match *self.mode.lock().unwrap() {
                VaultMode::Fido => write!(title_text, "FIDO").ok(),
                VaultMode::Totp => write!(title_text, "⏳1234").ok(),
                VaultMode::Password => write!(title_text, "🔐****").ok(),
            };
            self.gam.post_textview(&mut title_text).expect("couldn't post title");
            self.title_dirty = false;
        }
        if self.action_active.load(AtomicOrdering::SeqCst) {
            // don't redraw the list in the back when menus are active above
            return Ok(())
        }

        // line up the list to justify to the bottom of the screen, based on the actual font requested
        let items_height = self.items_per_screen * self.item_height;
        let mut insert_at = 1 + self.screensize.y - items_height; // +1 to get the border to overlap at the bottom

        if self.filtered_list.len() == 0 {
            let mut box_text = TextView::new(self.content,
                graphics_server::TextBounds::CenteredBot(
                    Rectangle::new(
                        Point::new(0, insert_at),
                        Point::new(self.screensize.x, insert_at + self.item_height)
                    )
                )
            );
            box_text.draw_border = false;
            box_text.clear_area = true;
            box_text.style = self.style;
            box_text.margin = self.margin;
            write!(box_text, "{}", t!("vault.no_items", xous::LANG)).ok();
            self.gam.post_textview(&mut box_text).expect("couldn't post empty notification");
            return Ok(());
        }
        // ---- draw list body area ----
        let page = self.selection_index as i16 / self.items_per_screen;
        let selected = self.selection_index as i16 % self.items_per_screen;
        let list_len = self.filtered_list.len();
        for (index, item) in self.filtered_list[
            ((page as usize) * self.items_per_screen as usize).min(list_len) ..
            ((1 + page as usize) * self.items_per_screen as usize).min(list_len)
        ].iter_mut().enumerate() {
            if insert_at - 1 > self.screensize.y - self.item_height { // -1 because of the overlapping border
                break;
            }
            if item.dirty {
                let mut box_text = TextView::new(self.content,
                    graphics_server::TextBounds::BoundingBox(
                        Rectangle::new(
                            Point::new(0, insert_at),
                            Point::new(self.screensize.x, insert_at + self.item_height)
                        )
                    )
                );
                box_text.draw_border = true;
                box_text.rounded_border = None;
                box_text.clear_area = true;
                box_text.style = self.style;
                box_text.margin = self.margin;
                if index == selected as usize {
                    box_text.border_width = 4;
                }
                match *self.mode.lock().unwrap() {
                    VaultMode::Fido | VaultMode::Password => {write!(box_text, "{}\n{}", item.name, item.extra).ok();},
                    VaultMode::Totp => {
                        let fields = item.extra.split(':').collect::<Vec<&str>>();
                        if fields.len() == 4 {
                            let shared_secret = base32::decode(
                                base32::Alphabet::RFC4648 { padding: false }, fields[0])
                                .unwrap_or(vec![]);
                            let digit_count = u8::from_str_radix(fields[1], 10).unwrap_or(6);
                            let step_seconds = u16::from_str_radix(fields[2], 10).unwrap_or(30);
                            let algorithm = TotpAlgorithm::try_from(fields[2]).unwrap_or(TotpAlgorithm::HmacSha1);
                            let totp = totp::TotpEntry {
                                step_seconds,
                                shared_secret,
                                digit_count,
                                algorithm
                            };
                            let code = generate_totp_code(
                                totp::get_current_unix_time().unwrap_or(0),
                                &totp
                            ).unwrap_or(t!("vault.error.record_error", xous::LANG).to_string());
                            write!(box_text, "{}\n{}", item.name, code).ok();
                        } else {
                            write!(box_text, "{}", t!("vault.error.record_error", xous::LANG)).ok();
                        }
                    },
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
        log::info!("raised menu");
    }
    pub (crate) fn change_focus_to(&mut self, _state: &gam::FocusState) {
        self.title_dirty = true;
    }

    pub(crate) fn filter(&mut self, criteria: &str) {
        self.filtered_list.clear();
        for item in self.item_list.lock().unwrap().iter() {
            if item.name.starts_with(criteria) {
                let mut staged_item = item.clone();
                staged_item.dirty = true;
                self.filtered_list.push(staged_item);
            }
        }
        // the selection index must always be at a valid point
        if self.selection_index >= self.filtered_list.len() {
            if self.filtered_list.len() > 0 {
                self.selection_index = self.filtered_list.len() - 1;
            } else {
                self.selection_index = 0;
            }
        }
    }

    pub(crate) fn autotype(&mut self) -> Result<(), xous::Error> {
        if self.selection_index >= self.filtered_list.len() {
            return Err(xous::Error::InvalidPID);
        }
        let entry = &self.filtered_list[self.selection_index].guid;
        // we re-fetch the entry for autotype, because the PDDB could have unmounted a basis.
        let updated_pw = match self.pddb.borrow().get(
            crate::actions::VAULT_PASSWORD_DICT,
            entry,
            None,
            false, false, None,
            Some(crate::basis_change)
        ) {
            Ok(mut record) => {
                let mut data = Vec::<u8>::new();
                match record.read_to_end(&mut data) {
                    Ok(_len) => {
                        if let Some(mut pw) = crate::actions::deserialize_password(data) {
                            match self.usb_dev.send_str(&pw.password) {
                                Ok(_) => {
                                    pw.count += 1;
                                    pw.atime = utc_now().timestamp() as u64;
                                    pw
                                },
                                Err(e) => {
                                    log::error!("couldn't autotype password: {:?}", e);
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
                    },
                }
            }
            Err(e) => {
                log::error!("couldn't access key {}: {:?}", entry, e);
                return Err(xous::Error::ProcessNotFound);
            }
        };
        match self.pddb.borrow().delete_key(crate::actions::VAULT_PASSWORD_DICT, entry, None) {
            Ok(_) => {}
            Err(_e) => {
                return Err(xous::Error::InternalError);
            }
        }
        match self.pddb.borrow().get(
            crate::actions::VAULT_PASSWORD_DICT, entry, None,
            false, true, Some(crate::actions::VAULT_ALLOC_HINT),
            Some(crate::basis_change)
        ) {
            Ok(mut record) => {
                let ser = crate::actions::serialize_password(&updated_pw);
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
        Ok(())
    }
    pub(crate) fn selected_entry(&self) -> Option<SelectedEntry> {
        if self.selection_index >= self.filtered_list.len() {
            None
        } else {
            let entry = &self.filtered_list[self.selection_index];
            Some(
                SelectedEntry {
                    key_name: xous_ipc::String::from_str(entry.guid.to_string()),
                    mode: (*self.mode.lock().unwrap()).clone(),
                    description: xous_ipc::String::from_str(entry.name.to_string()),
                }
            )
        }
    }
    pub(crate) fn ensure_hid(&self) {
        self.usb_dev.ensure_core(usb_device_xous::UsbDeviceType::Hid).unwrap();
    }
}