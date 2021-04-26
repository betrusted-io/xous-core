#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;

use xous::{CID, send_message};
use num_traits::ToPrimitive;

pub struct Susres {
    conn: CID,
}
impl Susres {
    pub fn new() -> Result<Self, xous::Error> {
        let conn = xous::connect(xous::SID::from_bytes(b"xoussusresserver").unwrap()).expect("Can't connect to SUSRES server");
        Ok(Susres {
            conn
        })
    }
}

impl Drop for Susres {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        unsafe{xous::disconnect(self.conn).unwrap();}

    }
}