pub(crate) const SERVER_NAME_OQC: &str     = "_Outgoing Quality Check Test Program_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    Trigger,
    KeyCode,
    Status,
    UxGutter,
    ModalRedraw,
    ModalKeys,
    ModalDrop,
    Quit,
}
