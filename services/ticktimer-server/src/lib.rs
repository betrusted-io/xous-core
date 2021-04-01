#![cfg_attr(target_os = "none", no_std)]

pub mod api;

use xous::{send_message, Error, CID};
use num_traits::ToPrimitive;

pub struct Ticktimer {
    conn: CID,
}
impl Ticktimer {
    pub fn new() -> Result<Self, Error> {
        let conn = xous::connect(xous::SID::from_bytes(b"ticktimer-server").unwrap())?;
        Ok(Ticktimer {
           conn,
        })
    }

    /// note special case for elapsed_ms() is "infalliable". it really should never fail so get rid of the Error
    pub fn elapsed_ms(&self) -> u64 {
        let response = send_message(self.conn,
            xous::Message::new_blocking_scalar(api::Opcode::ElapsedMs.to_usize().unwrap(),
                0, 0, 0, 0,
            )).expect("Ticktimer: failure to send message to Ticktimer");
        if let xous::Result::Scalar2(upper, lower) = response {
            upper as u64 | ((lower as u64) << 32)
        } else {
            panic!("Ticktimer elapsed_ms(): unexpected return value: {:#?}", response);
        }
    }

    pub fn sleep_ms(&self, ms: usize) -> Result<(), Error> {
        send_message(self.conn,
            xous::Message::new_blocking_scalar(api::Opcode::SleepMs.to_usize().unwrap(),
                 ms,
                 0, 0, 0)
        ).map(|_| ())
    }
}
