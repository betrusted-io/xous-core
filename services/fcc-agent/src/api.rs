use xous::{Message, ScalarMessage};

#[derive(Debug)]
pub enum Opcode<'a> {
    Char(u8),
    RxStats(&'a [u8]),
}
impl<'a> core::convert::TryFrom<&'a Message> for Opcode<'a> {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                1 => Ok(Opcode::Char(m.arg1 as u8)),
                _ => Err("unrecognized opcode"),
            },
            Message::Borrow(m) => match m.id {
                2 => {
                    let stats = unsafe {
                        core::slice::from_raw_parts(
                            m.buf.as_ptr(),
                            m.valid.map(|x| x.get()).unwrap_or_else(|| m.buf.len()),
                        )
                    };
                    Ok(Opcode::RxStats(stats))
                }
                _ => Err("unrecognized opcode"),
            }
            _ => Err("unhandled message type"),
        }
    }
}
impl<'a> Into<Message> for Opcode<'a> {
    fn into(self) -> Message {
        match self {
            Opcode::Char(c) => Message::Scalar(ScalarMessage {
                id: 1, arg1: c as usize, arg2: 0, arg3: 0, arg4: 0}),
            Opcode::RxStats(stats) => {
                let data = xous::carton::Carton::from_bytes(stats);
                Message::Borrow(data.into_message(2))
            }
        }
    }
}
