//! The main API entry point is the `Menu` struct. Click into the struct for more details.

pub use graphics_server::api::{DrawStyle, PixelColor, TextBounds};
pub use graphics_server::*;
use num_traits::*;
#[cfg(feature = "tts")]
use tts_frontend::TtsFrontend;
use xous_ipc::{Buffer, String};

use crate::api::*;
use crate::Gam;
use crate::{forwarding_thread, MsgForwarder};
#[derive(Debug)]
pub struct Menu<'a> {
    pub sid: xous::SID,
    pub gam: Gam,
    pub xns: xous_names::XousNames,
    pub items: Vec<MenuItem>,
    pub index: usize, // currently selected item
    pub canvas: Gid,
    pub authtoken: [u32; 4],
    pub margin: i16,
    pub divider_margin: i16,
    pub line_height: i16,
    pub canvas_width: Option<i16>,
    pub helper_data: Option<Buffer<'a>>,
    pub name: std::string::String,
    #[cfg(feature = "tts")]
    pub tts: TtsFrontend,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum MenuOpcode {
    // note: this should match ModalOpcode, for compatibility with the generic helper function
    Redraw = 0x4000_0000, /* set the high bit so that "standard" enums don't conflict with the
                           * Modal-specific opcodes */
    Rawkeys,
    Quit,
}

impl<'a> Menu<'a> {
    pub fn new(name: &str) -> Menu {
        let xns = xous_names::XousNames::new().unwrap();
        let sid = xous::create_server().expect("can't create private menu message server");
        let gam = Gam::new(&xns).expect("can't connect to GAM");
        let authtoken = gam
            .register_ux(UxRegistration {
                app_name: String::<128>::from_str(name),
                ux_type: UxType::Menu,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: MenuOpcode::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                focuschange_id: None, // should always be None because we're not an app
                rawkeys_id: Some(MenuOpcode::Rawkeys.to_u32().unwrap()),
            })
            .expect("couldn't register my Ux element with GAM");
        assert!(
            authtoken.is_some(),
            "Couldn't register menu. Did you remember to add the app_name to the tokens.rs expected boot contexts list?"
        );
        log::debug!("requesting content canvas for menu");
        let canvas =
            gam.request_content_canvas(authtoken.unwrap()).expect("couldn't get my content canvas from GAM");
        let line_height =
            gam.glyph_height_hint(crate::SYSTEM_STYLE).expect("couldn't get glyph height hint") as i16 + 2;
        #[cfg(feature = "tts")]
        let tts = TtsFrontend::new(&xns).unwrap();
        Menu {
            sid,
            gam,
            xns,
            items: Vec::new(),
            index: 0,
            canvas,
            authtoken: authtoken.unwrap(),
            margin: 8,
            divider_margin: 20,
            line_height,
            canvas_width: None,
            helper_data: None,
            name: std::string::String::from(name),
            #[cfg(feature = "tts")]
            tts,
        }
    }

    pub fn activate(&self) { self.gam.raise_menu(self.name.as_str()).expect("couldn't raise menu"); }

    pub fn set_index(&mut self, index: usize) { self.index = index; }

    /// this function spawns a client-side thread to forward redraw and key event
    /// messages on to a local server. The goal is to keep the local server's SID
    /// a secret. The GAM only knows the single-use SID for redraw commands; this
    /// isolates a server's private command set from the GAM.
    pub fn spawn_helper(
        &mut self,
        private_sid: xous::SID,
        public_sid: xous::SID,
        redraw_op: u32,
        rawkeys_op: u32,
        drop_op: u32,
    ) {
        let helper_data = MsgForwarder {
            private_sid: private_sid.to_array(),
            public_sid: public_sid.to_array(),
            redraw_op,
            rawkeys_op,
            drop_op,
        };
        let buf = Buffer::into_buf(helper_data).expect("couldn't allocate helper data for helper thread");
        let (addr, size, offset) = unsafe { buf.to_raw_parts() };
        self.helper_data = Some(buf);
        xous::create_thread_3(forwarding_thread, addr, size, offset).expect("couldn't spawn a helper thread");
    }

