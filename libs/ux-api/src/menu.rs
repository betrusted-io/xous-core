use std::fmt::Write;

use num_traits::*;
use xous_ipc::Buffer;

use crate::minigfx::*;
use crate::service::api::*;
use crate::service::gfx::Gfx;
use crate::widgets::ScrollableList;
use crate::{MsgForwarder, forwarding_thread};

#[derive(Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub(crate) enum MenuMgrOp {
    // incoming is one of these ops
    AddItem,
    InsertItem(usize),
    DeleteItem,
    SetIndex(usize),
    Quit,
    Redraw,
    KeyPress(char),
    // response must be one of these
    Ok,
    Err,
}

#[allow(dead_code)] // here until Memory types are implemented
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum MenuPayload {
    /// memorized scalar payload
    Scalar([u32; 4]),
    /// this a nebulous-but-TBD maybe way of bodging in a more complicated record, which would involve
    /// casting this memorized, static payload into a Buffer and passing it on. Let's not worry too much
    /// about it for now, it's mostly apirational...
    Memory(([u8; 256], usize)),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub(crate) struct MenuManagement {
    pub(crate) item: MenuItem,
    pub(crate) op: MenuMgrOp,
}

#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct MenuItem {
    pub name: String,
    /// if action_conn is None, this is a NOP menu item (it just does nothing and closes the menu)
    pub action_conn: Option<xous::CID>,
    pub action_opcode: u32, // this is ignored if action_conn is None
    pub action_payload: MenuPayload,
    pub close_on_select: bool,
}
impl MenuItem {
    pub fn default() -> Self {
        MenuItem {
            name: String::new(),
            action_conn: None,
            action_opcode: 0,
            action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: false,
        }
    }
}

