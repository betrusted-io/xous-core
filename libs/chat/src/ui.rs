use super::*;
//use crate::{ChatOp, Dialogue, Event, Post, CHAT_SERVER_NAME};
use crate::icontray::Icontray;
use dialogue::{post::Post, Dialogue};
use gam::{menu_matic, MenuMatic, UxRegistration};
use graphics_server::api::GlyphStyle;
use graphics_server::{DrawStyle, Gid, PixelColor, Point, Rectangle, TextBounds, TextView};
use locales::t;
use modals::Modals;
use rkyv::de::deserializers::AllocDeserializer;
use rkyv::ser::serializers::WriteSerializer;
use rkyv::ser::Serializer;
use rkyv::Deserialize;
use std::cmp::min;
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

    // variables regarding the posts currently onscreen
    // the selected post is hilighted onscreen and the focus of the msg menu F4
    post_selected: Option<usize>,
    // the anchor post is drawn first, at the top or bottom of the screen
    post_anchor: Option<usize>,
    // layout post bubbles on the screen from top-down or bottom-up
    post_topdown: bool,

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
                    crate::icontray::SERVER_NAME_ICONTRAY,
                )),
                listener: sid.to_array(), // note disclosure of our SID to the GAM -- the secret is now shared with the GAM!
                redraw_id: ChatOp::GamRedraw as u32,
                gotinput_id: Some(ChatOp::GamLine as u32),
                audioframe_id: None,
                rawkeys_id: Some(ChatOp::GamRawkeys as u32),
                focuschange_id: Some(ChatOp::GamChangeFocus as u32),
            })
            .expect("couldn't register Ux context for chat")
            .unwrap();
        gam.toggle_menu_mode(token)
            .expect("couldnt't toggle menu mode");
        let xns = XousNames::new().unwrap();
        let modals = Modals::new(&xns).unwrap();
        let canvas = gam
            .request_content_canvas(token)
            .expect("couldn't get content canvas");
        let screensize = gam
            .get_canvas_bounds(canvas)
            .expect("couldn't get dimensions of content canvas");
        let _icontray = Icontray::new(
            Some(xous::connect(sid).unwrap()),
            [app_name, "F2", "F3", "F4"],
        );

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
            post_selected: None,
            post_anchor: None,
            post_topdown: false,
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


    /// Read the current Dialogue from pddb
    ///
    pub fn dialogue_read(&mut self) -> Result<(), Error> {
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
                                    Ok(dialogue) => {
                                        // show most recent posts onscreen
                                        self.post_selected = dialogue.post_last();
                                        self.post_anchor = self.post_selected;
                                        self.post_topdown = false;
                                        Some(dialogue)
                                    }
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


    /// Save the current Dialogue to pddb
    ///
    pub fn dialogue_save(&self) -> Result<(), Error> {
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

    /// Set the current Dialogue
    ///
    /// # Arguments
    ///
    /// * `pddb_dict` - the pddb dict holding all Dialogues for this Chat App
    /// * `pddb_key` - the pddb key holding a Dialogue
    ///
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

    /// Present a Modal to select Dialogue from pddb
    ///
    /// typically called in offline mode
    ///
    /// TODO move non-dialogue keys elsewhere
    ///
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

    /// Add a new MenuItem to the App menu
    ///
    /// # Arguments
    ///
    /// * `item` - an item action not handled by the Chat UI
    ///
    pub fn menu_add(&self, item: MenuItem) {
        self.menu_mgr.add_item(item);
    }

    /// Add a new Post to the current Dialogue
    ///
    /// TODO posts are truncated to mask a critical error on some posts (long with replies????)
    ///
    /// note: posts are sorted by timestamp, so:
    /// - `post_add` at beginning or end is fast (middle triggers a binary partition)
    /// - if adding multiple posts then add oldest/newest last!
    ///
    /// # Arguments
    ///
    /// * `author` - the name of the Author of the Post
    /// * `timestamp` - the timestamp of the Post
    /// * `text` - the text content of the Post
    /// * `attach_url` - a url of an attachment (image for example)
    ///
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
            }
            None => log::warn!("no Dialogue available to add Post"),
        }
        Ok(())
    }

    /// Delete a Post from the current Dialogue
    pub fn post_del(&self, _key: u32) -> Result<(), Error> {

    ///
    /// TODO: implement post_delete()
    ///
        log::warn!("not implemented");
        Err(Error::new(ErrorKind::Other, "not implemented"))
    }

    /// Returns Some(index) of a matching Post by Author and Timestamp, or None
    ///
    /// # Arguments
    ///
    /// * `timestamp` - the Post timestamp criteria
    /// * `author` - the Post Author criteria
    ///
    pub fn post_find(&self, author: &str, timestamp: u64) -> Option<usize> {
        match &self.dialogue {
            Some(dialogue) => dialogue.post_find(author, timestamp),
            None => None,
        }
    }

    /// Return Some<Post> from the current Dialogue, or None
    ///
    /// # Arguments
    ///
    /// * `index` - index of the Post to retrieve
    ///
    pub fn post_get(&self, index: usize) -> Option<&Post> {
        match &self.dialogue {
            Some(dialogue) => dialogue.post_get(index),
            None => None,
        }
    }

    /// Set various status flags on a Post in the current Dialogue
    ///
    /// TODO: not implemented
    ///
    pub fn post_flag(&self, _key: u32) -> Result<(), Error> {
        log::warn!("not implemented");
        Err(Error::new(ErrorKind::Other, "not implemented"))
    }

    /// Set the Selected Post to an arbitrary index
    ///
    /// # Arguments
    ///
    /// * `index` - POST_SELECT_NEXT or POST_SELECT_PREV or an arbitraty index
    pub fn post_select(&mut self, index: usize) {
        self.post_selected = match &self.dialogue {
            Some(dialogue) => {
                match dialogue.post_last() {
                    Some(last_post) => {
                        match (index, self.post_selected) {
                            (POST_SELECTED_NEXT, Some(selected)) => {
                                if selected >= last_post {
                                    self.event(Event::Bottom);
                                    Some(last_post)
                                } else {
                                    Some(selected + 1)
                                }
                            },
                            (POST_SELECTED_PREV, Some(selected)) => {
                                if selected == 0 {
                                    self.event(Event::Top);
                                    Some(selected)
                                } else {
                                    Some(selected - 1)
                                }
                            },
                            (index, _) => Some(min(index, last_post)), // arbitrary post
                        }
                    },
                    None => None,
                }
            }
            None => None,
        }
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









    /// Send a xous scalar message with an Event to the Chat App cid/opcode
    ///
    /// # Arguments
    ///
    /// * `event` - the type of event to send
    ///
    /// Error when `app_cid` == None or `opcode_event` == None
    ///
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

    /// Return a TextView bubble representing a Dialogue Post 
    ///
    /// # Arguments
    ///
    /// * `post` - the post to represent in a TextView bubble
    /// * `dialogue` - containing the Post for context info
    /// * `hilite` - hilite this Post on the screen (thicker border)
    /// * `anchor_y` - the vertical position on screen to draw TextView bubble
    ///
    fn bubble(&self, post: &Post, dialogue: &Dialogue, hilite: bool, anchor_y: i16) -> TextView {
        // set alignment of bubble left/right
        let mut align_right = false;
        let mut anchor_x = self.margin.x; // default to align left
        if let Some(author) = dialogue.author(post.author_id()) {
            if author.flag_is(AuthorFlag::Right) {
                // align right
                align_right = true;
                anchor_x = self.screensize.x - self.margin.x;
            }
        }

        // set the text bounds of the bubble and the growth direction
        let anchor = Point::new(anchor_x, anchor_y);
        let width = self.bubble_width;
        let text_bounds = match (self.post_topdown, align_right) {
            (true, true) => TextBounds::GrowableFromTr(anchor, width),
            (true, false) => TextBounds::GrowableFromTl(anchor, width),
            (false, true) => TextBounds::GrowableFromBr(anchor, width),
            (false, false) => TextBounds::GrowableFromBl(anchor, width),
        };

        // create the bubble with the anchor and a growable direction
        use std::fmt::Write;
        let mut bubble_tv = TextView::new(self.canvas, text_bounds);
        if hilite {
            bubble_tv.border_width = 3;
        } else {
            bubble_tv.border_width = 1;
        }
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

    // Clear the screen area
    //
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

    /// Show the App Menu (← key)
    ///
    pub(crate) fn raise_app_menu(&mut self) {
        self.gam
            .raise_menu(&self.app_menu)
            .expect("couldn't raise our submenu");
        log::info!("raised app menu");
    }

    /// Redraw posts on the screen.
    ///
    /// Up to three attempts are made to layout the Posts:
    /// * ensuring the selected post is fully visible, and
    /// * best use of the screen is achieved
    ///
    pub(crate) fn redraw(&mut self) -> Result<(), xous::Error> {
        if self.dialogue.is_some() {
            let mut attempt = 0;
            while !self.layout().unwrap_or(true) && attempt < 3
            {
                attempt += 1;
            }
        } else {
            self.clear_area(); // no dialogue so clear screen
        }
        log::trace!("chat app redraw##");
        self.gam.redraw().expect("couldn't redraw screen");
        Ok(())
    }

    /// Layout the post bubbles on the screen.
    ///
    /// The layout proceeds from top-down or bottom-up (starting with the
    /// `post_anchor`), drawing a bubble for each Post, until the available space
    /// is exhausted.
    /// * If the `post_selected` is fully displayed, then `Ok(true)` is Returned.
    /// * If the `post_selected` is NOT fully displayed, then the `post_anchor`
    /// is set as the `post_anchor`, and `Ok(false)` is Returned - signalling that
    /// a re-layout is in order.
    /// * If the first/last Post is fully displayed, then the `post_topdown` is
    /// toggled, the `post_anchor` is set to the first/last Post, and `Ok(false)`
    /// is Returned - signalling that a re-layout is in order.
    /// 
    /// Error if there if Dialogue is None
    ///
    fn layout(&mut self) -> Result<bool, Error> {
        self.clear_area();
        match (&self.dialogue, &self.post_anchor ) {
            (Some(dialogue), Some(post_anchor)) => {
                log::info!("redrawing dialogue: {}", dialogue.title);
                let mut post_selected_visible = false;
                let mut bubble_count = 0;

                // initialise the first post index AND the vertical position on the screen
                let mut post_index = *post_anchor;
                let mut anchor_y = match self.post_topdown {
                    true => 0,
                    false => self.screensize.y - self.margin.y,
                };

                // fill the screen with post bubbles from top-down or bottom-up
                let mut post_is_fully_visible = true;
                let mut is_selected = false;
                while post_is_fully_visible {
                    log::trace!("redrawing post: {post_index}");
                    match (dialogue.post_get(post_index), &self.post_selected){
                        (Some(post), Some(post_selected)) => {
                            is_selected = post_index == *post_selected;

                            // create a bubble and place on canvas
                            let mut bubble_tv = self.bubble(post, dialogue, is_selected, anchor_y);
                            self.gam
                                .post_textview(&mut bubble_tv)
                                .expect("couldn't render bubble textview");
                            bubble_count += 1;
                            post_is_fully_visible = !bubble_tv.overflow.unwrap_or(true);
                            if post_is_fully_visible {
                                // step to the next post AND the next vertical position on the screen
                                match bubble_tv.bounds_computed {
                                    Some(bounds) => match self.post_topdown {
                                        true => {
                                            if post_index >= dialogue.post_last().unwrap_or(usize::MAX) {
                                                log::info!("trigger a re-layout from bottom-up");
                                                self.post_topdown = false;
                                                self.post_anchor = dialogue.post_last();
                                                return Ok(false);
                                            }
                                            post_index += 1;
                                            anchor_y += (bounds.br.y - bounds.tl.y)
                                                + self.bubble_space
                                                + self.bubble_margin.y;
                                        }
                                        false => {
                                            if post_index == 0 { 
                                                log::info!("trigger a re-layout from top-down");
                                                self.post_topdown = true;
                                                self.post_anchor = Some(0);
                                                return Ok(false);
                                            }
                                            post_index -= 1;
                                            anchor_y -= (bounds.br.y - bounds.tl.y)
                                                + self.bubble_space
                                                + self.bubble_margin.y;
                                        }
                                    },
                                    None => {
                                        log::info!("bubble is offscreen so noop");
                                        post_is_fully_visible = false;
                                    }
                                };

                                // check if the selected post is fully visible
                                post_selected_visible = post_selected_visible || is_selected;
                            }
                        },
                        (None, _) => {
                            log::trace!("not enough post bubbles to fill the screen");
                            return Ok(true)
                        },
                        (_,_) => return Ok(true), // get me outa-here
                    }
                }
                if post_selected_visible || (bubble_count == 1 && is_selected){
                    Ok(true)
                } else {
                    log::info!("trigger a re-layout with selected post visible");
                    self.post_topdown = self.post_selected >= self.post_anchor;
                    self.post_anchor = self.post_selected;
                    Ok(false)
                }
            },
            (Some(_dialogue), None) => {
                log::info!("no posts to display");
                // TODO show dialogue info as a default?
                Ok(true)
            }, 
            (None, _) => Err(Error::new(ErrorKind::InvalidData, "missing dialogue")),
        }
    }
}