    /// Appends a menu item to the end of the current Menu
    pub fn add_item(&mut self, new_item: MenuItem) {
        if new_item.name.as_str().unwrap() == "🔇" {
            // suppress the addition of menu items that are not applicable for a given locale
            return;
        }
        // first, do the insertion.
        // add the menu item to the first free slot
        // any modifications to the menu structure should guarantee that the list is compacted
        // and has no holes, in order for the "selected index" logic to work
        self.items.push(new_item);

        // now, recompute the height
        let mut total_items = self.num_items();
        if total_items == 0 {
            total_items = 1; // just so we see a blank menu at least, and have a clue how to debug
        }
        let current_bounds = self.gam.get_canvas_bounds(self.canvas).expect("couldn't get current bounds");
        let mut new_bounds = SetCanvasBoundsRequest {
            requested: Point::new(current_bounds.x, total_items as i16 * self.line_height + self.margin * 2),
            granted: None,
            token_type: TokenType::App,
            token: self.authtoken,
        };
        log::debug!("add_item requesting bounds of {:?}", new_bounds);
        self.gam.set_canvas_bounds_request(&mut new_bounds).expect("couldn't call set bounds");
    }

    // Attempts to insert a MenuItem at the index given. This displaces the item at that index down
    // by one slot. Returns false if the index is invalid.
    pub fn insert_item(&mut self, new_item: MenuItem, at: usize) -> bool {
        if new_item.name.as_str().unwrap() == "🔇" {
            // suppress the addition of menu items that are not applicable for a given locale
            return false;
        }
        // first, do the insertion.
        // add the menu item to the first free slot
        // any modifications to the menu structure should guarantee that the list is compacted
        // and has no holes, in order for the "selected index" logic to work
        if at <= self.items.len() {
            self.items.insert(at, new_item);
            // now, recompute the height
            let mut total_items = self.num_items();
            if total_items == 0 {
                total_items = 1; // just so we see a blank menu at least, and have a clue how to debug
            }
            let current_bounds =
                self.gam.get_canvas_bounds(self.canvas).expect("couldn't get current bounds");
            let mut new_bounds = SetCanvasBoundsRequest {
                requested: Point::new(
                    current_bounds.x,
                    total_items as i16 * self.line_height + self.margin * 2,
                ),
                granted: None,
                token_type: TokenType::App,
                token: self.authtoken,
            };
            log::debug!("add_item requesting bounds of {:?}", new_bounds);
            self.gam.set_canvas_bounds_request(&mut new_bounds).expect("couldn't call set bounds");
            true
        } else {
            false
        }
    }

    // note: this routine has yet to be tested. (remove this comment once it has been actually used by
    // something)
    pub fn delete_item(&mut self, item: &str) -> bool {
        let len_before = self.items.len();
        self.items.retain(|&candidate| candidate.name.as_str().unwrap() != item);

        // now, recompute the height
        let mut total_items = self.num_items();
        if total_items == 0 {
            total_items = 1; // just so we see a blank menu at least, and have a clue how to debug
        }
        let current_bounds = self.gam.get_canvas_bounds(self.canvas).expect("couldn't get current bounds");
        let mut new_bounds = SetCanvasBoundsRequest {
            requested: Point::new(current_bounds.x, total_items as i16 * self.line_height + self.margin * 2),
            granted: None,
            token_type: TokenType::App,
            token: self.authtoken,
        };
        log::debug!("add_item requesting bounds of {:?}", new_bounds);
        self.gam.set_canvas_bounds_request(&mut new_bounds).expect("couldn't call set bounds");

        if len_before > self.items.len() { true } else { false }
    }

    pub fn draw_item(&self, index: i16, with_marker: bool) {
        use core::fmt::Write;
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();

        let item = self.items[index as usize];
        let mut item_tv = TextView::new(
            self.canvas,
            TextBounds::BoundingBox(Rectangle::new(
                Point::new(self.margin, index * self.line_height + self.margin),
                Point::new(canvas_size.x - self.margin, (index + 1) * self.line_height + self.margin),
            )),
        );

        if with_marker {
            write!(item_tv.text, "\u{25B6}").unwrap();
            #[cfg(feature = "tts")]
            self.tts.tts_simple(item.name.as_str().unwrap()).unwrap();
        } else {
            write!(item_tv.text, "\t").unwrap();
        }
        write!(item_tv.text, "{}", item.name.as_str().unwrap()).unwrap();
        item_tv.draw_border = false;
        item_tv.style = crate::SYSTEM_STYLE;
        item_tv.margin = Point::new(0, 0);
        item_tv.ellipsis = true;

        self.gam.post_textview(&mut item_tv).expect("couldn't render menu list item");
    }

