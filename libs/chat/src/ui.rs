use super::*;
//use crate::{ChatOp, Dialogue, Event, Post, CHAT_SERVER_NAME};
use dialogue::{author::Author, post::Post, Dialogue};
use gam::{menu_matic, MenuMatic, MenuPayload, UxRegistration};
use graphics_server::api::GlyphStyle;
use graphics_server::{DrawStyle, Gid, PixelColor, Point, Rectangle, TextBounds, TextView};
use locales::t;
use modals::Modals;
use rkyv::de::deserializers::AllocDeserializer;
use rkyv::ser::serializers::WriteSerializer;
use rkyv::ser::Serializer;
use rkyv::Deserialize;
use std::convert::TryFrom;

use std::io::{Error, ErrorKind, Read, Write};
use xous::{MessageEnvelope, CID};

use xous_names::XousNames;

#[allow(dead_code)]
pub(crate) struct Ui {
    // optional structures that indicate new input to the Chat loop per iteration
    // an input string
    pub input: Option<xous_ipc::String<{ POST_TEXT_MAX }>>,
    // messages from other servers
    msg: Option<MessageEnvelope>,

    // Pddb connection
    pddb: pddb::Pddb,
    pddb_dict: Option<String>,
    pddb_key: Option<String>,
    dialogue: Option<Dialogue>,

    // Callbacks:
    // optional SID of the "Owner" Chat App to receive UI-events
    app_cid: Option<CID>,
    // optional opcode ID to process UI-event msgs
    opcode_event: Option<usize>,

    canvas: Gid,
    gam: gam::Gam,
    modals: Modals,

    // variables that define our graphical attributes
    screensize: Point,
    bubble_width: u16,
    margin: Point,        // margin to edge of canvas
    bubble_margin: Point, // margin of text in bubbles
    bubble_radius: u16,
    bubble_space: i16, // spacing between text bubbles

    // variables that define a menu
    app_menu: String,
    menu_mgr: MenuMatic,

    // our security token for making changes to our record on the GAM
    token: [u32; 4],
}

#[allow(dead_code)]
impl Ui {
    pub(crate) fn new(
        sid: xous::SID,
        app_name: &'static str,
        app_menu: &'static str,
        app_cid: Option<xous::CID>,
        opcode_event: Option<usize>,
    ) -> Self {
        let xns = XousNames::new().unwrap();
        let gam = gam::Gam::new(&xns).expect("can't connect to GAM");

        let token = gam
            .register_ux(UxRegistration {
                app_name: xous_ipc::String::<128>::from_str(app_name),
                ux_type: gam::UxType::Chat,
                predictor: Some(xous_ipc::String::<64>::from_str(
                    ime_plugin_shell::SERVER_NAME_IME_PLUGIN_SHELL,
                )),
                listener: sid.to_array(), // note disclosure of our SID to the GAM -- the secret is now shared with the GAM!
                redraw_id: ChatOp::GamRedraw as u32,
                gotinput_id: Some(ChatOp::GamLine as u32),
                audioframe_id: None,
                rawkeys_id: Some(ChatOp::GamRawkeys as u32),
                focuschange_id: Some(ChatOp::GamChangeFocus as u32),
            })
            .expect("couldn't register Ux context for chat");
        let xns = XousNames::new().unwrap();
        let modals = Modals::new(&xns).unwrap();
        let canvas = gam
            .request_content_canvas(token.unwrap())
            .expect("couldn't get content canvas");
        let screensize = gam
            .get_canvas_bounds(canvas)
            .expect("couldn't get dimensions of content canvas");
        let pddb = pddb::Pddb::new();
        pddb.try_mount();
        let menu_mgr = menu_matic(
            Vec::<MenuItem>::new(),
            app_menu,
            Some(xous::create_server().unwrap()),
        )
        .expect("couldn't create MenuMatic manager");
        Ui {
            input: None,
            msg: None,
            pddb: pddb,
            pddb_dict: None,
            pddb_key: None,
            dialogue: None,
            app_cid,
            opcode_event,
            canvas,
            gam,
            modals,
            screensize,
            bubble_width: ((screensize.x / 5) * 4) as u16, // 80% width for the text bubbles
            margin: Point::new(4, 4),
            bubble_margin: Point::new(4, 4),
            bubble_radius: 4,
            bubble_space: 4,
            app_menu: app_menu.to_owned(),
            menu_mgr: menu_mgr,
            token: token,
        }
    }

