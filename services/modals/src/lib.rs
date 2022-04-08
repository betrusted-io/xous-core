#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use gam::*;
use xous::{CID, send_message, Message};
use num_traits::*;
use xous_ipc::Buffer;
use core::cell::Cell;

pub struct Modals {
    conn: CID,
    token: [u32; 4],
    have_lock: Cell::<bool>,
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
            have_lock: Cell::new(false),
        })
    }

    pub fn get_text(&self,
        prompt: &str,
        maybe_validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
        maybe_validator_op: Option<u32>,
    ) -> Result<TextEntryPayload, xous::Error> {
        self.lock();

        let mut spec = ManagedPromptWithTextResponse {
            token: self.token,
            prompt: xous_ipc::String::from_str(prompt),
        };
        // question: do we want to add a retry limit?
        loop {
            let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
            buf.lend_mut(self.conn, Opcode::PromptWithTextResponse.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
            match buf.to_original::<TextEntryPayload, _>() {
                Ok(response) => {
                    if let Some(validator) = maybe_validator {
                        if let Some(err_msg) = validator(response, maybe_validator_op.unwrap_or(0)) {
                            spec.prompt.clear();
                            spec.prompt.append(err_msg.as_str().unwrap_or("UTF-8 error")).ok();
                        } else {
                            send_message(self.conn,
                                Message::new_blocking_scalar(Opcode::TextResponseValid.to_usize().unwrap(),
                                self.token[0] as _, self.token[1] as _, self.token[2] as _, self.token[3] as _,
                            )).expect("couldn't acknowledge text entry");
                            self.unlock();
                            return Ok(response)
                        }
                    } else {
                        send_message(self.conn,
                            Message::new_blocking_scalar(Opcode::TextResponseValid.to_usize().unwrap(),
                            self.token[0] as _, self.token[1] as _, self.token[2] as _, self.token[3] as _,
                        )).expect("couldn't acknowledge text entry");
                        self.unlock();
                        return Ok(response)
                    }
                },
                _ => {
                    // we send the valid response token even in this case because we want the modals server to move on and not get stuck on this error.
                    send_message(self.conn,
                        Message::new_blocking_scalar(Opcode::TextResponseValid.to_usize().unwrap(),
                        self.token[0] as _, self.token[1] as _, self.token[2] as _, self.token[3] as _,
                    )).expect("couldn't acknowledge text entry");
                    self.unlock();
                    return Err(xous::Error::InternalError);
                }
            }
        }
    }

    /// this blocks until the notification has been acknowledged.
    pub fn show_notification(&self, notification: &str, as_qrcode: bool) -> Result<(), xous::Error> {
        self.lock();
        let spec = ManagedNotification {
            token: self.token,
            message: xous_ipc::String::from_str(notification),
            as_qrcode,
        };
        let buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::Notification.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        self.unlock();
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
        match xous::try_send_message(self.conn,
            Message::new_scalar(Opcode::DoUpdateProgress.to_usize().unwrap(), current as usize, 0, 0, 0)
        ) {
            Ok(_) => (),
            Err(e) => {
                log::warn!("update_progress failed with {:?}, skipping request", e);
                // most likely issue is that the server queue is overfull because too many progress updates were sent
                // sleep the sending thread to rate-limit requests, while discarding the current request.
                xous::yield_slice()
            }
        }
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
        self.unlock();
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
        self.unlock();
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
        self.unlock();
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
        send_message(self.conn,
            Message::new_scalar(Opcode::CloseDynamicNotification.to_usize().unwrap(),
            self.token[0] as usize,
            self.token[1] as usize,
            self.token[2] as usize,
            self.token[3] as usize,
            )
        ).expect("couldn't stop progress");
        self.unlock();
        Ok(())
    }

    /// Blocks until we have a lock on the modals server
    fn lock(&self) {
        if !self.have_lock.get() {
            match send_message(
                self.conn,
                Message::new_blocking_scalar(Opcode::GetMutex.to_usize().unwrap(),
                self.token[0] as usize,
                self.token[1] as usize,
                self.token[2] as usize,
                self.token[3] as usize,
            )).expect("couldn't send mutex acquisition message") {
                xous::Result::Scalar1(code) => {
                    if code != 1 {
                        log::warn!("Unexpected return from lock acquisition.");
                    }
                },
                _ => {
                    log::error!("Internal error trying to acquire mutex");
                }
            }
        }
        self.have_lock.set(true);
    }
    fn unlock(&self) {
        self.have_lock.set(false);
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
