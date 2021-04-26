#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;

use xous::{CID, send_message};
use num_traits::ToPrimitive;

pub struct Susres {
    conn: CID,
}
impl Susres {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_SUSRES).expect("Can't connect to SUSRES");
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