    fn dialogue_read(&mut self) -> Result<(), Error> {
        match (&self.pddb_dict, &self.pddb_key) {
            (Some(dict), Some(key)) => {
                match self
                    .pddb
                    .get(&dict, &key, None, false, false, None, None::<fn()>)
                {
                    Ok(mut pddb_key) => {
                        let mut bytes = [0u8; dialogue::MAX_BYTES + 2];
                        match pddb_key.read(&mut bytes) {
                            Ok(_) => {
                                // extract pos u16 from the first 2 bytes
                                let pos: u16 = u16::from_be_bytes([bytes[0], bytes[1]]);
                                let pos: usize = pos.into();
                                // deserialize the Dialogue
                                let archive =
                                    unsafe { rkyv::archived_value::<Dialogue>(&bytes, pos) };
                                self.dialogue = match archive.deserialize(&mut AllocDeserializer {})
                                {
                                    Ok(dialogue) => Some(dialogue),
                                    Err(e) => {
                                        log::warn!(
                                            "failed to deserialize Dialogue {}:{} {}",
                                            dict,
                                            key,
                                            e
                                        );
                                        None
                                    }
                                };
                                log::debug!("get '{}' = '{:?}'", key, self.dialogue);
                            }
                            Err(e) => log::warn!("failed to read {}: {e}", key),
                        }
                    }
                    Err(e) => {
                        log::warn!("failed to get {}: {e}", key);
                        return Err(Error::new(ErrorKind::InvalidData, "missing"));
                    }
                }
                Ok(())
            }
            _ => {
                log::warn!("missing pddb dict or key");
                Err(Error::new(ErrorKind::InvalidData, "missing"))
            }
        }
    }

    fn dialogue_save(&self) -> Result<(), Error> {
        match (&self.dialogue, &self.pddb_dict, &self.pddb_key) {
            (Some(dialogue), Some(dict), Some(key)) => {
                let hint = Some(dialogue::MAX_BYTES + 2);
                match self
                    .pddb
                    .get(&dict, &key, None, true, true, hint, None::<fn()>)
                {
                    Ok(mut pddb_key) => {
                        let mut buf = Vec::<u8>::new();
                        // reserve 2 bytes to hold a u16 (see below)
                        let reserved = 2;
                        buf.push(0u8);
                        buf.push(0u8);

                        // serialize the Dialogue
                        let mut serializer = WriteSerializer::with_pos(buf, reserved);
                        let pos = serializer.serialize_value(dialogue).unwrap();
                        let mut bytes = serializer.into_inner();

                        // copy pop u16 into the first 2 bytes to enable the rkyv archive to be deserialised
                        let pos: u16 = u16::try_from(pos).expect("data > u16");
                        let pos_bytes = pos.to_be_bytes();
                        bytes[0] = pos_bytes[0];
                        bytes[1] = pos_bytes[1];
                        match pddb_key.write(&bytes) {
                            Ok(len) => {
                                self.pddb.sync().ok();
                                log::info!("Wrote {} bytes to {}:{}", len, dict, key);
                            }
                            Err(e) => {
                                log::warn!("Error writing {}:{}: {:?}", dict, key, e);
                            }
                        }
                    }
                    Err(e) => log::warn!("failed to create {}:{}\n{}", dict, key, e),
                }
                Ok(())
            }
            _ => {
                log::warn!("missing dict, key or dialogue");
                Ok(())
            }
        }
    }

