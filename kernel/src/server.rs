pub use crate::arch::ProcessContext;
use core::{mem, slice};
use xous::{CtxID, MemoryRange, MemorySize, PID, SID};

/// Internal representation of a queued message for a server. This should be
/// exactly 8 words / 32 bytes, yielding 128 queued messages per server
#[repr(usize)]
#[derive(PartialEq)]
enum QueuedMessage {
    Empty,
    ScalarMessage(
        usize, /* sender */
        usize, /* context */
        usize, /* id */
        usize, /* arg1 */
        usize, /* arg2 */
        usize, /* arg3 */
        usize, /* arg4 */
    ),
    MemoryMessageSend(
        usize, /* sender */
        usize, /* context */
        usize, /* id */
        usize, /* buf */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),
    MemoryMessageROLend(
        usize, /* sender */
        usize, /* context */
        usize, /* id */
        usize, /* buf */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),
    MemoryMessageRWLend(
        usize, /* sender */
        usize, /* context */
        usize, /* id */
        usize, /* buf */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),
}

/// A pointer to resolve a server ID to a particular process
#[derive(PartialEq)]
pub struct Server {
    /// A randomly-generated ID
    pub sid: SID,

    /// The process that owns this server
    pub pid: PID,

    /// An index into the queue
    queue_head: usize,

    queue_tail: usize,

    /// Where data will appear
    queue: &'static mut [QueuedMessage],

    /// The `context mask` is a bitfield of contexts that are able to handle
    /// this message. If there are no available contexts, then messages will
    /// need to be queued.
    ready_contexts: CtxID,
}

impl Server {
    pub fn init(
        new: &mut Option<Server>,
        pid: PID,
        sid: SID,
        queue_addr: *mut usize,
        queue_size: usize,
    ) -> Result<(), xous::Error> {
        if new != &None {
            return Err(xous::Error::MemoryInUse);
        }

        let queue = unsafe {
            slice::from_raw_parts_mut(
                queue_addr as *mut QueuedMessage,
                queue_size / mem::size_of::<QueuedMessage>(),
            )
        };

        *new = Some(Server {
            sid,
            pid,
            queue_head: 0,
            queue_tail: 0,
            queue,
            ready_contexts: 0,
        });
        Ok(())
    }
    /// Remove a message from the server's queue and replace it with
    /// QueuedMessage::Empty. Advance the queue pointer while we're at it.
    pub fn take_next_message(&mut self) -> Option<(xous::MessageEnvelope, CtxID)> {
        let result = match self.queue[self.queue_tail] {
            QueuedMessage::Empty => return None,
            QueuedMessage::MemoryMessageROLend(
                sender,
                context,
                id,
                buf,
                buf_size,
                offset,
                valid,
            ) => (
                xous::MessageEnvelope {
                    sender: sender,
                    message: xous::Message::ImmutableBorrow(xous::MemoryMessage {
                        id,
                        buf: MemoryRange::new(buf, buf_size),
                        offset: MemorySize::new(offset),
                        valid: MemorySize::new(valid),
                    }),
                },
                context,
            ),
            QueuedMessage::MemoryMessageRWLend(
                sender,
                context,
                id,
                buf,
                buf_size,
                offset,
                valid,
            ) => (
                xous::MessageEnvelope {
                    sender: sender,
                    message: xous::Message::MutableBorrow(xous::MemoryMessage {
                        id,
                        buf: MemoryRange::new(buf, buf_size),
                        offset: MemorySize::new(offset),
                        valid: MemorySize::new(valid),
                    }),
                },
                context,
            ),
            QueuedMessage::MemoryMessageSend(sender, context, id, buf, buf_size, offset, valid) => {
                (
                    xous::MessageEnvelope {
                        sender: sender,
                        message: xous::Message::Move(xous::MemoryMessage {
                            id,
                            buf: MemoryRange::new(buf, buf_size),
                            offset: MemorySize::new(offset),
                            valid: MemorySize::new(valid),
                        }),
                    },
                    context,
                )
            }
            QueuedMessage::ScalarMessage(sender, context, id, arg1, arg2, arg3, arg4) => (
                xous::MessageEnvelope {
                    sender: sender,
                    message: xous::Message::Scalar(xous::ScalarMessage {
                        id,
                        arg1,
                        arg2,
                        arg3,
                        arg4,
                    }),
                },
                context,
            ),
        };
        self.queue[self.queue_tail] = QueuedMessage::Empty;
        self.queue_tail += 1;
        if self.queue_tail >= self.queue.len() {
            self.queue_tail = 0;
        }
        Some(result)
    }

    /// Add the given message to this server's queue.
    pub fn queue_message(
        &mut self,
        context: usize,
        envelope: xous::MessageEnvelope,
    ) -> core::result::Result<(), xous::Error> {
        if self.queue[self.queue_head] != QueuedMessage::Empty {
            return Err(xous::Error::ServerQueueFull);
        }

        self.queue[self.queue_head] = match envelope.message {
            xous::Message::Scalar(msg) => QueuedMessage::ScalarMessage(
                envelope.sender,
                context,
                msg.id,
                msg.arg1,
                msg.arg2,
                msg.arg3,
                msg.arg4,
            ),
            xous::Message::Move(msg) => QueuedMessage::MemoryMessageSend(
                envelope.sender,
                context,
                msg.id,
                msg.buf.addr.get(),
                msg.buf.size.get(),
                msg.offset.map(|x| x.get()).unwrap_or(0) as usize,
                msg.valid.map(|x| x.get()).unwrap_or(0) as usize,
            ),
            xous::Message::MutableBorrow(_msg) => unimplemented!(),
            xous::Message::ImmutableBorrow(_msg) => unimplemented!(),
        };

        self.queue_head += 1;
        if self.queue_head >= self.queue.len() {
            self.queue_head = 0;
        }
        Ok(())
    }

    // assert!(
    //     mem::size_of::<QueuedMessage>() == 32,
    //     "QueuedMessage was supposed to be 32 bytes, but instead was {} bytes",
    //     mem::size_of::<QueuedMessage>()
    // );

    /// Return a context ID that is available and blocking.  If no such context
    /// ID exists, or if this server isn't actually ready to receive packets,
    /// return None.
    pub fn take_available_context(&mut self) -> Option<CtxID> {
        if self.ready_contexts == 0 {
            return None;
        }
        let mut test_ctx_mask = 1;
        let mut ctx_number = 0;
        loop {
            // If the context mask matches this context number, remove it
            // and return the index.
            if self.ready_contexts & test_ctx_mask == test_ctx_mask {
                self.ready_contexts = self.ready_contexts & !test_ctx_mask;
                return Some(ctx_number);
            }
            // Advance to the next slot.
            test_ctx_mask = test_ctx_mask.rotate_left(1);
            ctx_number = ctx_number + 1;
            if test_ctx_mask == 1 {
                panic!("didn't find a free context, even though there should be one");
            }
        }
    }

    /// Add the given context to the list of ready and waiting contexts.
    pub fn park_context(&mut self, context: CtxID) {
        self.ready_contexts |= 1 << context;
    }
}
