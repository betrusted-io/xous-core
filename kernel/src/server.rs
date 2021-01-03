pub use crate::arch::process::Thread;
use core::mem;
use xous_kernel::{MemoryAddress, MemoryRange, MemorySize, Message, MessageSender, PID, SID, TID};

pub struct SenderID {
    /// The index of the server within the SystemServices table
    pub sidx: usize,
    /// The index into the queue array
    pub idx: usize,
    /// The process ID that sent this message
    pid: Option<PID>,
}

impl SenderID {
    pub fn new(sidx: usize, idx: usize, pid: Option<PID>) -> Self {
        SenderID { sidx, idx, pid }
    }
}

impl From<usize> for SenderID {
    fn from(item: usize) -> SenderID {
        SenderID {
            sidx: (item >> 16) & 0xff,
            idx: item & 0xffff,
            pid: PID::new((item >> 24) as u8),
        }
    }
}

impl Into<usize> for SenderID {
    fn into(self) -> usize {
        (self.pid.map(|x| x.get() as usize).unwrap_or(0) << 24)
            | ((self.sidx << 16) & 0x00ff0000)
            | (self.idx & 0xffff)
    }
}

impl From<MessageSender> for SenderID {
    fn from(item: MessageSender) -> SenderID {
        SenderID::from(item.to_usize())
    }
}

impl Into<MessageSender> for SenderID {
    fn into(self) -> MessageSender {
        MessageSender::from_usize(self.into())
    }
}

#[derive(Debug)]
pub enum WaitingMessage {
    /// There is no waiting message.
    None,

    /// The memory was borrowed and should be returned to the given process.
    BorrowedMemory(PID, TID, MemoryAddress, MemoryAddress, MemorySize),

    /// The memory was moved, and so shouldn't be returned.
    MovedMemory,

    /// The message was a scalar message, so you should return the result to the process
    ScalarMessage(PID, TID),

    /// This memory should be returned to the system.
    ForgetMemory(MemoryRange),
}

/// Internal representation of a queued message for a server. This should be
/// exactly 8 words / 32 bytes, yielding 128 queued messages per server
#[repr(usize)]
#[derive(PartialEq, Debug)]
enum QueuedMessage {
    Empty,
    BlockingScalarMessage(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* server return address */
        usize, /* id */
        usize, /* arg1 */
        usize, /* arg2 */
        usize, /* arg3 */
        usize, /* arg4 */
    ),
    ScalarMessage(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* server return address */
        usize, /* id */
        usize, /* arg1 */
        usize, /* arg2 */
        usize, /* arg3 */
        usize, /* arg4 */
    ),
    MemoryMessageSend(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* reserved */
        usize, /* id */
        usize, /* buf */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),
    MemoryMessageROLend(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* address of memory base in server */
        usize, /* id */
        usize, /* buf */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),
    MemoryMessageRWLend(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* address of memory base in server */
        usize, /* id */
        usize, /* buf */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),

    /// The process lending this memory terminated before
    /// we could receive the message.
    MemoryMessageROLendTerminated(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* address of memory base in server */
        usize, /* id */
        usize, /* buf */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),

    /// The process lending this memory terminated before
    /// we could receive the message.
    MemoryMessageRWLendTerminated(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* address of memory base in server */
        usize, /* id */
        usize, /* buf */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),

    /// The process waiting for the response terminated before
    /// we could receive the message.
    BlockingScalarTerminated(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* server return address */
        usize, /* id */
        usize, /* arg1 */
        usize, /* arg2 */
        usize, /* arg3 */
        usize, /* arg4 */
    ),

    /// When a message is taken that needs to be returned -- such as an ROLend
    /// or RWLend -- the slot is replaced with a WaitingReturnMemory token and its
    /// index is returned as the message sender.  This is used to unblock the
    /// sending process.
    WaitingReturnMemory(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* address of memory base in server */
        usize, /* client base address */
        usize, /* Range size */
    ),

    /// When a server goes away, its memory must be forgotten instead of being returned
    /// to the previous process.
    WaitingForget(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* address of memory base in server */
        usize, /* client base address */
        usize, /* Range size */
    ),

    /// This is the state when a message is blocking, but has no associated memory
    /// page.
    WaitingReturnScalar(
        u16,   /* client PID */
        u16,   /* client TID */
        usize, /* server return address */
    ),
}

