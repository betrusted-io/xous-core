use gam::SetCanvasBoundsRequest;
use xous_ipc::*;
use graphics_server::*;
use num_traits::*;
use xous::msg_scalar_unpack;

const MAX_ITEMS: usize = 16;

#[allow(dead_code)] // here until Memory types are implemented
#[derive(Debug, Copy, Clone)]
pub enum MenuPayload {
    /// memorized scalar payload
    Scalar([u32; 4]),
    /// this a nebulous-but-TBD maybe way of bodging in a more complicated record, which would involve
    /// casting this memorized, static payload into a Buffer and passing it on. Let's not worry too much about it for now, it's mostly apirational...
    Memory(([u8; 256], usize)),
}
#[derive(Debug, Copy, Clone)]
pub struct MenuItem {
    name: String::<64>,
    action_conn: xous::CID,
    action_opcode: u32,
    action_payload: MenuPayload,
}

#[derive(Debug)]
pub struct Menu {
    pub sid: xous::SID,
    pub gam: gam::Gam,
    pub xns: xous_names::XousNames,
    pub items: [Option<MenuItem>; MAX_ITEMS],
    pub index: usize, // currently selected item
    pub canvas: Gid,
    pub authtoken: [u32; 4],
    pub margin: i16,
    pub divider_margin: i16,
    pub line_height: i16,
    pub canvas_width: Option<i16>,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum MenuOpcode {
    Redraw,
    Rawkeys,
    Quit,
}

impl Menu {
    pub fn new(name: &str) -> Menu {
        let xns = xous_names::XousNames::new().unwrap();
        let sid = xous::create_server().expect("can't create private menu message server");
        let gam = gam::Gam::new(&xns).expect("can't connect to GAM");
        let authtoken = gam.register_ux(
            gam::UxRegistration {
                app_name: String::<128>::from_str(name),
                ux_type: gam::UxType::Menu,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: MenuOpcode::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                rawkeys_id: Some(MenuOpcode::Rawkeys.to_u32().unwrap()),
            }
        ).expect("couldn't register my Ux element with GAM");
        log::debug!("requesting content canvas for menu");
        let canvas = gam.request_content_canvas(authtoken.unwrap()).expect("couldn't get my content canvas from GAM");
        let line_height = gam.glyph_height_hint(gam::GlyphStyle::Regular).expect("couldn't get glyph height hint") as i16;
        Menu {
            sid,
            gam,
            xns,
            items: [None; MAX_ITEMS],
            index: 0,
            canvas,
            authtoken: authtoken.unwrap(),
            margin: 4,
            divider_margin: 20,
            line_height,
            canvas_width: None,
        }
    }
    // if successful, returns None, otherwise, the menu item
    pub fn add_item(&mut self, new_item: MenuItem) -> Option<MenuItem> {
        // first, do the insertion.
        // add the menu item to the first free slot
        // any modifications to the menu structure should guarantee that the list is compacted
        // and has no holes, in order for the "selected index" logic to work
        let mut success = false;
        for item in self.items.iter_mut() {
            if item.is_none() {
                *item = Some(new_item);
                success = true;
                break;
            }
        }

        if success {
            // now, recompute the height
            let mut total_items = self.num_items();
            if total_items == 0 {
                total_items = 1; // just so we see a blank menu at least, and have a clue how to debug
            }
            let current_bounds = self.gam.get_canvas_bounds(self.canvas).expect("couldn't get current bounds");
            let mut new_bounds = SetCanvasBoundsRequest {
                requested: Point::new(current_bounds.x, total_items as i16 * self.line_height + self.margin * 2),
                granted: None,
                token_type: gam::TokenType::App,
                token: self.authtoken,
            };
            log::debug!("add_item requesting bounds of {:?}", new_bounds);
            self.gam.set_canvas_bounds_request(&mut new_bounds).expect("couldn't call set bounds");

            None
        } else {
            Some(new_item)
        }
    }
    pub fn draw_item(&self, index: i16, with_marker: bool) {
        use core::fmt::Write;
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();

        if let Some(item) = self.items[index as usize] {
            let mut item_tv = TextView::new(
                self.canvas,
                TextBounds::BoundingBox(Rectangle::new(
                    Point::new(self.margin, index * self.line_height + self.margin),
                    Point::new(canvas_size.x - self.margin, (index + 1) * self.line_height + self.margin),
                )));

            if with_marker {
                write!(item_tv.text, " • ").unwrap();
            } else {
                write!(item_tv.text, "    ").unwrap();
            }
            write!(item_tv.text, "{}", item.name.as_str().unwrap()).unwrap();
            item_tv.draw_border = false;
            item_tv.style = GlyphStyle::Small;
            item_tv.margin = Point::new(0, 0);
            item_tv.ellipsis = true;

            self.gam.post_textview(&mut item_tv).expect("couldn't render menu list item");
        }
    }
    // draw a dividing line above the indexed item
    pub fn draw_divider(&self, index: i16) {
        if let Some(canvas_width) = self.canvas_width {
            self.gam.draw_line(self.canvas, Line::new_with_style(
                Point::new(self.divider_margin, index * self.line_height + self.margin/2),
                Point::new(canvas_width - self.divider_margin, index * self.line_height + self.margin/2),
                DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1))
                ).expect("couldn't draw dividing line")
        } else {
            log::debug!("cant draw divider because our canvas width was not initialized. Ignoring request.");
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
            self.draw_divider(self.index as i16);
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
        }
    }
    pub fn redraw(&mut self) {
        // for now, just draw a black rectangle
        log::trace!("menu redraw");
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();
        // draw the outer border
        self.gam.draw_rounded_rectangle(self.canvas,
            RoundedRectangle::new(
                Rectangle::new_with_style(Point::new(0, 0), canvas_size,
                    DrawStyle::new(PixelColor::Light, PixelColor::Dark, 3)
                ), 5
            )).unwrap();

        // draw the line items
        // we require that the items list be in index-order, with no holes: we abort at the first None item
        let mut cur_index: i16 = 0;
        for maybe_item in self.items.iter() {
            if let Some(_item) = maybe_item {
                if self.index == cur_index as usize {
                    self.draw_item(cur_index as i16, true);
                } else {
                    self.draw_item(cur_index as i16, false);
                }
                if cur_index != 0 {
                    self.draw_divider(cur_index);
                }

                cur_index += 1;
            } else {
                break;
            }
        }
    }
    fn num_items(&self) -> usize {
        let mut items = 0;
        for maybe_item in self.items.iter() {
            if maybe_item.is_some() {
                items += 1;
            } else {
                break;
            }
        }
        items
    }
    pub fn key_event(&mut self, keys: [char; 4]) {
        for &k in keys.iter() {
            log::debug!("got key '{}'", k);
            match k {
                '∴' => {
                    if let Some(mi) = self.items[self.index] {
                        log::debug!("doing menu action for {}", mi.name);
                        match mi.action_payload {
                            MenuPayload::Scalar(args) => {
                                xous::send_message(mi.action_conn,
                                    xous::Message::new_scalar(mi.action_opcode as usize,
                                        args[0] as usize, args[1] as usize, args[2] as usize, args[3] as usize)
                                ).expect("couldn't send menu action");
                            },
                            MenuPayload::Memory((_buf, _len)) => {
                                unimplemented!("menu buffer targets are a future feature");
                            }
                        }
                    }
                },
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
                }
                '↓' => {
                    self.next_item();
                }
                _ => {}
            }
        }
    }
}

