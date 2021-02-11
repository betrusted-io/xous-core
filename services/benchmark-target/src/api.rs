use xous::{Message, ScalarMessage};

#[repr(C)]
#[derive(Debug)]
pub struct TestStruct {
    pub challenge: [u32; 8],
}
impl TestStruct {
    pub fn new() -> Self {
        TestStruct {
            challenge: [0; 8],
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum Opcode {
    TestScalar(u32),
    TestMemory(TestStruct),
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::BlockingScalar(m) => match m.id {
                0 => Ok(Opcode::TestScalar(m.arg1 as u32)),
                _ => Err("BENCHMARK-TARGET api: unknown BlockingScalar ID"),
            },
            _ => Err("BENCHMARK-TARGET api: unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::TestScalar(count) => Message::BlockingScalar(ScalarMessage {
                id: 0,
                arg1: count as usize,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            _ => panic!("GFX api: Opcode type not handled by Into(), maybe you meant to use a helper method?"),
        }
    }
}
