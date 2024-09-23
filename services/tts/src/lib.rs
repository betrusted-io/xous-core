#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;
use num_traits::ToPrimitive;
use xous::{CID, Message, send_message};
use xous_ipc::Buffer;

#[derive(Debug)]
pub struct TtsFrontend {
    conn: CID,
}
impl TtsFrontend {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns
            .request_connection_blocking(api::SERVER_NAME_TTS)
            .expect("Can't connect to TtsFrontend server");
        Ok(TtsFrontend { conn })
    }

    /// A fully synchronous text to speech call. The text is turned into speech and played immediately.
    /// If there is speech currently playing, it is cut short and the new text takes its place.
    pub fn tts_simple(&self, text: &str) -> Result<(), xous::Error> {
        let msg = TtsFrontendMsg { text: String::from(text) };
        let buf = Buffer::into_buf(msg).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::TextToSpeech.to_u32().unwrap()).map(|_| ())
    }

    /// This blocks until the text is finished rendering
    pub fn tts_blocking(&self, text: &str) -> Result<(), xous::Error> {
        let msg = TtsFrontendMsg { text: String::from(text) };
        let buf = Buffer::into_buf(msg).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::TextToSpeechBlocking.to_u32().unwrap()).map(|_| ())
    }

    pub fn set_words_per_minute(&self, wpm: u32) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(Opcode::SetWordsPerMinute.to_usize().unwrap(), wpm as usize, 0, 0, 0),
        )
        .map(|_| ())
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for TtsFrontend {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this
        // object within a single process do not end up de-allocating the CID on other threads before
        // they go out of scope. Note to future me: you want this. Don't get rid of it because you
        // think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to
        // the object instance), de-allocate those items here. They don't need a reference count
        // because they are object-specific
    }
}
