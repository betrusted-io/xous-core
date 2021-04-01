#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};
use num_traits::ToPrimitive;

pub struct Trng {
    conn: CID,
}
impl Trng {
    pub fn new(xns: xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_TRNG).expect("Can't connect to TRNG server");
        Ok(Trng {
            conn
        })
    }
    pub fn get_u32(&self) -> Result<u32, xous::Error> {
        let response = send_message(self.conn,
            xous::Message::new_blocking_scalar(api::Opcode::GetTrng.to_usize().unwrap(),
                1 /* count */, 0, 0, 0, )
            ).expect("TRNG|LIB: can't get_u32");
        if let xous::Result::Scalar2(trng, _) = response {
            Ok(trng as u32)
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }
    pub fn get_u64(&self) -> Result<u64, xous::Error> {
        let response = send_message(self.conn,
            xous::Message::new_blocking_scalar(api::Opcode::GetTrng.to_usize().unwrap(),
                2 /* count */, 0, 0, 0, )
        ).expect("TRNG|LIB: can't get_u32");
    if let xous::Result::Scalar2(lo, hi) = response {
            Ok( lo as u64 | ((hi as u64) << 32) )
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }
}
