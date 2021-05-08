#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};
use num_traits::ToPrimitive;

pub struct Codec {
    conn: CID,
}
impl Codec {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_CODEC).expect("Can't connect to Codec server");
        Ok(Codec {
            conn
        })
    }
}

impl Drop for Codec {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        unsafe{xous::disconnect(self.conn).unwrap();}

    }
}