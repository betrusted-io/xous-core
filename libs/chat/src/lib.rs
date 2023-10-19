pub mod api;
pub mod dialogue;
pub mod icontray;
pub mod ui;

pub use api::*;
pub use ui::BUSY_ANIMATION_RATE_MS;
use gam::MenuItem;
use num_traits::FromPrimitive;
use std::convert::TryInto;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::{Arc, atomic::AtomicBool, atomic::Ordering};

use xous::{Error, CID, SID};
use xous_ipc::Buffer;

pub struct Chat {
    cid: CID,
}

impl Chat {
    /// Create a new Chat UI
    ///
    /// # Arguments
    ///
    /// * `app_name` - registered with GAM
    /// * `app_menu` - with menu items handled by the Chat App rather than the Chat UI
    /// * `app_cid` - to accept messages from the Chat UI (see below)
    /// * `post_opcode` - to handle a `MemoryMessage` containing a new outbound user Post
    /// * `event_opcode` - to handle `ScalarMessage` representing a UI Event, such as F1 click, Left click, Top Post, etc.
    /// * `rawkeys_opcode` - to handle a raw-keystroke.
    ///
    pub fn new(
        app_name: &'static str,
        app_menu: &'static str,
        app_cid: Option<CID>,
        opcode_post: Option<usize>,
        opcode_event: Option<usize>,
        opcode_rawkeys: Option<usize>,
    ) -> Self {
        let chat_sid = xous::create_server().unwrap();
        let chat_cid = xous::connect(chat_sid).unwrap();

        let busy_bumper = xous::create_server().unwrap();
        let busy_bumper_cid = xous::connect(busy_bumper).unwrap();

        log::info!("starting idle animation runner");
        let run_busy_animation = Arc::new(AtomicBool::new(false));
        thread::spawn({
            let run_busy_animation = run_busy_animation.clone();
            move || {
                busy_animator(busy_bumper, busy_bumper_cid, chat_cid, run_busy_animation);
            }
        });

        log::info!("Starting chat UI server",);
        thread::spawn({
            move || {
                server(
                    chat_sid,
                    app_name,
                    app_menu,
                    app_cid,
                    opcode_post,
                    opcode_event,
                    opcode_rawkeys,
                    run_busy_animation,
                    busy_bumper_cid,
                );
            }
        });

        Chat { cid: chat_cid }
    }

    /// Return the Chat App CID
    ///
    /// This cid allows the Chat App to contact this Chat UI server
    ///
    pub fn cid(&self) -> CID {
        self.cid
    }

    /// Create an offline/read-only Chat UI over and existing Dialogue in pddb
    ///
    /// # Arguments
    ///
    /// * `pddb_dict` - the pddb dict holding all Dialogues for this Chat App
    /// * `pddb_key` - the pddb key holding a Dialogue
    ///
    pub fn read_only(pddb_dict: &str, pddb_key: Option<&str>) -> Self {
        let chat = Chat::new("_Chat Read_", "unused", None, None, None, None);
        chat.dialogue_set(pddb_dict, pddb_key).unwrap();
        chat
    }

    /// Set the current Dialogue
    ///
    /// # Arguments
    ///
    /// * `pddb_dict` - the pddb dict holding all Dialogues for this Chat App
    /// * `pddb_key` - the pddb key holding a Dialogue
    ///
    pub fn dialogue_set(&self, pddb_dict: &str, pddb_key: Option<&str>) -> Result<(), Error> {
        let dialogue = api::Dialogue {
            dict: xous_ipc::String::from_str(pddb_dict),
            key: pddb_key.map(|key| xous_ipc::String::from_str(key)),
        };
        match Buffer::into_buf(dialogue) {
            Ok(buf) => buf.send(self.cid, ChatOp::DialogueSet as u32).map(|_| ()),
            Err(_) => Err(xous::Error::InternalError),
        }
    }

    /// Show some user help
    ///
    pub fn help(&self) {
        xous::send_message(
            self.cid,
            xous::Message::new_scalar(ChatOp::Help as usize, 0, 0, 0, 0),
        )
        .map(|_| ())
        .expect("failed to get help");
    }

    /// Add a new MenuItem to the App menu
    ///
    /// # Arguments
    ///
    /// * `item` - an item action not handled by the Chat UI
    ///
    pub fn menu_add(&self, item: MenuItem) -> Result<(), Error> {
        match Buffer::into_buf(item) {
            Ok(buf) => buf.send(self.cid, ChatOp::MenuAdd as u32).map(|_| ()),
            Err(_) => Err(xous::Error::InternalError),
        }
    }

