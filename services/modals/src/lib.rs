#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use gam::*;
use xous::{CID, SID, send_message, Message};
use num_traits::{ToPrimitive, FromPrimitive};
use xous_ipc::Buffer;
use std::thread;

pub struct Modals {
    conn: CID,
    token: [u32; 4],
    tt: ticktimer_server::Ticktimer,
}
impl Modals {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_MODALS).expect("Can't connect to Modals server");
        let trng = trng::Trng::new(&xns).unwrap();
        let mut token = [0u32; 4];
        trng.fill_buf(&mut token).unwrap();
        Ok(Modals {
            conn,
            token,
            tt: ticktimer_server::Ticktimer::new().unwrap(),
        })
    }

    pub fn get_text(&self,
        prompt: &str,
        maybe_validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
        maybe_validator_op: Option<u32>,
    ) -> Result<TextEntryPayload, xous::Error> {
        match self.get_text_multi(prompt, maybe_validator, maybe_validator_op, 1, None) {
            Ok(res) => {
                return Ok(res.first())
            },
            Err(error) => return Err(error),
        }
    }

    pub fn get_text_multi(&self,
        prompt: &str,
        maybe_validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
        maybe_validator_op: Option<u32>,
        fields: u32,
        placeholders: Option<Vec<Option<String>>>,
    ) -> Result<TextEntryPayloads, xous::Error> {
        self.lock();
        let validator = if let Some(validator) = maybe_validator {
            // create a one-time use server
            let validator_server = xous::create_server().unwrap();
            thread::spawn({
                let vsid = validator_server.to_array();
                move || {
                    loop {
                        let mut msg = xous::receive_message(SID::from_array(vsid)).unwrap();
                        log::debug!("validator message: {:?}", msg);
                        match FromPrimitive::from_usize(msg.body.id()) {
                            Some(ValidationOp::Validate) => {
                                let mut buffer = unsafe{Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())};
                                let validation = buffer.to_original::<Validation, _>().unwrap();
                                let result = validator(validation.text, validation.opcode);
                                buffer.replace(result).expect("couldn't place validation result");
                            }
                            Some(ValidationOp::Quit) => {
                                // this is a blocking scalar, have to return a dummy value to unblock the caller
                                xous::return_scalar(msg.sender, 0).unwrap();
                                break;
                            }
                            _ => {
                                log::error!("received unknown message: {:?}", msg);
                            }
                        }
                    }
                }
            });
            Some(validator_server.to_array())
        } else {
            None
        };

        let mut final_placeholders: Option<[Option<xous_ipc::String<256>>; 10]> = None;

        match placeholders {
            Some(placeholders) => {
                let mut pl:[Option<xous_ipc::String<256>>; 10] = Default::default();
                 //final_placeholders = Some(Default::default());
                
                if fields != placeholders.len() as u32 {
                    log::warn!("can't have more fields than placeholders");
                    return Err(xous::Error::UnknownError);
                }
        
                for (index, placeholder) in placeholders.iter().enumerate() {
                    match placeholder {
                        Some(string) => {
                            pl[index] = Some(xous_ipc::String::from_str(&string))
                        },
                        None => pl[index] = None,
                    }
                }

                final_placeholders = Some(pl)
            }
            None => (),
        }

        let spec = ManagedPromptWithTextResponse {
            token: self.token,
            prompt: xous_ipc::String::from_str(prompt),
            validator,
            validator_op: maybe_validator_op.unwrap_or(0),
            fields: fields,
            placeholders: final_placeholders,
        };
        let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::PromptWithTextResponse.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        if let Some(server) = validator {
            let cid = xous::connect(SID::from_array(server)).unwrap();
            send_message(cid, Message::new_blocking_scalar(ValidationOp::Quit.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap()}; // must disconnect before destroy to avoid the CID from hanging out in our outbound table which is limited to 32 entries
            xous::destroy_server(SID::from_array(server)).expect("couldn't destroy temporary server");
        }
        match buf.to_original::<TextEntryPayloads, _>() {
            Ok(response) => Ok(response),
            _ => Err(xous::Error::InternalError)
        }
    }

    /// this blocks until the notification has been acknowledged.
    pub fn show_notification(&self, notification: &str) -> Result<(), xous::Error> {
        self.lock();
        let spec = ManagedNotification {
            token: self.token,
            message: xous_ipc::String::from_str(notification),
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::Notification.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        Ok(())
    }

    pub fn start_progress(&self, title: &str, start: u32, end: u32, current: u32) -> Result<(), xous::Error> {
        self.lock();
        let spec = ManagedProgress {
            token: self.token,
            title: xous_ipc::String::from_str(title),
            start_work: start,
            end_work: end,
            current_work: current,
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::StartProgress.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        Ok(())
    }

    /// note that this API is not atomically token-locked, so, someone could mess with the progress bar state
    /// but, progress updates are meant to be fast and frequent, and generally if a progress bar shows
    /// something whacky it's not going to affect a security outcome
    pub fn update_progress(&self, current: u32) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::UpdateProgress.to_usize().unwrap(), current as usize, 0, 0, 0)
        ).expect("couldn't update progress");
        Ok(())
    }

    /// close the progress bar, regardless of the current state
    pub fn finish_progress(&self) -> Result<(), xous::Error> {
        self.lock();
        send_message(self.conn,
            Message::new_scalar(Opcode::StopProgress.to_usize().unwrap(),
            self.token[0] as usize,
            self.token[1] as usize,
            self.token[2] as usize,
            self.token[3] as usize,
            )
        ).expect("couldn't stop progress");
        Ok(())
    }

    pub fn add_list_item(&self, item: &str) -> Result<(), xous::Error> {
        self.lock();
        let itemname = ManagedListItem {
            token: self.token,
            item: ItemName::new(item)
        };
        let buf = Buffer::into_buf(itemname).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::AddModalItem.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        Ok(())
    }

    pub fn get_radiobutton(&self, prompt: &str) -> Result<String, xous::Error> {
        self.lock();
        let spec = ManagedPromptWithFixedResponse {
            token: self.token,
            prompt: xous_ipc::String::from_str(prompt),
        };
        let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::PromptWithFixedResponse.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let itemname = buf.to_original::<ItemName, _>().unwrap();
        Ok(String::from(itemname.as_str()))
    }

    pub fn get_checkbox(&self, prompt: &str) -> Result<Vec::<String>, xous::Error> {
        self.lock();
        let spec = ManagedPromptWithFixedResponse {
            token: self.token,
            prompt: xous_ipc::String::from_str(prompt),
        };
        let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::PromptWithMultiResponse.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let selected_items = buf.to_original::<CheckBoxPayload, _>().unwrap();
        let mut ret = Vec::<String>::new();
        for maybe_item in selected_items.payload() {
            if let Some(item) = maybe_item {
                ret.push(String::from(item.as_str()));
            }
        }
        Ok(ret)
    }

    pub fn dynamic_notification(&self, title: Option<&str>, text: Option<&str>) -> Result<(), xous::Error> {
        self.lock();
        let spec = DynamicNotification {
            token: self.token,
            title: if let Some(t) = title {Some(xous_ipc::String::from_str(t))} else {None},
            text: if let Some(t) = text {Some(xous_ipc::String::from_str(t))} else {None},
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::DynamicNotification.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        Ok(())
    }
    pub fn dynamic_notification_update(&self, title: Option<&str>, text: Option<&str>) -> Result<(), xous::Error> {
        let spec = DynamicNotification {
            token: self.token,
            title: if let Some(t) = title {Some(xous_ipc::String::from_str(t))} else {None},
            text: if let Some(t) = text {Some(xous_ipc::String::from_str(t))} else {None},
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::UpdateDynamicNotification.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        Ok(())
    }
    pub fn dynamic_notification_close(&self) -> Result<(), xous::Error> {
        self.lock();
        send_message(self.conn,
            Message::new_scalar(Opcode::CloseDynamicNotification.to_usize().unwrap(),
            self.token[0] as usize,
            self.token[1] as usize,
            self.token[2] as usize,
            self.token[3] as usize,
            )
        ).expect("couldn't stop progress");
        Ok(())
    }

    /// busy-wait until we have acquired a mutex on the Modals server
    fn lock(&self) {
        while !self.try_get_mutex() {
            self.tt.sleep_ms(1000).unwrap();
        }
    }

    fn try_get_mutex(&self) -> bool {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::GetMutex.to_usize().unwrap(),
            self.token[0] as usize,
            self.token[1] as usize,
            self.token[2] as usize,
            self.token[3] as usize,
        )).expect("couldn't send mutex acquisition message") {
            xous::Result::Scalar1(code) => {
                if code == 1 {
                    true
                } else {
                    false
                }
            },
            _ => {
                log::error!("Internal error trying to acquire mutex");
                false
            }
        }

    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Modals {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}