    // set the current Dialogue
    pub fn dialogue_set(&mut self, pddb_dict: &str, pddb_key: Option<&str>) {
        self.pddb_dict = Some(pddb_dict.to_string());
        self.pddb_key = pddb_key.map(|key| key.to_string());
        if self.pddb_key.is_none() {
            self.dialogue_modal();
        }
        log::info!("Dialogue set to {:?}:{:?}", self.pddb_dict, self.pddb_key);
        match self.dialogue_read() {
            Ok(_) => {
                log::info!("read dialogue {:?}:{:?}", self.pddb_dict, self.pddb_key);
                self.redraw().expect("couldn't redraw screen");
            }
            Err(_) => {
                if let Some(key) = &self.pddb_key {
                    self.dialogue = Some(Dialogue::new(&key));
                    match self.dialogue_save() {
                        Ok(_) => log::info!("Dialogue created {}:{}", pddb_dict, key),
                        Err(e) => {
                            log::warn!("Failed to create Dialogue {}:{} : {e}", pddb_dict, key)
                        }
                    }
                }
            }
        }
    }

    pub fn dialogue_modal(&mut self) {
        if let Some(dict) = &self.pddb_dict {
            match self.pddb.list_keys(&dict, None) {
                Ok(keys) => {
                    if keys.len() > 0 {
                        self.modals
                            .add_list(keys.iter().map(|s| s.as_str()).collect())
                            .expect("failed modal add_list");
                        self.pddb_key = self
                            .modals
                            .get_radiobutton(t!("chat.dialogue_title", locales::LANG))
                            .ok();
                        log::info!("selected dialogue {}:{:?}", dict, self.pddb_key);
                    } else {
                        self.modals
                            .show_notification(t!("chat.dict_empty", locales::LANG), None)
                            .expect("notification failed");
                    }
                }
                Err(e) => log::warn!("failed to list pddb keys: {e}"),
            }
        }
    }

    pub fn menu_add(&self, item: MenuItem) {
        self.menu_mgr.add_item(item);
    }

    // add a new Post to the current Dialogue
    pub fn post_add(
        &mut self,
        author: &str,
        timestamp: u64,
        text: &str,
        attach_url: Option<&str>,
    ) -> Result<(), Error> {
        match &mut self.dialogue {
            Some(ref mut dialogue) => {
                dialogue
                    .post_add(author, timestamp, text, attach_url)
                    .unwrap();
                match self.dialogue_save() {
                    Ok(_) => log::info!("Dialogue saved"),
                    Err(e) => log::warn!("Failed to save Dialogue: {e}"),
                }
                self.redraw().expect("failed chat ui redraw");
            }
            None => log::warn!("no Dialogue available to add Post"),
        }
        Ok(())
    }

    // delete a Post from the current Dialogue
    pub fn post_del(&self, _key: u32) -> Result<(), Error> {
        log::warn!("not implemented");
        Err(Error::new(ErrorKind::Other, "not implemented"))
    }

    // get a Post from the current Dialogue
    pub fn post_find(&self, author: &str, timestamp: u64) -> Option<usize> {
        match &self.dialogue {
            Some(dialogue) => dialogue.post_find(author, timestamp),
            None => None,
        }
    }

    // get a Post from the current Dialogue
    pub fn post_get(&self, _key: u32) -> Result<Post, Error> {
        log::warn!("not implemented");
        Err(Error::new(ErrorKind::Other, "not implemented"))
    }

    // set various status flags on a Post in the current Dialogue
    pub fn post_flag(&self, _key: u32) -> Result<(), Error> {
        log::warn!("not implemented");
        Err(Error::new(ErrorKind::Other, "not implemented"))
    }

    // set the text displayed on each of the Precursor Fn buttons
    pub fn ui_button(
        &self,
        _f1: Option<&str>,
        _f2: Option<&str>,
        _f3: Option<&str>,
        _f4: Option<&str>,
    ) -> Result<(), Error> {
        log::warn!("not implemented");
        Err(Error::new(ErrorKind::Other, "not implemented"))
    }