    /// Add a new Post to the current Dialogue
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
        &self,
        author: &str,
        timestamp: u64,
        text: &str,
        attach_url: Option<&str>,
    ) -> Result<(), Error> {
        let mut post = api::Post {
            dialogue_id: xous_ipc::String::new(),
            author: xous_ipc::String::new(),
            timestamp: timestamp,
            text: xous_ipc::String::new(),
            attach_url: match attach_url {
                Some(url) => Some(xous_ipc::String::from_str(url)),
                None => None,
            },
        };
        post.author.append(author).unwrap();
        post.text.append(text).unwrap();
        match Buffer::into_buf(post) {
            Ok(buf) => buf.send(self.cid, ChatOp::PostAdd as u32).map(|_| ()),
            Err(_) => Err(xous::Error::InternalError),
        }
    }

    /// Delete a Post from the current Dialogue
    ///
    /// # Arguments
    ///
    /// * `index` - the index of the Post to delete.
    ///
    pub fn post_del(&self, index: usize) -> Result<(), Error> {
        xous::send_message(
            self.cid,
            xous::Message::new_scalar(ChatOp::PostDel as usize, index, 0, 0, 0),
        )
        .map(|_| ())
        .expect("failed to delete Pose {index}");
        Ok(())
    }

    /// Returns Some(index) of a matching Post by Author and Timestamp, or None
    ///
    /// # Arguments
    ///
    /// * `timestamp` - the Post timestamp criteria
    /// * `author` - the Post Author criteria
    ///
    /// Error if unable to send the msg to the Chat UI server
    ///
    pub fn post_find(&self, author: &str, timestamp: u64) -> Result<Option<usize>, Error> {
        let mut find = Find {
            author: xous_ipc::String::new(),
            timestamp: timestamp,
            key: None,
        };
        find.author.append(author).unwrap();
        match Buffer::into_buf(find) {
            Ok(mut buf) => match buf.lend_mut(self.cid, ChatOp::PostFind as u32) {
                Ok(..) => {
                    find = buf.to_original::<api::Find, _>().unwrap();
                    Ok(find.key)
                }
                Err(_) => Err(xous::Error::InternalError),
            },
            Err(_) => Err(xous::Error::InternalError),
        }
    }

    /// Set various status flags on a Post in the current Dialogue
    ///
    /// TODO: not implemented
    ///
    pub fn post_flag(&self, _key: &str) -> Result<(), Error> {
        log::warn!("not implemented");
        Err(xous::Error::InternalError)
    }

    /// Redraw our Chat UI.
    ///
    pub fn redraw(&self) {
        xous::send_message(
            self.cid,
            xous::Message::new_scalar(ChatOp::GamRedraw as usize, 0, 0, 0, 0),
        )
        .map(|_| ())
        .expect("failed to Redraw Chat UI");
    }

    /// Run or stop the busy/waiting animation.
    ///
    /// # Arguments
    ///
    /// `msg` - If `Some()`, updates the status bar to show the `msg` and start the animation running
    /// If `None`, restores the last status message before being busy and stops the animation.
    pub fn set_busy_state(&self, msg: Option<String>) -> Result<(), Error> {
        let bm = BusyMessage {
            busy_msg: match msg {
                None => None,
                Some(m) => {
                    Some(
                        xous_ipc::String::from_str(&m)
                    )
                }
            }
        };
        match Buffer::into_buf(bm) {
            Ok(buf) => match buf.send(self.cid, ChatOp::SetBusyAnimationState as u32) {
                Ok(..) => {
                    Ok(())
                }
                Err(_) => Err(xous::Error::InternalError),
            },
            Err(_) => Err(xous::Error::InternalError),
        }
    }
}

/// Helper server that pumps the busy animation state until instructed to stop.
///
/// # Arguments
///
/// * `busy_bumper` - the server ID to use for the helper server
/// * `busy_bumper_cid` - the corresponding connection ID
/// * `chat_cid` - the CID to the main chat loop, used to initiate redraw events as necessary
/// * `run_busy_animation` - a shared `AtomicBool` which, when `true`, causes the loop to reschedule itself to run.
pub fn busy_animator(
    busy_bumper: SID,
    busy_bumper_cid : CID,
    chat_cid: CID,
    run_busy_animation: Arc<AtomicBool>) {
        let tt = ticktimer_server::Ticktimer::new().unwrap();
        loop {
            let msg = xous::receive_message(busy_bumper).unwrap();
            match FromPrimitive::from_usize(msg.body.id()) {
                Some(BusyAnimOp::Start) => {
                    tt.sleep_ms(crate::BUSY_ANIMATION_RATE_MS).unwrap();
                    xous::try_send_message(busy_bumper_cid,
                        xous::Message::new_scalar(
                            BusyAnimOp::Pump as usize, 0, 0, 0, 0)
                    ).ok();
                }
                Some(BusyAnimOp::Pump) => {
                    if run_busy_animation.load(Ordering::SeqCst) {
                        xous::try_send_message(chat_cid,
                            xous::Message::new_scalar(
                                ChatOp::UpdateBusy as usize, 0, 0, 0, 0)
                        ).ok();
                        tt.sleep_ms(crate::BUSY_ANIMATION_RATE_MS).unwrap();
                        xous::try_send_message(busy_bumper_cid,
                            xous::Message::new_scalar(
                                BusyAnimOp::Pump as usize, 0, 0, 0, 0)
                        ).ok();
                    }
                }
                _ => {
                    log::warn!("Unexpected message: {:?}", msg);
                }
            }
        }
}