#[derive(Debug)]
pub struct Menu<'a> {
    pub gfx: Gfx,
    pub sid: xous::SID,
    pub items: Vec<MenuItem>,
    pub helper_data: Option<Buffer<'a>>,
    pub name: String,
    pub list: ScrollableList, // UI rendering element
    pub parent_conn: xous::CID,
    /// Opcode for notifying the parent that the menu is done and should redraw itself
    pub parent_redraw_op: usize,
    pub title_tv: TextView,
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
    pub fn new(name: &str, parent_conn: xous::CID, parent_redraw_op: usize) -> Menu<'_> {
        let xns = xous_names::XousNames::new().unwrap();
        let sid = xous::create_server().expect("can't create private menu message server");
        let mut list = ScrollableList::default();
        list.set_alignment(crate::widgets::TextAlignment::Center);
        let mut title_tv = TextView::new(
            Gid::dummy(),
            TextBounds::CenteredTop(Rectangle::new(
                Point::new(0, 0),
                Point::new(crate::platform::WIDTH as isize, list.row_height() as isize),
            )),
        );
        title_tv.draw_border = true;
        title_tv.invert = true;
        write!(title_tv, "{}", name).ok();
        Menu {
            gfx: Gfx::new(&xns).unwrap(),
            sid,
            items: Vec::new(),
            helper_data: None,
            name: String::from(name),
            list,
            parent_conn,
            parent_redraw_op,
            title_tv,
        }
    }

    pub fn set_index(&mut self, index: usize) { self.list.set_selected(0, index).unwrap() }

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
        let _ = std::thread::spawn({
            move || {
                forwarding_thread(addr, size, offset);
            }
        });
    }

    /// Appends a menu item to the end of the current Menu
    pub fn add_item(&mut self, new_item: MenuItem) {
        if new_item.name.as_str() == "🔇" {
            // suppress the addition of menu items that are not applicable for a given locale
            return;
        }
        self.list.add_item(0, &new_item.name);
        self.items.push(new_item);
    }

    // Attempts to insert a MenuItem at the index given. This displaces the item at that index down
    // by one slot. Returns false if the index is invalid.
    pub fn insert_item(&mut self, new_item: MenuItem, at: usize) -> bool {
        if new_item.name.as_str() == "🔇" {
            // suppress the addition of menu items that are not applicable for a given locale
            return false;
        }
        self.list.insert_item(0, at, &new_item.name);
        if at <= self.items.len() {
            self.items.insert(at, new_item);
            true
        } else {
            false
        }
    }

    // note: this routine has yet to be tested. (remove this comment once it has been actually used by
    // something)
    pub fn delete_item(&mut self, item: &str) -> bool {
        let item_index = match self.items.iter().position(|menu_item| &menu_item.name == item) {
            Some(index) => index,
            None => return false,
        };
        self.list.delete_item(0, item_index);

        let len_before = self.items.len();
        self.items.retain(|candidate| candidate.name.as_str() != item);

        // now, recompute the height
        if len_before > self.items.len() { true } else { false }
    }

    pub fn redraw(&mut self) {
        // clear pending ops
        self.gfx.flush().unwrap();
        self.gfx.clear().unwrap();

        // issue new drawlist
        self.gfx.draw_textview(&mut self.title_tv).ok();
        self.gfx
            .draw_line(Line::new_with_style(
                Point::new(0, self.list.row_height() as isize + 1),
                Point::new(crate::platform::WIDTH as isize, self.list.row_height() as isize + 1),
                DrawStyle::new(PixelColor::Light, PixelColor::Light, 3),
            ))
            .ok();
        let title_height = self
            .title_tv
            .bounds_computed
            .unwrap_or(Rectangle::new(
                Point::new(0, 0),
                Point::new(crate::platform::WIDTH as isize, self.list.row_height() as isize),
            ))
            .height() as isize;
        self.list.pane_size(Rectangle::new(
            Point::new(0, 0),
            Point::new(crate::platform::WIDTH, crate::platform::HEIGHT as isize - title_height),
        ));
        self.list.draw(title_height);
    }

    pub fn key_event(&mut self, keys: [char; 4]) {
        for &k in keys.iter() {
            log::debug!("got key '{}'", k);
            match k {
                '∴' => {
                    let (_, selected) = self.list.get_selected_index();
                    log::info!("selected index {}", selected);
                    let mi = &self.items[selected];
                    if let Some(action) = mi.action_conn {
                        log::debug!("doing menu action for {}", mi.name);
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
                    self.list.set_selected(0, 0).ok(); // reset selection
                    if !mi.close_on_select {
                        self.redraw();
                    } else {
                        // inform the parent to redraw itself
                        xous::send_message(
                            self.parent_conn,
                            xous::Message::new_scalar(self.parent_redraw_op, 0, 0, 0, 0),
                        )
                        .ok();
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
                    self.list.key_action(k);
                    self.redraw();
                }
                '↓' => {
                    self.list.key_action(k);
                    self.redraw();
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
        let mut item = MenuItem::default();
        item.name = item_name.to_owned();
        let mm = MenuManagement { item, op: MenuMgrOp::DeleteItem };
        let mut buf = Buffer::into_buf(mm).expect("Couldn't convert to memory structure");
        buf.lend_mut(self.cid, 0).expect("Couldn't issue management opcode");
        let ret = buf.to_original::<MenuManagement, _>().unwrap();
        if ret.op == MenuMgrOp::Ok { true } else { false }
    }

    pub fn set_index(&self, index: usize) {
        let op = MenuManagement { item: MenuItem::default(), op: MenuMgrOp::SetIndex(index) };
        let mut buf = xous_ipc::Buffer::into_buf(op).expect("couldn't transform to memory");
        buf.lend_mut(self.cid, 0).expect("couldn't set menu index");
        // do nothing with the return code
    }

    pub fn redraw(&self) {
        let op = MenuManagement { item: MenuItem::default(), op: MenuMgrOp::Redraw };
        let mut buf = xous_ipc::Buffer::into_buf(op).expect("couldn't transform to memory");
        buf.lend_mut(self.cid, 0).expect("couldn't set menu index");
        // do nothing with the return code
    }

    pub fn key_press(&self, key: char) {
        let op = MenuManagement { item: MenuItem::default(), op: MenuMgrOp::KeyPress(key) };
        let mut buf = xous_ipc::Buffer::into_buf(op).expect("couldn't transform to memory");
        buf.lend_mut(self.cid, 0).expect("couldn't set menu index");
        // do nothing with the return code
    }

    pub fn quit(&self) {
        let mm = MenuManagement { item: MenuItem::default(), op: MenuMgrOp::Quit };
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
    parent_conn: xous::CID,
    parent_redraw_op: usize,
) -> Option<MenuMatic> {
    log::debug!("building menu '{:?}'", menu_name);
    let mut naked_menu = Menu::new(menu_name, parent_conn, parent_redraw_op);
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
                    log::debug!("menu manager op: {:?}", mgmt.op);
                    match mgmt.op {
                        MenuMgrOp::AddItem => {
                            menu.lock().unwrap().add_item(mgmt.item.clone());
                            mgmt.op = MenuMgrOp::Ok;
                            buffer.replace(mgmt).unwrap();
                        }
                        MenuMgrOp::InsertItem(at) => {
                            if menu.lock().unwrap().insert_item(mgmt.item.clone(), at) {
                                mgmt.op = MenuMgrOp::Ok;
                                buffer.replace(mgmt).unwrap();
                            } else {
                                mgmt.op = MenuMgrOp::Err;
                                buffer.replace(mgmt).unwrap();
                            }
                        }
                        MenuMgrOp::DeleteItem => {
                            if !menu.lock().unwrap().delete_item(mgmt.item.name.as_str()) {
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
                        MenuMgrOp::Redraw => {
                            menu.lock().unwrap().redraw();
                            mgmt.op = MenuMgrOp::Ok;
                            buffer.replace(mgmt).unwrap();
                        }
                        MenuMgrOp::KeyPress(c) => {
                            menu.lock().unwrap().key_event([c, '\u{0000}', '\u{0000}', '\u{0000}']);
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
