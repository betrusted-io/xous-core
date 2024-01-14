use core::convert::TryFrom;

pub mod envelope;
pub use envelope::Envelope as MessageEnvelope;

pub mod sender;
pub use sender::Sender as MessageSender;

pub mod id;
pub use id::Id as MessageId;

use super::{MemoryAddress, MemoryRange, MemorySize};

#[repr(C)]
#[derive(Debug, PartialEq)]
/// A struct describing memory that is passed between processes.
/// The `buf` value will get translated as necessary.
pub struct MemoryMessage {
    /// A user-assignable message ID.
    pub id: MessageId,

    /// The offset of the buffer.  This address will get transformed when the
    /// message is moved between processes.
    pub buf: MemoryRange,

    /// The offset within the buffer where the interesting stuff starts. The usage
    /// of this field is purely advisory, is not checked by the kernel, should not
    /// be trusted by the receiver. It is perfectly legal for this to be larger
    /// than the buffer size.
    ///
    /// As a result, this field may be repurposed for other uses. For example,
    /// you can store a `usize` in this field by setting
    /// `message.offset = MemoryAddress::new(val)`, and get a `usize` back by
    /// reading `message.offset.map(|v| v.get()).unwrap_or_default()`.
    ///
    /// For `MutableBorrow` messages this value will be returned to the sender and the
    /// field will be updated when the Message is returned. Therefore you may also use
    /// this field to communicate additional information to the message sender.
    pub offset: Option<MemoryAddress>,

    /// How many bytes in the buffer are valid. This field is advisory, and is not
    /// checked by the kernel. It is legal for the sender to specify a `valid` range
    /// that is larger than `buf.len()`, so this value should not be blindly trusted.
    ///
    /// As a result, this field may be repurposed for other uses. For example,
    /// you can store a `usize` in this field by setting
    /// `message.valid = MemoryAddress::new(val)`, and get a `usize` back by
    /// reading `message.valid.map(|v| v.get()).unwrap_or_default()`.
    ///
    /// For `MutableBorrow` messages this value will be returned to the sender and the
    /// field will be updated when the Message is returned. Therefore you may also use
    /// this field to communicate additional information to the message sender.
    pub valid: Option<MemorySize>,
}

impl MemoryMessage {
    pub fn from_usize(
        id: usize,
        addr: usize,
        size: usize,
        offset: usize,
        valid: usize,
    ) -> Option<MemoryMessage> {
        let addr = match MemoryAddress::new(addr) {
            None => return None,
            Some(s) => s,
        };
        let size = match MemorySize::new(size) {
            None => return None,
            Some(s) => s,
        };
        let buf = MemoryRange { addr, size };
        let offset = MemoryAddress::new(offset);
        let valid = MemorySize::new(valid);

        Some(MemoryMessage { id, buf, offset, valid })
    }

    pub fn to_usize(&self) -> [usize; 5] {
        [
            self.id,
            self.buf.addr.get(),
            self.buf.size.get(),
            self.offset.map(|e| e.get()).unwrap_or(0),
            self.valid.map(|e| e.get()).unwrap_or(0),
        ]
    }
}

#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy, Default)]
/// A simple scalar message.  This is similar to a `move` message.
pub struct ScalarMessage {
    pub id: MessageId,
    pub arg1: usize,
    pub arg2: usize,
    pub arg3: usize,
    pub arg4: usize,
}

impl ScalarMessage {
    pub fn from_usize(id: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize) -> ScalarMessage {
        ScalarMessage { id, arg1, arg2, arg3, arg4 }
    }

    pub fn to_usize(&self) -> [usize; 5] { [self.id, self.arg1, self.arg2, self.arg3, self.arg4] }
}

#[repr(usize)]
#[derive(Debug, PartialEq)]
pub enum Message {
    MutableBorrow(MemoryMessage),
    Borrow(MemoryMessage),
    Move(MemoryMessage),
    Scalar(ScalarMessage),
    BlockingScalar(ScalarMessage),
}
unsafe impl Send for Message {}

impl Message {
    pub fn new_scalar(id: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize) -> crate::Message {
        Message::Scalar(crate::ScalarMessage { id, arg1, arg2, arg3, arg4 })
    }

    pub fn new_blocking_scalar(
        id: usize,
        arg1: usize,
        arg2: usize,
        arg3: usize,
        arg4: usize,
    ) -> crate::Message {
        Message::BlockingScalar(crate::ScalarMessage { id, arg1, arg2, arg3, arg4 })
    }

    pub fn new_lend(
        id: usize,
        buf: MemoryRange,
        offset: Option<MemoryAddress>,
        valid: Option<MemorySize>,
    ) -> crate::Message {
        Message::Borrow(crate::MemoryMessage { id, buf, offset, valid })
    }

    pub fn new_lend_mut(
        id: usize,
        buf: MemoryRange,
        offset: Option<MemoryAddress>,
        valid: Option<MemorySize>,
    ) -> crate::Message {
        Message::MutableBorrow(crate::MemoryMessage { id, buf, offset, valid })
    }