/// A pointer to resolve a server ID to a particular process
#[derive(PartialEq, Debug)]
pub struct Server {
    /// A randomly-generated ID
    pub sid: SID,

    /// The process that owns this server
    pub pid: PID,

    /// An index into the queue
    queue_head: usize,

    queue_tail: usize,

    /// Where data will appear
    #[cfg(baremetal)]
    queue: &'static mut [QueuedMessage],
    #[cfg(not(baremetal))]
    queue: Vec<QueuedMessage>,

    /// The `context mask` is a bitfield of contexts that are able to handle
    /// this message. If there are no available contexts, then messages will
    /// need to be queued.
    ready_threads: usize,
}

impl Server {
    /// Initialize a server in the given option array. This function is
    /// designed to be called with `new` pointing to an entry in a vec.
    ///
    /// # Errors
    ///
    /// * **MemoryInUse**: The provided Server option already exists
    pub fn init(
        new: &mut Option<Server>,
        pid: PID,
        sid: SID,
        _backing: MemoryRange,
    ) -> Result<(), xous_kernel::Error> {
        if new != &None {
            return Err(xous_kernel::Error::MemoryInUse);
        }

        #[cfg(baremetal)]
        let queue = unsafe {
            core::slice::from_raw_parts_mut(
                _backing.as_mut_ptr() as *mut QueuedMessage,
                _backing.len() / mem::size_of::<QueuedMessage>(),
            )
        };

        #[cfg(not(baremetal))]
        let queue = {
            let mut queue = vec![];
            // TODO: Replace this with a direct operation on a passed-in page
            queue.resize_with(
                crate::arch::mem::PAGE_SIZE / mem::size_of::<QueuedMessage>(),
                || QueuedMessage::Empty,
            );
            queue
        };

        *new = Some(Server {
            sid,
            pid,
            queue_head: 0,
            queue_tail: 0,
            queue,
            ready_threads: 0,
        });
        Ok(())
    }

    /// Take a current slot and replace it with `None`, clearing out the contents of the queue.
    pub fn destroy(current: &mut Option<Server>) -> Result<(), xous_kernel::Error> {
        if let Some(mut server) = current.take() {
            server.queue_head = 0;
            server.queue_tail = 0;
            server.ready_threads = 0;
        }
        Ok(())
    }

    // pub fn print_queue(&self) {
    //     println!("    Q Queue Head: {}", self.queue_head);
    //     println!("    Q Queue Tail: {}", self.queue_tail);
    //     for (_idx, _entry) in self.queue.iter().enumerate() {
    //         if _entry != &QueuedMessage::Empty {
    //             println!("    Q  entry[{}]: {:?}", _idx, _entry);
    //         }
    //     }
    // }

    /// When a process terminates, there may be memory that is lent to us.
    /// Mark all of that memory to be discarded when it is returned, rather than
    /// giving it back to the previous process space.
    pub fn discard_messages_for_pid(&mut self, pid: PID) {
        for entry in self.queue.iter_mut() {
            match *entry {
                QueuedMessage::MemoryMessageROLend(
                    msg_pid,
                    tid,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                    arg5,
                    arg6,
                ) => {
                    if msg_pid == pid.get() as _ {
                        *entry = QueuedMessage::MemoryMessageROLendTerminated(
                            msg_pid, tid, arg1, arg2, arg3, arg4, arg5, arg6,
                        );
                    }
                }
                QueuedMessage::MemoryMessageRWLend(
                    msg_pid,
                    tid,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                    arg5,
                    arg6,
                ) => {
                    if msg_pid == pid.get() as _ {
                        *entry = QueuedMessage::MemoryMessageRWLendTerminated(
                            msg_pid, tid, arg1, arg2, arg3, arg4, arg5, arg6,
                        );
                    }
                }
                QueuedMessage::BlockingScalarMessage(
                    msg_pid,
                    tid,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                    arg5,
                    arg6,
                ) => {
                    if msg_pid == pid.get() as _ {
                        *entry = QueuedMessage::BlockingScalarTerminated(
                            msg_pid, tid, arg1, arg2, arg3, arg4, arg5, arg6,
                        );
                    }
                }
                // For "Scalar" and "Move" messages, this memory has already
                // been moved into this process, so memory will be reclaimed
                // when the process terminates.
                _ => (),
            }
        }
    }

