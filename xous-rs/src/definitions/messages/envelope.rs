use super::{Message, MessageSender};

#[repr(C)]
#[derive(Debug, PartialEq)]
pub struct Envelope {
    pub sender: MessageSender,
    pub body: Message,
}

impl Envelope {
    pub fn to_usize(&self) -> [usize; 7] {
        let ret = match &self.body {
            Message::MutableBorrow(m) => (0, m.to_usize()),
            Message::Borrow(m) => (1, m.to_usize()),
            Message::Move(m) => (2, m.to_usize()),
            Message::Scalar(m) => (3, m.to_usize()),
            Message::BlockingScalar(m) => (4, m.to_usize()),
        };
        [
            self.sender.to_usize(),
            ret.0,
            ret.1[0],
            ret.1[1],
            ret.1[2],
            ret.1[3],
            ret.1[4],
        ]
    }
}

#[cfg(not(feature = "forget-memory-messages"))]
/// When a MessageEnvelope goes out of scope, return the memory.  It must either
/// go to the kernel (in the case of a Move), or back to the borrowed process
/// (in the case of a Borrow).  Ignore Scalar messages.
impl Drop for Envelope {
    fn drop(&mut self) {
        match &self.body {
            Message::Borrow(x) | Message::MutableBorrow(x) => {
                crate::syscall::return_memory_offset_valid(self.sender, x.buf, x.offset, x.valid)
                    .expect("couldn't return memory")
            }
            Message::Move(msg) => {
                crate::syscall::unmap_memory(msg.buf).expect("couldn't free memory message")
            }
            _ => (),
        }
    }
}