/////// strictly speaking this doesn't have to be in this file, but we make it part of this server so we are guaranteed to have a main menu at all times
pub fn main_menu_thread() {
    let mut menu = Menu::new(crate::MAIN_MENU_NAME);

    let thing_item = MenuItem {
        name: String::<64>::from_str("Do a thing"),
        action_conn: menu.gam.conn(),
        action_opcode: crate::Opcode::RevertFocus.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
    };
    menu.add_item(thing_item);

    let another_item = MenuItem {
        name: String::<64>::from_str("Another thing"),
        action_conn: menu.gam.conn(),
        action_opcode: crate::Opcode::RevertFocus.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
    };
    menu.add_item(another_item);

    let close_item = MenuItem {
        name: String::<64>::from_str("Close Menu"),
        action_conn: menu.gam.conn(),
        action_opcode: crate::Opcode::RevertFocus.to_u32().unwrap(),
        action_payload: MenuPayload::Scalar([0, 0, 0, 0]),
    };
    menu.add_item(close_item);

    loop {
        let msg = xous::receive_message(menu.sid).unwrap();
        log::trace!("|status: Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(MenuOpcode::Redraw) => {
                menu.redraw();
            },
            Some(MenuOpcode::Rawkeys) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    if let Some(a) = core::char::from_u32(k1 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k2 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k3 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k4 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                ];
                menu.key_event(keys);
            }),
            Some(MenuOpcode::Quit) => {
                break;
            },
            None => {
                log::error!("unknown opcode {:?}", msg.body.id());
            }
        }
    }
    log::trace!("menu thread exit, destroying servers");
    // do we want to add a deregister_ux call to the system?
    xous::destroy_server(menu.sid).unwrap();
}