    /// Convert a `QueuedMesage::WaitingReturnMemory` into `QueuedMessage::Empty`
    /// and return the pair.  Advance the tail.  Note that the `idx` could be
    /// somewhere other than the tail, but as long as it points to a valid
    /// message that's waiting a response, that's acceptable.
    pub fn take_waiting_message(
        &mut self,
        idx: usize,
        buf: Option<&MemoryRange>,
    ) -> Result<WaitingMessage, xous_kernel::Error> {
        let current_val = self
            .queue
            .get_mut(idx)
            .ok_or(xous_kernel::Error::BadAddress)?;
        klog!("memory in queue[{}]: {:?}", idx, current_val);
        let (pid, tid, server_addr, client_addr, len, forget, is_memory) = match *current_val {
            QueuedMessage::WaitingReturnMemory(pid, tid, server_addr, client_addr, len) => {
                (pid, tid, server_addr, client_addr, len, false, true)
            }
            QueuedMessage::WaitingForget(pid, tid, server_addr, client_addr, len) => {
                (pid, tid, server_addr, client_addr, len, true, true)
            }
            QueuedMessage::WaitingReturnScalar(pid, tid, return_address) => {
                (pid, tid, return_address, 0, 0, true, false)
            }
            _ => return Ok(WaitingMessage::None),
        };

        // Sanity check the specified address was correct, and matches what we
        // had cached.
        if is_memory && cfg!(baremetal) {
            let buf = buf.expect("memory message expected but no buffer passed!");
            if server_addr != buf.as_ptr() as usize || len != buf.len() {
                // println!("KERNEL: Memory is attached but the returned buffer doesn't match (len: {} vs {}), buf addr: {:08x} vs {:08x}", len, buf.len(), server_addr, buf.as_ptr() as usize);
                Err(xous_kernel::Error::BadAddress)?;
            }
        }
        *current_val = QueuedMessage::Empty;
        self.queue_tail += 1;
        if self.queue_tail >= self.queue.len() {
            self.queue_tail = 0;
        }

        // Destructure the PID and context ID from the `pid_tid` field
        klog!(
            "taking waiting message and returning to pid: {} tid: {}",
            pid,
            tid
        );

        if !is_memory {
            return Ok(WaitingMessage::ScalarMessage(
                PID::new(pid as _).unwrap(),
                tid as _,
            ));
        }

        if forget {
            return Ok(WaitingMessage::ForgetMemory(MemoryRange::new(
                server_addr,
                len,
            )?));
        }

        // If a `move` address somehow ends up here, indicate the memory has been moved.
        let server_addr = match MemoryAddress::new(server_addr) {
            Some(o) => o,
            None => return Ok(WaitingMessage::MovedMemory),
        };

        let client_addr = MemoryAddress::new(client_addr)
            .expect("client memory address was 0, but server address was not");
        let len = MemorySize::new(len).expect("memory length was 0, but address was not None");
        Ok(WaitingMessage::BorrowedMemory(
            PID::new(pid as _).unwrap(),
            tid as _,
            server_addr,
            client_addr,
            len,
        ))
    }

