pub(crate) const SERVER_NAME_U2F: &str     = "_U2F server_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Exits the server
    Quit,
}
