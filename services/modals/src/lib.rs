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
}
impl Modals {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_MODALS).expect("Can't connect to Modals server");
        Ok(Modals {
            conn
        })
    }

    pub fn get_text_input(&self,
        prompt: &str,
        maybe_validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr> >,
        maybe_validator_op: Option<u32>,
    ) -> Result<TextEntryPayload, xous::Error> {
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
        let spec = ManagedPromptWithTextResponse {
            prompt: xous_ipc::String::<1024>::from_str(prompt),
            validator,
            validator_op: maybe_validator_op.unwrap_or(0)
        };
        let mut buf = Buffer::into_buf(spec).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::PromptWithTextResponse.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        if let Some(server) = validator {
            let cid = xous::connect(SID::from_array(server)).unwrap();
            send_message(cid, Message::new_blocking_scalar(ValidationOp::Quit.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap()}; // must disconnect before destroy to avoid the CID from hanging out in our outbound table which is limited to 32 entries
            xous::destroy_server(SID::from_array(server)).expect("couldn't destroy temporary server");
        }
        match buf.to_original::<TextEntryPayload, _>() {
            Ok(response) => Ok(response),
            _ => Err(xous::Error::InternalError)
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
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}