/// The Chat UI server a manages a Chat UI to read a display and navigate a
/// series of Posts in a Dialogue stored in the pddb - and to Author a new
/// Post.
///
/// # Arguments
///
/// * `app_name` - registered with GAM
/// * `app_menu` - with menu items handled by the Chat App rather than the Chat UI
/// * `app_cid` - to accept messages from the Chat UI (see below)
/// * `post_opcode` - to handle a `MemoryMessage` containing a new outbound user Post
/// * `event_opcode` - to handle `ScalarMessage` representing a UI Event, such as F1 click, Left click, Top Post, etc.
/// * `rawkeys_opcode` - to handle a raw-keystroke.
///
pub fn server(
    sid: SID,
    app_name: &'static str,
    app_menu: &'static str,
    app_cid: Option<CID>,
    opcode_post: Option<usize>,
    opcode_event: Option<usize>,
    opcode_rawkeys: Option<usize>,
    run_busy_animation: Arc<AtomicBool>,
    busy_bumper_cid: CID,
) -> ! {
    //log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let mut ui = ui::Ui::new(sid, app_name, app_menu, app_cid, opcode_event);

    let mut allow_redraw = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        if ui.is_busy() {
            if !run_busy_animation.swap(true, Ordering::SeqCst) {
                // only send off the Pump request on the transition from false->true; this causes the machine to run
                xous::try_send_message(busy_bumper_cid,
                    xous::Message::new_scalar(
                        BusyAnimOp::Pump as usize, 0, 0, 0, 0)
                ).ok();
            }
        } else {
            // make sure the animation stops running. Not sure if this is the most efficient way to handle this,
            // but I think an atomic bool set is just a couple dozen CPU cycles...?
            run_busy_animation.store(false, Ordering::SeqCst);
        }
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ChatOp::UpdateBusy) => {
                ui.redraw_busy().expect("CHAT couldn't redraw");
            }
            Some(ChatOp::SetBusyAnimationState) => {
                let buffer = unsafe {
                    Buffer::from_memory_message(msg.body.memory_message().unwrap())
                };
                let s = buffer.to_original::<BusyMessage, _>().unwrap();
                match s.busy_msg {
                    Some(msg) => {
                        ui.set_busy(msg.as_str().unwrap());
                    }
                    None => {
                        ui.clear_busy();
                    }
                }
            }
            Some(ChatOp::DialogueSave) => {
                log::info!("ChatOp::DialogueSave");
                ui.dialogue_save().expect("failed to save Dialogue");
                ui.dialogue_read().expect("failed to read Dialogue");
                if allow_redraw {
                    ui.redraw().expect("CHAT couldn't redraw");
                }
            }
            Some(ChatOp::DialogueSet) => {
                log::info!("ChatOp::DialogueSet");
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let dialogue = buffer.to_original::<Dialogue, _>().unwrap();
                let dialogue_key = match dialogue.key {
                    Some(key) => Some(key.to_string()),
                    None => None,
                };
                ui.dialogue_set(dialogue.dict.as_str().unwrap(), dialogue_key.as_deref());
            }
            Some(ChatOp::GamChangeFocus) => {
                log::info!("ChatOp::GamChangeFocus");
                xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                    let new_state = gam::FocusState::convert_focus_change(new_state_code);
                    match new_state {
                        gam::FocusState::Background => {
                            allow_redraw = false;
                        }
                        gam::FocusState::Foreground => {
                            allow_redraw = true;
                            ui.event(Event::Focus);
                        }
                    }
                })
            }
            Some(ChatOp::GamLine) => {
                log::info!("got ChatOp::GamLine");
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<xous_ipc::String<4000>, _>().unwrap();
                match s.as_str() {
                    "\u{0011}" => {}
                    "\u{0012}" => {}
                    "\u{0013}" => {}
                    "\u{0014}" => {}
                    "↑" => {}
                    "↓" => {}
                    "←" => {}
                    "→" => {}
                    _ => {
                        drop(buffer);
                        if let Some(cid) = app_cid {
                            if let Some(opcode) = opcode_post {
                                log::info!("Forwarding msg to Chat App: {:?}", msg);
                                msg.forward(cid, opcode).expect("failed to fwd msg");
                            }
                        }
                    }
                }
            }
            Some(ChatOp::GamRawkeys) => {
                log::info!("got ChatOp::GamRawkeys");
                xous::msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                    log::info!("got Chat UI RawKey :{}:{}:{}:{}:", k1, k2, k3, k4);
                    match core::char::from_u32(k1 as u32).unwrap_or('\u{0000}') {
                        F1 => {
                            log::info!("click F1 : pull request welcome!");
                            ui.event(Event::F1);
                        }
                        F2 => {
                            log::info!("click F2 : pull request welcome!");
                            ui.event(Event::F2);
                        }
                        F3 => {
                            log::info!("click F3 : pull request welcome!");
                            ui.event(Event::F3);
                        }
                        F4 => {
                            log::info!("click F4 : pull request welcome!");
                            ui.event(Event::F4);
                        }
                        '↑' => {
                            log::info!("click ↑ : previous post");
                            ui.set_menu_mode(true); // ← & → activate menus
                            ui.post_select(POST_SELECTED_PREV);
                            ui.redraw().expect("failed to redraw chat");
                            ui.event(Event::Up);
                        }
                        '↓' => {
                            log::info!("click ↓ : next post");
                            ui.post_select(POST_SELECTED_NEXT);
                            ui.redraw().expect("failed to redraw chat");
                            ui.event(Event::Down);
                        }
                        '←' => {
                            log::info!("click ← : raise app menu");
                            if ui.get_menu_mode() {
                                ui.raise_app_menu();
                            }
                            ui.event(Event::Left);
                        }
                        '→' => {
                            log::info!("click → : raise msg menu : pull request welcome!");
                            if ui.get_menu_mode() {
                                ui.raise_msg_menu();
                            }
                            ui.event(Event::Right);
                        }
                        _ => {
                            ui.set_menu_mode(false); // ← & → move input cursor
                        }
                    }
                });
                if let Some(cid) = app_cid {
                    if let Some(opcode) = opcode_rawkeys {
                        log::info!("Forwarding msg to Chat App: {:?}", msg);
                        msg.forward(cid, opcode).expect("failed to fwd rawkey");
                    }
                }
            }
            Some(ChatOp::Help) => {
                log::info!("ChatOp::Help");
                ui.help();
            }
            Some(ChatOp::GamRedraw) => {
                log::info!("ChatOp::GamRedraw");
                if allow_redraw {
                    ui.redraw().expect("CHAT couldn't redraw");
                }
            }
            Some(ChatOp::PostAdd) => {
                log::info!("ChatOp::PostAdd");
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                match buffer.to_original::<api::Post, _>() {
                    Ok(post) => ui
                        .post_add(
                            post.dialogue_id.as_str().unwrap(),
                            post.author.as_str().unwrap(),
                            post.timestamp,
                            post.text.as_str().unwrap(),
                            None, // TODO implement
                        )
                        .unwrap(),
                    Err(e) => log::warn!("failed to deserialize Post: {:?}", e),
                }
            }
            Some(ChatOp::PostDel) => {
                xous::msg_scalar_unpack!(msg, index, _, _, _, {
                    log::info!("ChatOp::PostDel {index}");
                    ui.post_del(index).expect("failed to delete post {index}");
                });
            }
            Some(ChatOp::PostFind) => {
                log::info!("ChatOp::PostAdd");
                let mut buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                if let Ok(mut find) = buffer.to_original::<Find, _>() {
                    find.key = ui.post_find(find.author.as_str().unwrap(), find.timestamp);
                    buffer.replace(find).expect("couldn't serialize return");
                } else {
                    log::warn!("failed to serialize Find");
                }
            }
            Some(ChatOp::PostFlag) => {
                log::warn!("ChatOp::PostFlag not implemented");
            }
            Some(ChatOp::MenuAdd) => {
                log::warn!("ChatOp::MenuAdd not implemented");
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                if let Ok(menu_item) = buffer.to_original::<MenuItem, _>() {
                    ui.menu_add(menu_item);
                } else {
                    log::warn!("failed to deserialize MenuItem");
                }
            }
            Some(ChatOp::Quit) => {
                log::error!("got Quit");
                break;
            }
            _ => log::warn!("got unknown message"),
        }
        log::trace!("reached bottom of main loop");
    }
    // clean up our program
    log::error!("main loop exit, destroying servers");
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .try_into()
        .unwrap()
}