    /// Determine whether the specified Message will block
    pub fn is_blocking(&self) -> bool {
        match *self {
            Message::MutableBorrow(_) | Message::Borrow(_) | Message::BlockingScalar(_) => true,
            Message::Move(_) | Message::Scalar(_) => false,
        }
    }

    /// Determine whether the specified message has data attached
    pub fn has_memory(&self) -> bool {
        match *self {
            Message::MutableBorrow(_) | Message::Borrow(_) | Message::Move(_) => true,
            Message::BlockingScalar(_) | Message::Scalar(_) => false,
        }
    }

    pub fn is_scalar(&self) -> bool { !self.has_memory() }

    pub fn memory(&self) -> Option<&MemoryRange> { self.memory_message().map(|msg| &msg.buf) }

    pub fn memory_message(&self) -> Option<&MemoryMessage> {
        match self {
            Message::MutableBorrow(mem) | Message::Borrow(mem) | Message::Move(mem) => Some(mem),
            Message::BlockingScalar(_) | Message::Scalar(_) => None,
        }
    }

    pub fn memory_message_mut(&mut self) -> Option<&mut MemoryMessage> {
        match self {
            Message::MutableBorrow(mem) | Message::Move(mem) => Some(mem),
            Message::BlockingScalar(_) | Message::Scalar(_) | Message::Borrow(_) => None,
        }
    }

    pub fn scalar_message(&self) -> Option<&ScalarMessage> {
        match self {
            Message::MutableBorrow(_) | Message::Borrow(_) | Message::Move(_) => None,
            Message::BlockingScalar(scalar) | Message::Scalar(scalar) => Some(scalar),
        }
    }

    pub fn scalar_message_mut(&mut self) -> Option<&mut ScalarMessage> {
        match self {
            Message::MutableBorrow(_) | Message::Borrow(_) | Message::Move(_) | Message::Scalar(_) => None,
            Message::BlockingScalar(scalar) => Some(scalar),
        }
    }

    pub(crate) fn message_type(&self) -> usize {
        match *self {
            Message::MutableBorrow(_) => 1,
            Message::Borrow(_) => 2,
            Message::Move(_) => 3,
            Message::Scalar(_) => 4,
            Message::BlockingScalar(_) => 5,
        }
    }

    /// Return the ID of this message
    pub fn id(&self) -> MessageId {
        match self {
            Message::MutableBorrow(mem) | Message::Borrow(mem) | Message::Move(mem) => mem.id,
            Message::Scalar(s) | Message::BlockingScalar(s) => s.id,
        }
    }

    /// Set the ID or opcode of this message
    pub fn set_id(&mut self, id: MessageId) {
        match self {
            Message::MutableBorrow(mem) | Message::Borrow(mem) | Message::Move(mem) => mem.id = id,
            Message::Scalar(s) | Message::BlockingScalar(s) => s.id = id,
        }
    }

    pub fn to_usize(&self) -> [usize; 6] {
        let ret = match self {
            Message::MutableBorrow(m) => (0, m.to_usize()),
            Message::Borrow(m) => (1, m.to_usize()),
            Message::Move(m) => (2, m.to_usize()),
            Message::Scalar(m) => (3, m.to_usize()),
            Message::BlockingScalar(m) => (4, m.to_usize()),
        };
        [ret.0, ret.1[0], ret.1[1], ret.1[2], ret.1[3], ret.1[4]]
    }
}

impl TryFrom<(usize, usize, usize, usize, usize, usize)> for Message {
    type Error = ();

    fn try_from(
        value: (usize, usize, usize, usize, usize, usize),
    ) -> core::result::Result<Self, Self::Error> {
        match value.0 {
            1 => Ok(Message::MutableBorrow(MemoryMessage {
                id: value.1,
                buf: unsafe { MemoryRange::new(value.2, value.3).map_err(|_| ()) }?,
                offset: MemoryAddress::new(value.4),
                valid: MemorySize::new(value.5),
            })),
            2 => Ok(Message::Borrow(MemoryMessage {
                id: value.1,
                buf: unsafe { MemoryRange::new(value.2, value.3).map_err(|_| ()) }?,
                offset: MemoryAddress::new(value.4),
                valid: MemorySize::new(value.5),
            })),
            3 => Ok(Message::Move(MemoryMessage {
                id: value.1,
                buf: unsafe { MemoryRange::new(value.2, value.3).map_err(|_| ()) }?,
                offset: MemoryAddress::new(value.4),
                valid: MemorySize::new(value.5),
            })),
            4 => Ok(Message::Scalar(ScalarMessage {
                id: value.1,
                arg1: value.2,
                arg2: value.3,
                arg3: value.4,
                arg4: value.5,
            })),
            5 => Ok(Message::BlockingScalar(ScalarMessage {
                id: value.1,
                arg1: value.2,
                arg2: value.3,
                arg3: value.4,
                arg4: value.5,
            })),
            _ => Err(()),
        }
    }
}