    /// Remove a message from the server's queue and replace it with either a QueuedMessage::WaitingReturnMemory
    /// or, for Scalar messages, QueuedMessage::Empty.
    ///
    /// For non-Scalar messages, you must call `take_waiting_message()` in order to return
    /// memory to the calling process.
    ///
    /// # Returns
    ///
    /// * **None**: There are no waiting messages
    /// ***Some(MessageEnvelope): This message is queued.
    pub fn take_next_message(&mut self, sidx: usize) -> Option<xous_kernel::MessageEnvelope> {
        // println!(
        //     "queue_head: ((({})))  queue_tail: ((({}))): {:?}  CID: ((({})))",
        //     self.queue_head, self.queue_tail, self.queue[self.queue_tail], sidx
        // );
        use core::convert::TryInto;
        let mut sender = SenderID::new(sidx, self.queue_tail, None);
        let (result, response) = match self.queue[self.queue_tail] {
            QueuedMessage::Empty => return None,
            QueuedMessage::WaitingReturnMemory(_, _, _, _, _) => return None,
            QueuedMessage::WaitingForget(_, _, _, _, _) => return None,
            QueuedMessage::WaitingReturnScalar(_, _, _) => return None,
            QueuedMessage::MemoryMessageROLend(
                pid,
                tid,
                client_addr,
                id,
                buf,
                buf_size,
                offset,
                valid,
            ) => {
                sender.pid = PID::new(pid.try_into().unwrap());
                (
                    xous_kernel::MessageEnvelope {
                        sender: sender.into(),
                        body: xous_kernel::Message::Borrow(xous_kernel::MemoryMessage {
                            id,
                            buf: MemoryRange::new(buf, buf_size).ok()?,
                            offset: MemorySize::new(offset),
                            valid: MemorySize::new(valid),
                        }),
                    },
                    QueuedMessage::WaitingReturnMemory(pid, tid, buf, client_addr, buf_size),
                )
            }
            QueuedMessage::MemoryMessageRWLend(
                pid,
                tid,
                client_addr,
                id,
                buf,
                buf_size,
                offset,
                valid,
            ) => {
                sender.pid = PID::new(pid.try_into().unwrap());
                (
                    xous_kernel::MessageEnvelope {
                        sender: sender.into(),
                        body: xous_kernel::Message::MutableBorrow(xous_kernel::MemoryMessage {
                            id,
                            buf: MemoryRange::new(buf, buf_size).ok()?,
                            offset: MemorySize::new(offset),
                            valid: MemorySize::new(valid),
                        }),
                    },
                    QueuedMessage::WaitingReturnMemory(pid, tid, buf, client_addr, buf_size),
                )
            }
            QueuedMessage::MemoryMessageROLendTerminated(
                pid,
                tid,
                client_addr,
                id,
                buf,
                buf_size,
                offset,
                valid,
            ) => {
                sender.pid = PID::new(pid.try_into().unwrap());
                (
                    xous_kernel::MessageEnvelope {
                        sender: sender.into(),
                        body: xous_kernel::Message::Borrow(xous_kernel::MemoryMessage {
                            id,
                            buf: MemoryRange::new(buf, buf_size).ok()?,
                            offset: MemorySize::new(offset),
                            valid: MemorySize::new(valid),
                        }),
                    },
                    QueuedMessage::WaitingReturnMemory(pid, tid, buf, client_addr, buf_size),
                )
            }
            QueuedMessage::MemoryMessageRWLendTerminated(
                pid,
                tid,
                client_addr,
                id,
                buf,
                buf_size,
                offset,
                valid,
            ) => {
                sender.pid = PID::new(pid.try_into().unwrap());
                (
                    xous_kernel::MessageEnvelope {
                        sender: sender.into(),
                        body: xous_kernel::Message::MutableBorrow(xous_kernel::MemoryMessage {
                            id,
                            buf: MemoryRange::new(buf, buf_size).ok()?,
                            offset: MemorySize::new(offset),
                            valid: MemorySize::new(valid),
                        }),
                    },
                    QueuedMessage::WaitingReturnMemory(pid, tid, buf, client_addr, buf_size),
                )
            }

            QueuedMessage::BlockingScalarMessage(
                pid,
                tid,
                client_addr,
                id,
                arg1,
                arg2,
                arg3,
                arg4,
            ) => {
                sender.pid = PID::new(pid.try_into().unwrap());
                (
                    xous_kernel::MessageEnvelope {
                        sender: sender.into(),
                        body: xous_kernel::Message::BlockingScalar(xous_kernel::ScalarMessage {
                            id,
                            arg1,
                            arg2,
                            arg3,
                            arg4,
                        }),
                    },
                    QueuedMessage::WaitingReturnScalar(pid, tid, client_addr),
                )
            }
            QueuedMessage::MemoryMessageSend(
                pid,
                _tid,
                _reserved,
                id,
                buf,
                buf_size,
                offset,
                valid,
            ) => {
                sender.pid = PID::new(pid.try_into().unwrap());
                let msg = xous_kernel::MessageEnvelope {
                    sender: sender.into(),
                    body: xous_kernel::Message::Move(xous_kernel::MemoryMessage {
                        id,
                        buf: MemoryRange::new(buf, buf_size).ok()?,
                        offset: MemorySize::new(offset),
                        valid: MemorySize::new(valid),
                    }),
                };
                self.queue[self.queue_tail] = QueuedMessage::Empty;
                self.queue_tail += 1;
                if self.queue_tail >= self.queue.len() {
                    self.queue_tail = 0;
                }
                return Some(msg);
            }

            // Scalar messages have nothing to return, so they can go straight to the `Free` state
            QueuedMessage::ScalarMessage(pid, _tid, _reserved, id, arg1, arg2, arg3, arg4) => {
                sender.pid = PID::new(pid.try_into().unwrap());
                let msg = xous_kernel::MessageEnvelope {
                    sender: sender.into(),
                    body: xous_kernel::Message::Scalar(xous_kernel::ScalarMessage {
                        id,
                        arg1,
                        arg2,
                        arg3,
                        arg4,
                    }),
                };
                self.queue[self.queue_tail] = QueuedMessage::Empty;
                self.queue_tail += 1;
                if self.queue_tail >= self.queue.len() {
                    self.queue_tail = 0;
                }
                return Some(msg);
            }
            QueuedMessage::BlockingScalarTerminated(
                pid,
                _tid,
                _reserved,
                id,
                arg1,
                arg2,
                arg3,
                arg4,
            ) => {
                sender.pid = PID::new(pid.try_into().unwrap());
                let msg = xous_kernel::MessageEnvelope {
                    sender: sender.into(),
                    body: xous_kernel::Message::Scalar(xous_kernel::ScalarMessage {
                        id,
                        arg1,
                        arg2,
                        arg3,
                        arg4,
                    }),
                };
                self.queue[self.queue_tail] = QueuedMessage::Empty;
                self.queue_tail += 1;
                if self.queue_tail >= self.queue.len() {
                    self.queue_tail = 0;
                }
                return Some(msg);
            }
        };

        self.queue[self.queue_tail] = response;
        Some(result)
    }