    // request the Chat object to display a menu with options to the user
    pub fn ui_menu(&self, _options: Vec<&str>) -> Result<Vec<u32>, Error> {
        log::warn!("not implemented");
        Err(Error::new(ErrorKind::Other, "not implemented"))
    }

    fn app_cid(&self) -> Option<CID> {
        self.app_cid
    }

    // send a xous scalar message with an Event to the Chat App cid/opcode
    pub fn event(&self, event: Event) {
        log::info!("Event {:?}", event);
        match (self.app_cid, self.opcode_event) {
            (Some(cid), Some(opcode)) => match xous::send_message(
                cid,
                xous::Message::new_scalar(opcode as usize, event as usize, 0, 0, 0),
            ) {
                Ok(_) => log::info!("sent event msg"),
                Err(e) => log::warn!("failed to send event msg: {:?}", e),
            },
            _ => log::warn!("missing cid or event opcode"),
        }
    }

    fn bubble(&self, post: &Post, author: &Author, baseline: i16) -> TextView {
        use std::fmt::Write;
        let mut bubble_tv = if author.flag_is(AuthorFlag::Right) {
            TextView::new(
                self.canvas,
                TextBounds::GrowableFromBr(
                    Point::new(self.screensize.x - self.margin.x, baseline),
                    self.bubble_width,
                ),
            )
        } else {
            TextView::new(
                self.canvas,
                TextBounds::GrowableFromBl(Point::new(self.margin.x, baseline), self.bubble_width),
            )
        };
        bubble_tv.border_width = 1;
        bubble_tv.draw_border = true;
        bubble_tv.clear_area = true;
        bubble_tv.rounded_border = Some(self.bubble_radius);
        bubble_tv.style = GlyphStyle::Regular;
        bubble_tv.margin = self.bubble_margin;
        bubble_tv.ellipsis = false;
        bubble_tv.insertion = None;
        write!(bubble_tv.text, "{}", post.text()).expect("couldn't write history text to TextView");
        bubble_tv
    }

    fn clear_area(&self) {
        self.gam
            .draw_rectangle(
                self.canvas,
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
            .expect("can't clear canvas area");
    }

    pub(crate) fn raise_menu(&mut self) {
        // self.title_dirty = true;
        self.gam
            .raise_menu(&self.app_menu)
            .expect("couldn't raise our submenu");
        log::info!("raised menu");
    }

    pub(crate) fn redraw(&mut self) -> Result<(), xous::Error> {
        self.clear_area();

        // this defines the bottom border of the text bubbles as they stack up wards
        let mut bubble_baseline = self.screensize.y - self.margin.y;
        if let Some(dialogue) = &self.dialogue {
            for post in dialogue.posts().rev() {
                if let Some(author) = dialogue.author(post.author_id()) {
                    let mut bubble_tv = self.bubble(post, author, bubble_baseline);
                    self.gam
                        .post_textview(&mut bubble_tv)
                        .expect("couldn't render bubble textview");

                    if let Some(bounds) = bubble_tv.bounds_computed {
                        // we only subtract 1x of the margin because the bounds were computed from a "bottom right" that already counted
                        // the margin once.
                        bubble_baseline -=
                            (bounds.br.y - bounds.tl.y) + self.bubble_space + self.bubble_margin.y;
                        if bubble_baseline <= 0 {
                            // don't draw history that overflows the top of the screen
                            break;
                        }
                    } else {
                        break; // we get None on the bounds computed if the text view fell off the top of the screen
                    }
                } else {
                    log::warn!(
                        "Post missing Author: {:?}:{:?} {:?}",
                        self.pddb_dict,
                        self.pddb_key,
                        post
                    );
                }
            }
        }
        log::trace!("chat app redraw##");
        self.gam.redraw().expect("couldn't redraw screen");
        Ok(())
    }
}
