use super::{Message, MessageId, MessageSender};
use crate::CID;

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

    /// Take the message, throwing away the sender information. If the message is
    /// Blocking (i.e. if message.is_blocking() is `true`), then the process that
    /// sent this message will not automatically get a response. You will need to
    /// manually send a response.
    ///
    /// If the message is non-blocking, then this has no side effects.
    ///
    /// This is equivalent to "leaking" the message, which is not strictly unsafe.
    pub fn take_message(self) -> Message {
        use core::mem::ManuallyDrop;
        // Convert `Self` into something that won't have its "Drop" method called
        let manual_self = ManuallyDrop::new(self);

        // This function only works on memory messages
        unsafe { core::ptr::read(&manual_self.body) }
    }

    /// Take this message and forward it to another server.
    ///
    /// **Note**: For blocking messages, this will block until the other server responds to
    /// the message. In the future, this may be turned into a nonblocking operation where
    /// this will return immediately and allow the target Server to respond directly.
    ///
    /// ## Result
    ///
    /// If the result is successful, then nothing is returned.
    ///
    /// If there is an error, then the original Envelope is returned along with the resulting
    /// error.
    pub fn forward(
        mut self,
        connection: CID,
        id: MessageId,
    ) -> Result<(), (Envelope, crate::Error)> {
        use core::mem::ManuallyDrop;

        // Update our ID to match the newly-sent message. Reuse the same message struct.
        self.body.set_id(id);

        // Convert `Self` into something that won't have its "Drop" method called
        let manual_self = ManuallyDrop::new(self);

        // Unsafe because there are now two things that are pointing at "self.body". However,
        // this is fine since these two pointers are never used at the same time.
        let body = unsafe { core::ptr::read(&manual_self.body) };
        let sender = unsafe { core::ptr::read(&manual_self.sender) };

        // Different messages have different kinds of lifetimes, so they must all be
        // handled differently.
        match body {
            Message::Move(_) => {
                let result = crate::send_message(connection, body);

                // If the Move was successful, return so.
                if let Ok(crate::Result::Ok) = result {
                    // `self` goes out of scope here without having `Drop` called on it
                    return Ok(());
                }

                // If there's an error, reconstitute ourselves and return.
                if let Err(e) = result {
                    return Err((ManuallyDrop::into_inner(manual_self), e));
                }

                return Err((
                    ManuallyDrop::into_inner(manual_self),
                    crate::Error::MemoryInUse,
                ));
            }
            Message::BlockingScalar(_) => {
                let result = crate::send_message(connection, body);

                // If there's an error, reconstitute ourselves and return.
                if let Err(e) = result {
                    return Err((ManuallyDrop::into_inner(manual_self), e));
                } else if let Ok(crate::Result::Scalar1(v)) = result {
                    if let Err(e) = crate::return_scalar(sender, v) {
                        return Err((ManuallyDrop::into_inner(manual_self), e));
                    }
                    return Ok(());
                } else if let Ok(crate::Result::Scalar2(v1, v2)) = result {
                    if let Err(e) = crate::return_scalar2(sender, v1, v2) {
                        return Err((ManuallyDrop::into_inner(manual_self), e));
                    }
                    return Ok(());
                }
                return Err((
                    ManuallyDrop::into_inner(manual_self),
                    crate::Error::MemoryInUse,
                ));
            }

            _ => {
                let result = crate::send_message(connection, body);
                let new_self = ManuallyDrop::into_inner(manual_self);

                if let Ok(crate::Result::Ok) = result {
                    // `new_self` will have its Drop() called
                    return Ok(());
                }

                // If there's an error, reconstitute ourselves and return.
                if let Err(e) = result {
                    return Err((new_self, e));
                }

                return Err((new_self, crate::Error::MemoryInUse));
            }
        }
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