    /// Add the given message to this server's queue.
    ///
    /// # Errors
    ///
    /// * **ServerQueueFull**: The server queue cannot accept any more messages
    pub fn queue_message(
        &mut self,
        pid: PID,
        context: TID,
        message: xous_kernel::Message,
        original_address: Option<MemoryAddress>,
    ) -> core::result::Result<usize, xous_kernel::Error> {
        // println!("Queueing message: {:?} for pid: {}  tid: {}", message, pid.get(), context);
        if self.queue[self.queue_head] != QueuedMessage::Empty {
            return Err(xous_kernel::Error::ServerQueueFull);
        }

        self.queue[self.queue_head] = match message {
            xous_kernel::Message::Scalar(msg) => QueuedMessage::ScalarMessage(
                pid.get() as _,
                context as _,
                original_address.map(|x| x.get()).unwrap_or(0),
                msg.id,
                msg.arg1,
                msg.arg2,
                msg.arg3,
                msg.arg4,
            ),
            xous_kernel::Message::BlockingScalar(msg) => QueuedMessage::BlockingScalarMessage(
                pid.get() as _,
                context as _,
                original_address.map(|x| x.get()).unwrap_or(0),
                msg.id,
                msg.arg1,
                msg.arg2,
                msg.arg3,
                msg.arg4,
            ),
            xous_kernel::Message::Move(msg) => QueuedMessage::MemoryMessageSend(
                pid.get() as _,
                context as _,
                original_address.map(|x| x.get()).unwrap_or(0),
                msg.id,
                msg.buf.addr.get(),
                msg.buf.size.get(),
                msg.offset.map(|x| x.get()).unwrap_or(0) as usize,
                msg.valid.map(|x| x.get()).unwrap_or(0) as usize,
            ),
            xous_kernel::Message::MutableBorrow(msg) => QueuedMessage::MemoryMessageRWLend(
                pid.get() as _,
                context as _,
                original_address.map(|x| x.get()).unwrap_or(0),
                msg.id,
                msg.buf.addr.get(),
                msg.buf.size.get(),
                msg.offset.map(|x| x.get()).unwrap_or(0) as usize,
                msg.valid.map(|x| x.get()).unwrap_or(0) as usize,
            ),
            xous_kernel::Message::Borrow(msg) => QueuedMessage::MemoryMessageROLend(
                pid.get() as _,
                context as _,
                original_address.map(|x| x.get()).unwrap_or(0),
                msg.id,
                msg.buf.addr.get(),
                msg.buf.size.get(),
                msg.offset.map(|x| x.get()).unwrap_or(0) as usize,
                msg.valid.map(|x| x.get()).unwrap_or(0) as usize,
            ),
        };

        let idx = self.queue_head;
        self.queue_head += 1;
        if self.queue_head >= self.queue.len() {
            self.queue_head = 0;
        }
        Ok(idx)
    }

