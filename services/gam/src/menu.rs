use gam::SetCanvasBoundsRequest;
use xous_ipc::*;
use graphics_server::*;
use num_traits::*;
use xous::msg_scalar_unpack;

const MAX_ITEMS: usize = 16;

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
            let mut total_items = 0;
            for item in self.items.iter() {
                if item.is_some() {
                    total_items += 1;
                }
            }
            if total_items == 0 {
                total_items = 1; // just so we see a blank menu at least, and have a clue how to debug
            }
            let line_height = self.gam.glyph_height_hint(gam::GlyphStyle::Regular).expect("couldn't get glyph height hint");
            let current_bounds = self.gam.get_canvas_bounds(self.canvas).expect("couldn't get current bounds");
            let mut new_bounds = SetCanvasBoundsRequest {
                requested: Point::new(current_bounds.x, (total_items * line_height + self.margin as usize * 2) as i16),
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
    pub fn redraw(&mut self) {
        use core::fmt::Write;

        // for now, just draw a black rectangle
        log::trace!("menu redraw");
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();
        let line_height = self.gam.glyph_height_hint(gam::GlyphStyle::Regular).expect("couldn't get glyph height hint") as i16;
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
        let mut first_item = true;
        for maybe_item in self.items.iter() {
            if let Some(item) = maybe_item {
                let mut item_tv = TextView::new(
                    self.canvas,
                    TextBounds::BoundingBox(Rectangle::new(
                        Point::new(self.margin, cur_index * line_height + self.margin as i16),
                        Point::new(canvas_size.x - self.margin, (cur_index + 1) * line_height + self.margin as i16),
                    )));

                if self.index == cur_index as usize {
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

                if !first_item {
                    // draw a dividing line above this item
                    self.gam.draw_line(self.canvas, Line::new_with_style(
                        Point::new(self.divider_margin, cur_index * line_height + self.margin/2),
                        Point::new(canvas_size.x - self.divider_margin, cur_index * line_height + self.margin/2),
                        DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1))
                    ).expect("couldn't draw dividing line")
                }

                cur_index += 1;
                first_item = false;
            } else {
                break;
            }
        }
    }
    pub fn key_event(&mut self, keys: [char; 4]) {
        for &k in keys.iter() {
            log::trace!("got key '{}'", k);
            match k {
                '∴' => {
                    log::info!("relinquishing focus");
                    self.gam.relinquish_focus().expect("couldn't relinquish focus of the menu!");
                },
                '←' => {
                    log::info!("got left arrow");
                }
                '→' => {
                    log::info!("got right arrow");
                }
                '↑' => {
                    log::info!("got up arrow");
                }
                '↓' => {
                    log::info!("got down arrow");
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