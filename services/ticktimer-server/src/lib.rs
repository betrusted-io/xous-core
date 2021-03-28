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

    pub fn elapsed_ms(&self) -> Result<u64, Error> {
        let response = send_message(self.conn,
            xous::Message::BlockingScalar(xous::ScalarMessage {
                id: api::Opcode::ElapsedMs.to_usize().unwrap(),
                arg1: 0, arg2: 0, arg3: 0, arg4: 0
            })
        )?;
        if let xous::Result::Scalar2(upper, lower) = response {
            Ok(upper as u64 | ((lower as u64) << 32))
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn sleep_ms(&self, ms: usize) -> Result<(), Error> {
        send_message(self.conn,
            xous::Message::BlockingScalar(xous::ScalarMessage {
                id: api::Opcode::SleepMs.to_usize().unwrap(),
                arg1: ms,
                arg2: 0, arg3: 0, arg4: 0
            })
        ).map(|_| ())
    }
}
