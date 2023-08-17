pub mod api;
pub mod cmd;
pub mod dialogue;
pub mod ui;

pub use api::*;
use gam::MenuItem;
use num_traits::FromPrimitive;
use std::convert::TryInto;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use xous::{Error, MessageEnvelope, CID, SID};
use xous_ipc::Buffer;

pub struct Chat {
    cid: CID,
}

impl Chat {
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
                );
            }
        });

        Chat { cid: chat_cid }
    }

    pub fn cid(&self) -> CID {
        self.cid
    }

    pub fn read_only(pddb_dict: &str, pddb_key: Option<&str>) -> Self {
        let chat = Chat::new("_Chat Read_", "unused", None, None, None, None);
        chat.dialogue_set(pddb_dict, pddb_key).unwrap();
        chat
    }

    // set the current Dialogue
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

    pub fn menu_add(&self, item: MenuItem) -> Result<(), Error> {
        match Buffer::into_buf(item) {
            Ok(buf) => buf.send(self.cid, ChatOp::MenuAdd as u32).map(|_| ()),
            Err(_) => Err(xous::Error::InternalError),
        }
    }

    // add a new Post to the current Dialogue
    pub fn post_add(
        &self,
        author: &str,
        timestamp: u64,
        text: &str,
        attach_url: Option<&str>,
    ) -> Result<(), Error> {
        let mut post = api::Post {
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

    // delete a Post from the current Dialogue
    pub fn post_del(&self, _key: u32) -> Result<(), Error> {
        log::warn!("not implemented");
        Err(xous::Error::InternalError)
    }

    // get a Post from the current Dialogue
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

    // set various status flags on a Post in the current Dialogue
    pub fn post_flag(&self, _key: &str) -> Result<(), Error> {
        log::warn!("not implemented");
        Err(xous::Error::InternalError)
    }

    pub fn redraw(&self) {
        xous::send_message(
            self.cid,
            xous::Message::new_scalar(ChatOp::GamRedraw as usize, 0, 0, 0, 0),
        )
        .map(|_| ())
        .expect("failed to Redraw Chat UI");
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
        Err(xous::Error::InternalError)
    }

    // request the Chat object to display a menu with options to the user
    pub fn ui_menu(&self, _options: Vec<&str>) -> Result<(), Error> {
        log::warn!("not implemented");
        Err(xous::Error::InternalError)
    }
}

pub fn server(
    sid: SID,
    app_name: &'static str,
    app_menu: &'static str,
    app_cid: Option<CID>,
    opcode_post: Option<usize>,
    opcode_event: Option<usize>,
    opcode_rawkeys: Option<usize>,
) -> ! {
    //log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let mut ui = ui::Ui::new(sid, app_name, app_menu, app_cid, opcode_event);

    let mut allow_redraw = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
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
                if let Some(cid) = app_cid {
                    if let Some(opcode) = opcode_post {
                        log::info!("Forwarding msg to Chat App: {:?}", msg);
                        msg.forward(cid, opcode).expect("failed to fwd msg");
                    }
                }
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
                match buffer.to_original::<Post, _>() {
                    Ok(post) => ui
                        .post_add(
                            post.author.as_str().unwrap(),
                            post.timestamp,
                            post.text.as_str().unwrap(),
                            None, // TODO implement
                        )
                        .unwrap(),
                    Err(e) => log::warn!("failed to deserialize Post: {:?}", e),
                }
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
            Some(ChatOp::UiButton) => {
                log::warn!("ChatOp::UiButton not implemented");
            }
            Some(ChatOp::UiMenu) => {
                log::warn!("ChatOp::UiMenu not implemented");
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
