use num_traits::{ToPrimitive, FromPrimitive};

pub(crate) const SERVER_NAME: &str    = "_Callback test server_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum Opcode {
    RegisterListener,
    UnregisterListener,
    Tick,
}
