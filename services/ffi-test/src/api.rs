pub(crate) const SERVER_NAME_FFITEST: &str = "_FfiTest Server_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    Quit,
}