    // draw a dividing line above the indexed item
    pub fn draw_divider(&self, index: i16) {
        if false {
            // aesthetically, we don't need this
            if let Some(canvas_width) = self.canvas_width {
                self.gam
                    .draw_line(
                        self.canvas,
                        Line::new_with_style(
                            Point::new(self.divider_margin, index * self.line_height + self.margin / 2),
                            Point::new(
                                canvas_width - self.divider_margin,
                                index * self.line_height + self.margin / 2,
                            ),
                            DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1),
                        ),
                    )
                    .expect("couldn't draw dividing line")
            } else {
                log::debug!(
                    "cant draw divider because our canvas width was not initialized. Ignoring request."
                );
            }
        }
    }

    pub fn prev_item(&mut self) {
        if self.index > 0 {
            // wipe out the current marker
            self.draw_item(self.index as i16, false);
            self.index -= 1;
            // add the marker to the pervious item
            self.draw_item(self.index as i16, true);

            if self.index != 0 {
                self.draw_divider(self.index as i16);
            }
            if self.index < self.num_items() - 1 {
                self.draw_divider(self.index as i16 + 1);
            }
            if self.index < self.num_items() - 2 {
                self.draw_divider(self.index as i16 + 2);
            }
        } else if self.index == 0 {
            // wipe out the current marker
            self.draw_item(self.index as i16, false);
            self.index = self.num_items() - 1;
            // add the marker to the last item
            self.draw_item(self.index as i16, true);

            // NOTE: if we bring back the dividers, we will need to add them to this edge case here as well.
        }
    }

    pub fn next_item(&mut self) {
        if self.index < (self.num_items() - 1) {
            // wipe out the current marker
            self.draw_item(self.index as i16, false);
            self.index += 1;
            // add the marker to the pervious item
            self.draw_item(self.index as i16, true);

            if self.index != 1 {
                self.draw_divider(self.index as i16 - 1);
            }
            self.draw_divider(self.index as i16);
            if self.index < self.num_items() - 1 {
                self.draw_divider(self.index as i16 + 1);
            }
        } else if self.index == (self.num_items() - 1) {
            // wipe out the current marker
            self.draw_item(self.index as i16, false);
            self.index = 0;
            // add the marker to the first item
            self.draw_item(self.index as i16, true);

            // NOTE: if we bring back the dividers, we will need to add them to this edge case here as well.
        }
    }

    pub fn redraw(&mut self) {
        // for now, just draw a black rectangle
        log::trace!("menu redraw");
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();
        self.canvas_width = Some(canvas_size.x);

        // draw the outer border
        self.gam
            .draw_rounded_rectangle(
                self.canvas,
                RoundedRectangle::new(
                    Rectangle::new_with_style(
                        Point::new(0, 0),
                        canvas_size,
                        DrawStyle::new(PixelColor::Light, PixelColor::Dark, 3),
                    ),
                    5,
                ),
            )
            .unwrap();

        // draw the line items
        // we require that the items list be in index-order, with no holes: we abort at the first None item
        for cur_index in 0..self.items.len() {
            if self.index == cur_index as usize {
                self.draw_item(cur_index as i16, true);
            } else {
                self.draw_item(cur_index as i16, false);
            }
            if cur_index != 0 {
                self.draw_divider(cur_index as _);
            }
        }
        log::trace!("menu redraw##");
        self.gam.redraw().unwrap();
    }

    fn num_items(&self) -> usize { self.items.len() }

    pub fn key_event(&mut self, keys: [char; 4]) {
        for &k in keys.iter() {
            log::debug!("got key '{}'", k);
            match k {
                '∴' => {
                    let mi = self.items[self.index];
                    // give up focus before issuing the command, as some commands conflict with loss of
                    // focus...
                    if mi.close_on_select {
                        self.gam.relinquish_focus().unwrap();
                        xous::yield_slice();
                        #[cfg(not(target_os = "xous"))]
                        ticktimer_server::Ticktimer::new().unwrap().sleep_ms(100).unwrap();
                    }
                    if let Some(action) = mi.action_conn {
                        log::debug!("doing menu action for {}", mi.name);
                        #[cfg(feature = "tts")]
                        {
                            let mut phrase = "select ".to_string();
                            phrase.push_str(mi.name.as_str().unwrap());
                            self.tts.tts_blocking(&phrase).unwrap();
                        }
                        match mi.action_payload {
                            MenuPayload::Scalar(args) => {
                                xous::send_message(
                                    action,
                                    xous::Message::new_scalar(
                                        mi.action_opcode as usize,
                                        args[0] as usize,
                                        args[1] as usize,
                                        args[2] as usize,
                                        args[3] as usize,
                                    ),
                                )
                                .expect("couldn't send menu action");
                            }
                            MenuPayload::Memory((_buf, _len)) => {
                                unimplemented!("menu buffer targets are a future feature");
                            }
                        }
                    }
                    self.index = 0; // reset the index to 0
                    if !mi.close_on_select {
                        // fix a double-redraw issue. I relinquish_focus() maps to active() which contains a
                        // redraw() already.
                        log::trace!("menu redraw## select key");
                        self.gam.redraw().unwrap();
                    }
                    break; // drop any characters that happened to trail the select key, it's probably a fat-finger error.
                }
                '←' => {
                    // placeholder
                    log::trace!("got left arrow");
                }
                '→' => {
                    // placeholder
                    log::trace!("got right arrow");
                }
                '↑' => {
                    self.prev_item();
                    log::trace!("menu redraw## up key");
                    self.gam.redraw().unwrap();
                }
                '↓' => {
                    self.next_item();
                    log::trace!("menu redraw## down key");
                    self.gam.redraw().unwrap();
                }
                _ => {}
            }
        }
    }
}