    pub fn queue_response(
        &mut self,
        pid: PID,
        context: TID,
        message: &Message,
        client_address: Option<MemoryAddress>,
    ) -> core::result::Result<usize, xous_kernel::Error> {
        // println!("Queueing address message: {:?} (pid: {} tid: {})", message, pid.get(), context);
        if self.queue[self.queue_head] != QueuedMessage::Empty {
            return Err(xous_kernel::Error::ServerQueueFull);
        }
        self.queue[self.queue_head] = match message {
            xous_kernel::Message::Scalar(_) | xous_kernel::Message::BlockingScalar(_) => {
                QueuedMessage::WaitingReturnScalar(
                    pid.get() as _,
                    context as _,
                    client_address.map(|x| x.get()).unwrap_or(0),
                )
            }
            xous_kernel::Message::Move(msg) => {
                let server_address = msg.buf.addr.get();
                let len = msg.buf.size.get();
                QueuedMessage::WaitingForget(
                    pid.get() as _,
                    context as _,
                    server_address,
                    client_address.map(|x| x.get()).unwrap_or(0),
                    len,
                )
            }
            xous_kernel::Message::MutableBorrow(msg) | xous_kernel::Message::Borrow(msg) => {
                let server_address = msg.buf.addr.get();
                let len = msg.buf.size.get();
                QueuedMessage::WaitingReturnMemory(
                    pid.get() as _,
                    context as _,
                    server_address,
                    client_address.map(|x| x.get()).unwrap_or(0),
                    len,
                )
            }
        };
        let idx = self.queue_head;
        self.queue_head += 1;
        if self.queue_head >= self.queue.len() {
            self.queue_head = 0;
        }
        Ok(idx)
    }
    // assert!(
    //     mem::size_of::<QueuedMessage>() == 32,
    //     "QueuedMessage was supposed to be 32 bytes, but instead was {} bytes",
    //     mem::size_of::<QueuedMessage>()
    // );

    /// Return a context ID that is available and blocking.  If no such context
    /// ID exists, or if this server isn't actually ready to receive packets,
    /// return None.
    pub fn take_available_thread(&mut self) -> Option<TID> {
        if self.ready_threads == 0 {
            return None;
        }
        let mut test_thread_mask = 1;
        let mut thread_number = 0;
        klog!("ready threads: 0b{:08b}", self.ready_threads);
        loop {
            // If the context mask matches this context number, remove it
            // and return the index.
            if self.ready_threads & test_thread_mask == test_thread_mask {
                self.ready_threads &= !test_thread_mask;
                return Some(thread_number);
            }
            // Advance to the next slot.
            test_thread_mask = test_thread_mask.rotate_left(1);
            thread_number += 1;
            if test_thread_mask == 1 {
                panic!("didn't find a free context, even though there should be one");
            }
        }
    }

    /// Return an available context to the blocking list.  This is part of the
    /// error condition when a message cannot be handled but the context has
    /// already been claimed.
    ///
    /// # Panics
    ///
    /// If the context cannot be returned because it is already blocking.
    pub fn return_available_thread(&mut self, tid: TID) {
        if self.ready_threads & 1 << tid != 0 {
            panic!(
                "tried to return context {}, but it was already blocking",
                tid
            );
        }
        self.ready_threads |= 1 << tid;
    }

    /// Add the given context to the list of ready and waiting contexts.
    pub fn park_thread(&mut self, tid: TID) {
        klog!("parking thread {}", tid);
        assert!(self.ready_threads & (1 << tid) == 0);
        self.ready_threads |= 1 << tid;
        klog!("ready threads now: {:08b}", self.ready_threads);
    }
}
