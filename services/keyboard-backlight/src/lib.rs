pub mod api;
use api::*;

use num_traits::*;


#[derive(Debug)]
pub struct KeyboardBacklight {
    conn: xous::CID,
}
impl KeyboardBacklight {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::KBB_SERVER_NAME).expect("Can't connect to KBD");
        Ok(KeyboardBacklight {
            conn,
        })
    }

    pub fn cid(&self) -> xous::CID {
        self.conn
    }

    pub fn enabled(&self) -> Result<bool, xous::Error> {
        match xous::send_message(self.conn,
            xous::Message::new_blocking_scalar(KbbOps::Status.to_usize().unwrap(),
                0,
                0,
                0,
                0
            )
        ) {
            Ok(xous::Result::Scalar1(last_state)) => {
                if last_state == 1 {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            _ => {
                Err(xous::Error::InternalError)
            }
        }
    }
}