pub struct MenuMatic {
    cid: xous::CID,
}
impl MenuMatic {
    pub fn add_item(&self, item: MenuItem) -> bool {
        let mm = MenuManagement { item, op: MenuMgrOp::AddItem };
        let mut buf = Buffer::into_buf(mm).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.cid, 0).expect("Couldn't issue management opcode");
        let ret = buf.to_original::<MenuManagement, _>().unwrap();
        if ret.op == MenuMgrOp::Ok { true } else { false }
    }

    pub fn insert_item(&self, item: MenuItem, at: usize) -> bool {
        let mm = MenuManagement { item, op: MenuMgrOp::InsertItem(at) };
        let mut buf = Buffer::into_buf(mm).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.cid, 0).expect("Couldn't issue management opcode");
        let ret = buf.to_original::<MenuManagement, _>().unwrap();
        if ret.op == MenuMgrOp::Ok { true } else { false }
    }

    pub fn delete_item(&self, item_name: &str) -> bool {
        let mm = MenuManagement {
            item: MenuItem {
                name: String::from_str(item_name),
                // the rest are ignored
                action_conn: None,
                action_opcode: 0,
                action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                close_on_select: false,
            },
            op: MenuMgrOp::DeleteItem,
        };
        let mut buf = Buffer::into_buf(mm).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.cid, 0).expect("Couldn't issue management opcode");
        let ret = buf.to_original::<MenuManagement, _>().unwrap();
        if ret.op == MenuMgrOp::Ok { true } else { false }
    }

    pub fn set_index(&self, index: usize) {
        let op = MenuManagement {
            item: MenuItem {
                // dummy item, not used
                name: xous_ipc::String::<64>::new(),
                action_conn: None,
                action_opcode: 0,
                action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                close_on_select: false,
            },
            op: MenuMgrOp::SetIndex(index),
        };
        let mut buf = xous_ipc::Buffer::into_buf(op).expect("couldn't transform to memory");
        buf.lend_mut(self.cid, 0).expect("couldn't set menu index");
        // do nothing with the return code
    }

    pub fn quit(&self) {
        let mm = MenuManagement {
            item: MenuItem {
                // dummy record
                name: String::new(),
                action_conn: None,
                action_opcode: 0,
                action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
                close_on_select: false,
            },
            op: MenuMgrOp::Quit,
        };
        let mut buf = Buffer::into_buf(mm).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.cid, 0).expect("Couldn't issue management opcode");
    }
}
use std::sync::{Arc, Mutex};
use std::thread;
/// Builds a menu that is described by a vector of MenuItems, and then manages it.
/// If you want to modify the menu, pass it a Some(xous::SID) which is the private server
/// address of the management interface.
pub fn menu_matic(
    items: Vec<MenuItem>,
    menu_name: &'static str,
    maybe_manager: Option<xous::SID>,
) -> Option<MenuMatic> {
    log::debug!("building menu '{:?}'", menu_name);
    let mut naked_menu = Menu::new(menu_name);
    for item in items {
        naked_menu.add_item(item);
    }
    let menu = Arc::new(Mutex::new(naked_menu));
    let _ = thread::spawn({
        let menu = menu.clone();
        let sid = menu.lock().unwrap().sid.clone();
        move || {
            loop {
                let msg = xous::receive_message(sid).unwrap();
                log::trace!("message: {:?}", msg);
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(MenuOpcode::Redraw) => {
                        menu.lock().unwrap().redraw();
                    }
                    Some(MenuOpcode::Rawkeys) => xous::msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                        let keys = [
                            core::char::from_u32(k1 as u32).unwrap_or('\u{0000}'),
                            core::char::from_u32(k2 as u32).unwrap_or('\u{0000}'),
                            core::char::from_u32(k3 as u32).unwrap_or('\u{0000}'),
                            core::char::from_u32(k4 as u32).unwrap_or('\u{0000}'),
                        ];
                        menu.lock().unwrap().key_event(keys);
                    }),
                    Some(MenuOpcode::Quit) => {
                        xous::return_scalar(msg.sender, 1).unwrap();
                        break;
                    }
                    None => {
                        log::error!("unknown opcode {:?}", msg.body.id());
                    }
                }
            }
            log::trace!("menu thread exit, destroying servers");
            // do we want to add a deregister_ux call to the system?
            xous::destroy_server(menu.lock().unwrap().sid).unwrap();
        }
    });
    if let Some(manager) = maybe_manager {
        let _ = std::thread::spawn({
            let menu = menu.clone();
            move || {
                loop {
                    let mut msg = xous::receive_message(manager).unwrap();
                    // this particular manager only expcets/handles memory messages, so its loop is a bit
                    // different than the others
                    let mut buffer =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    let mut mgmt = buffer
                        .to_original::<MenuManagement, _>()
                        .expect("menu manager received unexpected message type");
                    match mgmt.op {
                        MenuMgrOp::AddItem => {
                            menu.lock().unwrap().add_item(mgmt.item);
                            mgmt.op = MenuMgrOp::Ok;
                            buffer.replace(mgmt).unwrap();
                        }
                        MenuMgrOp::InsertItem(at) => {
                            if menu.lock().unwrap().insert_item(mgmt.item, at) {
                                mgmt.op = MenuMgrOp::Ok;
                                buffer.replace(mgmt).unwrap();
                            } else {
                                mgmt.op = MenuMgrOp::Err;
                                buffer.replace(mgmt).unwrap();
                            }
                        }
                        MenuMgrOp::DeleteItem => {
                            if !menu.lock().unwrap().delete_item(mgmt.item.name.as_str().unwrap()) {
                                mgmt.op = MenuMgrOp::Err;
                            } else {
                                mgmt.op = MenuMgrOp::Ok;
                            }
                            buffer.replace(mgmt).unwrap();
                        }
                        MenuMgrOp::SetIndex(index) => {
                            log::info!("setting menu index {}", index);
                            menu.lock().unwrap().set_index(index);
                            log::info!("index is set");
                            mgmt.op = MenuMgrOp::Ok;
                            buffer.replace(mgmt).unwrap();
                        }
                        MenuMgrOp::Quit => {
                            mgmt.op = MenuMgrOp::Ok;
                            buffer.replace(mgmt).unwrap();
                            break;
                        }
                        _ => {
                            log::error!("Unhandled opcode: {:?}", mgmt.op);
                        }
                    }
                }
                xous::destroy_server(manager).unwrap();
            }
        });
        Some(MenuMatic { cid: xous::connect(manager).unwrap() })
    } else {
        None
    }
}
