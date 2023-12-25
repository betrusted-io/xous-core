// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use crate::{mem::MemoryManager, services::SystemServices};
use core::mem;
use xous_kernel::{MemoryAddress, MemoryRange, MemorySize, Message, MessageSender, PID, SID, TID};

/// A pointer to resolve a server ID to a particular process
#[derive(PartialEq, Debug)]
pub struct Server {
    /// A randomly-generated ID
    pub sid: SID,

    /// The process that owns this server
    pub pid: PID,

    /// Where messages should be inserted
    queue_head: usize,

    /// The index that the server is currently reading from
    queue_tail: usize,

    /// An increasing number that indicates where the server is reading.
    head_generation: u8,

    /// An increasing (but wrapping number) that indicates where clients are writing.
    tail_generation: u8,

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

impl From<SenderID> for usize {
    fn from(val: SenderID) -> Self {
        (val.pid.map(|x| x.get() as usize).unwrap_or(0) << 24)
            | ((val.sidx << 16) & 0x00ff0000)
            | (val.idx & 0xffff)
    }
}

impl From<MessageSender> for SenderID {
    fn from(item: MessageSender) -> SenderID {
        SenderID::from(item.to_usize())
    }
}

impl From<SenderID> for MessageSender {
    fn from(val: SenderID) -> Self {
        MessageSender::from_usize(val.into())
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
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* reserved */
        usize, /* id */
        usize, /* arg1 */
        usize, /* arg2 */
        usize, /* arg3 */
        usize, /* arg4 */
    ),
    ScalarMessage(
        u16,   /* client PID */
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* reserved */
        usize, /* id */
        usize, /* arg1 */
        usize, /* arg2 */
        usize, /* arg3 */
        usize, /* arg4 */
    ),
    MemoryMessageSend(
        u16,   /* client PID */
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* client memory address */
        usize, /* id */
        usize, /* server memory address */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),
    MemoryMessageROLend(
        u16,   /* client PID */
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* client memory address */
        usize, /* id */
        usize, /* server memory address */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),
    MemoryMessageRWLend(
        u16,   /* client PID */
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* client memory address */
        usize, /* id */
        usize, /* server memory address */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),

    /// The process lending this memory terminated before
    /// we could receive the message.
    MemoryMessageROLendTerminated(
        u16,   /* client PID */
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* client memory address */
        usize, /* id */
        usize, /* server memory address */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),

    /// The process lending this memory terminated before
    /// we could receive the message.
    MemoryMessageRWLendTerminated(
        u16,   /* client PID */
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* client memory address */
        usize, /* id */
        usize, /* server memory address */
        usize, /* buf_size */
        usize, /* offset */
        usize, /* valid */
    ),

    /// The process waiting for the response terminated before
    /// we could receive the message.
    BlockingScalarTerminated(
        u16,   /* client PID */
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* reserved */
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
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* address of memory base in server */
        usize, /* client base address */
        usize, /* Range size */
    ),

    /// When a server goes away, its memory must be forgotten instead of being returned
    /// to the previous process.
    WaitingForget(
        u16,   /* client PID */
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* address of memory base in server */
        usize, /* client base address */
        usize, /* Range size */
    ),

    /// This is the state when a message is blocking, but has no associated memory
    /// page.
    WaitingReturnScalar(
        u16,   /* client PID */
        u8,    /* client TID */
        u8,    /* message index */
        usize, /* server return address */
    ),
}

impl QueuedMessage {
    /// Return `true` if this Queued Message is sitting inside of the Server, and
    /// is therefore waiting to be returned.
    /// This only indicates messages that have been seen by the Server and have
    /// not yet been responded to -- Messages that have not yet been seen by the
    /// Server will return `false`.
    fn is_in_server(&self) -> bool {
        matches!(
            self,
            &QueuedMessage::WaitingForget(_, _, _, _, _, _)
                | &QueuedMessage::WaitingReturnMemory(_, _, _, _, _, _)
                | &QueuedMessage::WaitingReturnScalar(_, _, _, _)
        )
    }
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
            head_generation: 0,
            tail_generation: 0,
            queue,
            ready_threads: 0,
        });
        Ok(())
    }

    /// Take a current slot and replace it with `None`, clearing out the contents of the queue.
    /// Returns an error if the queue has any waiting elements.
    /// Returns a list of threads that should be readied.
    pub fn destroy(mut self, ss: &mut SystemServices) -> Result<(), Self> {
        // First determine if the server has any Waiting messages. That is, any messages
        // that are currently sitting in the memory space of the Server. These cannot
        // safely be handled, and will cause an error if we try to mess with them.
        for queue_entry in self.queue.iter() {
            if queue_entry.is_in_server() {
                return Err(self);
            }
        }

        // We now know there will be no problems in shutting down this server. Look
        // through the queue and respond to each message in turn.
        for entry in self.queue.iter_mut() {
            match *entry {
                // If there are `Waiting` messages, then something is seriously wrong because
                // we already determined above that this wouldn't happen.
                QueuedMessage::WaitingForget(_, _, _, _, _, _)
                | QueuedMessage::WaitingReturnMemory(_, _, _, _, _, _)
                | QueuedMessage::WaitingReturnScalar(_, _, _, _) => panic!("message was waiting"),

                // For `Empty` and `Scalar` messages, all we have to do is ignore them.
                // The sending process will not be blocked. These messages will be dropped,
                // and the server will never see them.
                QueuedMessage::Empty | QueuedMessage::ScalarMessage(_, _, _, _, _, _, _, _, _) => {}

                // For `Send` messages, the Server has not yet seen these messages. Simply
                // prevent this memory from getting mapped into the Server and free it.
                QueuedMessage::MemoryMessageSend(
                    _pid,
                    _tid,
                    _idx,
                    _client_memory_addr,
                    _id,
                    server_memory_addr,
                    memory_length,
                    _offset,
                    _valid,
                ) => {
                    MemoryManager::with_mut(|mm| {
                        let mut result = Ok(xous_kernel::Result::Ok);
                        let virt = server_memory_addr;
                        let size = memory_length;
                        if !cfg!(baremetal) && virt & 0xfff != 0 {
                            return Err(xous_kernel::Error::BadAlignment);
                        }
                        for addr in (virt..(virt + size)).step_by(crate::mem::PAGE_SIZE) {
                            if let Err(e) = mm.unmap_page(addr as *mut usize) {
                                if result.is_ok() {
                                    result = Err(e);
                                }
                            }
                        }
                        result
                    })
                    .unwrap();
                }

                // For BlockingScalar messages, the client is waiting for a response.
                // Unblock the client and return an error indicating the server does
                // not exist.
                QueuedMessage::BlockingScalarTerminated(pid, tid, _, _, _, _, _, _, _)
                | QueuedMessage::BlockingScalarMessage(pid, tid, _, _, _, _, _, _, _) => {
                    let pid = PID::new(pid as _).unwrap();
                    let tid = tid as _;

                    // Set the return value of the specified thread.
                    ss.set_thread_result(
                        pid,
                        tid,
                        xous_kernel::Result::Error(xous_kernel::Error::ServerNotFound),
                    )
                    .unwrap();

                    // Mark it as ready to run.
                    ss.ready_thread(pid, tid).unwrap();
                }

                QueuedMessage::MemoryMessageROLend(
                    client_pid,
                    client_tid,
                    _idx,
                    client_addr,
                    _id,
                    server_addr,
                    buf_size,
                    _,
                    _,
                )
                | QueuedMessage::MemoryMessageRWLend(
                    client_pid,
                    client_tid,
                    _idx,
                    client_addr,
                    _id,
                    server_addr,
                    buf_size,
                    _,
                    _,
                )
                | QueuedMessage::MemoryMessageROLendTerminated(
                    client_pid,
                    client_tid,
                    _idx,
                    client_addr,
                    _id,
                    server_addr,
                    buf_size,
                    _,
                    _,
                )
                | QueuedMessage::MemoryMessageRWLendTerminated(
                    client_pid,
                    client_tid,
                    _idx,
                    client_addr,
                    _id,
                    server_addr,
                    buf_size,
                    _,
                    _,
                ) => {
                    let client_pid = PID::new(client_pid as _).unwrap();
                    let client_tid = client_tid as _;
                    // Return the memory to the calling process
                    ss.return_memory(
                        server_addr as *mut usize,
                        client_pid,
                        client_tid,
                        client_addr as _,
                        buf_size,
                    )
                    .unwrap();
                    ss.ready_thread(client_pid, client_tid).unwrap();
                    ss.set_thread_result(
                        client_pid,
                        client_tid,
                        xous_kernel::Result::Error(xous_kernel::Error::ServerNotFound),
                    )
                    .unwrap();
                }
            }
            *entry = QueuedMessage::Empty;
        }

        let server_pid = ss.current_pid();

        // Finally, wake up all threads that are waiting on this Server.
        while let Some(server_tid) = self.take_available_thread() {
            ss.ready_thread(server_pid, server_tid).unwrap();
            ss.set_thread_result(
                server_pid,
                server_tid,
                xous_kernel::Result::Error(xous_kernel::Error::ServerNotFound),
            )
            .unwrap();
        }

        // Release the backing memory
        #[cfg(baremetal)]
        MemoryManager::with_mut(|mm| {
            let virt = self.queue.as_mut_ptr() as usize;
            let size = self.queue.len();
            for addr in (virt..(virt + size)).step_by(crate::arch::mem::PAGE_SIZE) {
                mm.unmap_page(addr as *mut usize).unwrap();
            }
        });

        // The server should now be destroyed.
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
                    idx,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                    arg5,
                    arg6,
                ) => {
                    if msg_pid == pid.get() as _ {
                        *entry = QueuedMessage::MemoryMessageROLendTerminated(
                            msg_pid, tid, idx, arg1, arg2, arg3, arg4, arg5, arg6,
                        );
                    }
                }
                QueuedMessage::MemoryMessageRWLend(
                    msg_pid,
                    tid,
                    idx,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                    arg5,
                    arg6,
                ) => {
                    if msg_pid == pid.get() as _ {
                        *entry = QueuedMessage::MemoryMessageRWLendTerminated(
                            msg_pid, tid, idx, arg1, arg2, arg3, arg4, arg5, arg6,
                        );
                    }
                }
                QueuedMessage::BlockingScalarMessage(
                    msg_pid,
                    tid,
                    idx,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                    arg5,
                    arg6,
                ) => {
                    if msg_pid == pid.get() as _ {
                        *entry = QueuedMessage::BlockingScalarTerminated(
                            msg_pid, tid, idx, arg1, arg2, arg3, arg4, arg5, arg6,
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
        message_index: usize,
        buf: Option<&MemoryRange>,
    ) -> Result<WaitingMessage, xous_kernel::Error> {
        // klog!("head generation: {}  tail generation: {}", self.head_generation, self.tail_generation);
        // if self.tail_generation == self.head_generation {
        //     Err(xous_kernel::Error::BadAddress)?;
        // }

        let current_val = self
            .queue
            .get_mut(message_index)
            .ok_or(xous_kernel::Error::BadAddress)?;
        // klog!("memory in queue[{}]: {:?}", message_index, current_val);
        let (pid, tid, _idx, server_addr, client_addr, len, forget, is_memory) = match *current_val
        {
            QueuedMessage::WaitingReturnMemory(pid, tid, idx, server_addr, client_addr, len) => {
                (pid, tid, idx, server_addr, client_addr, len, false, true)
            }
            QueuedMessage::WaitingForget(pid, tid, idx, server_addr, client_addr, len) => {
                (pid, tid, idx, server_addr, client_addr, len, true, true)
            }
            QueuedMessage::WaitingReturnScalar(pid, tid, idx, return_address) => {
                (pid, tid, idx, return_address, 0, 0, true, false)
            }
            _ => return Ok(WaitingMessage::None),
        };

        // Sanity check the specified address was correct, and matches what we
        // had cached.
        if is_memory && cfg!(baremetal) && buf.is_some() {
            let buf = buf.expect("memory message expected but no buffer passed!");
            if server_addr != buf.as_ptr() as usize || len != buf.len() {
                // klog!("Memory is attached but the returned buffer doesn't match (len: {} vs {}), buf addr: {:08x} vs {:08x}", len, buf.len(), server_addr, buf.as_ptr() as usize);
                return Err(xous_kernel::Error::BadAddress);
            }
        }
        *current_val = QueuedMessage::Empty;
        if message_index == self.queue_tail {
            self.queue_tail += 1;
            if self.queue_tail >= self.queue.len() {
                self.queue_tail = 0;
            }
            // Advance the pointer in case we have a long string of Empty messages.
            while self.queue_tail != self.queue_head
                && self.queue[self.queue_tail] == QueuedMessage::Empty
            {
                self.queue_tail += 1;
                if self.queue_tail >= self.queue.len() {
                    self.queue_tail = 0;
                }
            }
        }

        // Destructure the PID and context ID from the `pid_tid` field
        // klog!(
        //     "taking waiting message and returning to pid: {} tid: {}",
        //     pid,
        //     tid
        // );

        if !is_memory {
            return Ok(WaitingMessage::ScalarMessage(
                PID::new(pid as _).unwrap(),
                tid as _,
            ));
        }

        if forget {
            return Ok(WaitingMessage::ForgetMemory(unsafe {
                MemoryRange::new(server_addr, len)
            }?));
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
        // klog!(
        //     "queue_head: ((({})))  queue_tail: ((({}))): {:?}  CID: ((({})))  head_gen: {}  tail_gen: {}",
        //     self.queue_head,
        //     self.queue_tail,
        //     self.queue[self.queue_tail],
        //     sidx,
        //     self.head_generation,
        //     self.tail_generation,
        // );

        // If the reading head and tail generations are the same, the queue is empty.
        if self.tail_generation == self.head_generation {
            // klog!("self.tail_generation {} == self.head_generation {}", self.tail_generation, self.head_generation);
            return None;
        }

        use core::convert::TryInto;
        let mut queue_idx = self.queue_tail;
        loop {
            let mut sender = SenderID::new(sidx, queue_idx, None);
            // klog!("Message @ server.queue[{}]: {:?}", queue_idx, self.queue[queue_idx]);
            let (result, response) = match self.queue[queue_idx] {
                QueuedMessage::MemoryMessageROLend(
                    pid,
                    tid,
                    idx,
                    client_addr,
                    id,
                    server_addr,
                    buf_size,
                    offset,
                    valid,
                ) if idx == self.head_generation => {
                    sender.pid = PID::new(pid.try_into().unwrap());
                    (
                        xous_kernel::MessageEnvelope {
                            sender: sender.into(),
                            body: xous_kernel::Message::Borrow(xous_kernel::MemoryMessage {
                                id,
                                buf: unsafe { MemoryRange::new(server_addr, buf_size).ok() }?,
                                offset: MemorySize::new(offset),
                                valid: MemorySize::new(valid),
                            }),
                        },
                        QueuedMessage::WaitingReturnMemory(
                            pid,
                            tid,
                            idx,
                            server_addr,
                            client_addr,
                            buf_size,
                        ),
                    )
                }
                QueuedMessage::MemoryMessageRWLend(
                    pid,
                    tid,
                    idx,
                    client_addr,
                    id,
                    server_addr,
                    buf_size,
                    offset,
                    valid,
                ) if idx == self.head_generation => {
                    sender.pid = PID::new(pid.try_into().unwrap());
                    (
                        xous_kernel::MessageEnvelope {
                            sender: sender.into(),
                            body: xous_kernel::Message::MutableBorrow(xous_kernel::MemoryMessage {
                                id,
                                buf: unsafe { MemoryRange::new(server_addr, buf_size).ok() }?,
                                offset: MemorySize::new(offset),
                                valid: MemorySize::new(valid),
                            }),
                        },
                        QueuedMessage::WaitingReturnMemory(
                            pid,
                            tid,
                            idx,
                            server_addr,
                            client_addr,
                            buf_size,
                        ),
                    )
                }
                QueuedMessage::MemoryMessageROLendTerminated(
                    pid,
                    tid,
                    idx,
                    client_addr,
                    id,
                    server_addr,
                    buf_size,
                    offset,
                    valid,
                ) if idx == self.head_generation => {
                    sender.pid = PID::new(pid.try_into().unwrap());
                    (
                        xous_kernel::MessageEnvelope {
                            sender: sender.into(),
                            body: xous_kernel::Message::Borrow(xous_kernel::MemoryMessage {
                                id,
                                buf: unsafe { MemoryRange::new(server_addr, buf_size).ok() }?,
                                offset: MemorySize::new(offset),
                                valid: MemorySize::new(valid),
                            }),
                        },
                        QueuedMessage::WaitingReturnMemory(
                            pid,
                            tid,
                            idx,
                            server_addr,
                            client_addr,
                            buf_size,
                        ),
                    )
                }
                QueuedMessage::MemoryMessageRWLendTerminated(
                    pid,
                    tid,
                    idx,
                    client_addr,
                    id,
                    server_addr,
                    buf_size,
                    offset,
                    valid,
                ) if idx == self.head_generation => {
                    sender.pid = PID::new(pid.try_into().unwrap());
                    (
                        xous_kernel::MessageEnvelope {
                            sender: sender.into(),
                            body: xous_kernel::Message::MutableBorrow(xous_kernel::MemoryMessage {
                                id,
                                buf: unsafe { MemoryRange::new(server_addr, buf_size).ok() }?,
                                offset: MemorySize::new(offset),
                                valid: MemorySize::new(valid),
                            }),
                        },
                        QueuedMessage::WaitingReturnMemory(
                            pid,
                            tid,
                            idx,
                            server_addr,
                            client_addr,
                            buf_size,
                        ),
                    )
                }

                QueuedMessage::BlockingScalarMessage(
                    pid,
                    tid,
                    idx,
                    client_addr,
                    id,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                ) if idx == self.head_generation => {
                    sender.pid = PID::new(pid.try_into().unwrap());
                    (
                        xous_kernel::MessageEnvelope {
                            sender: sender.into(),
                            body: xous_kernel::Message::BlockingScalar(
                                xous_kernel::ScalarMessage {
                                    id,
                                    arg1,
                                    arg2,
                                    arg3,
                                    arg4,
                                },
                            ),
                        },
                        QueuedMessage::WaitingReturnScalar(pid, tid, idx, client_addr),
                    )
                }
                QueuedMessage::MemoryMessageSend(
                    pid,
                    _tid,
                    idx,
                    _reserved,
                    id,
                    server_addr,
                    buf_size,
                    offset,
                    valid,
                ) if idx == self.head_generation => {
                    sender.pid = PID::new(pid.try_into().unwrap());
                    let msg = xous_kernel::MessageEnvelope {
                        sender: sender.into(),
                        body: xous_kernel::Message::Move(xous_kernel::MemoryMessage {
                            id,
                            buf: unsafe { MemoryRange::new(server_addr, buf_size).ok() }?,
                            offset: MemorySize::new(offset),
                            valid: MemorySize::new(valid),
                        }),
                    };
                    self.queue[queue_idx] = QueuedMessage::Empty;
                    if queue_idx == self.queue_tail {
                        self.queue_tail += 1;
                        if self.queue_tail >= self.queue.len() {
                            self.queue_tail = 0;
                        }
                    }
                    self.head_generation = self.head_generation.wrapping_add(1);
                    return Some(msg);
                }

                // Scalar messages have nothing to return, so they can go straight to the `Free` state
                QueuedMessage::ScalarMessage(
                    pid,
                    _tid,
                    idx,
                    _reserved,
                    id,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                ) if idx == self.head_generation => {
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
                    self.queue[queue_idx] = QueuedMessage::Empty;
                    if queue_idx == self.queue_tail {
                        self.queue_tail += 1;
                        if self.queue_tail >= self.queue.len() {
                            self.queue_tail = 0;
                        }
                    }
                    self.head_generation = self.head_generation.wrapping_add(1);
                    return Some(msg);
                }
                QueuedMessage::BlockingScalarTerminated(
                    pid,
                    _tid,
                    idx,
                    _reserved,
                    id,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                ) if idx == self.head_generation => {
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
                    self.queue[queue_idx] = QueuedMessage::Empty;
                    if queue_idx == self.queue_tail {
                        self.queue_tail += 1;
                        if self.queue_tail >= self.queue.len() {
                            self.queue_tail = 0;
                        }
                    }
                    self.head_generation = self.head_generation.wrapping_add(1);
                    return Some(msg);
                }
                _ => {
                    queue_idx += 1;
                    if queue_idx >= self.queue.len() {
                        queue_idx = 0;
                    }
                    if queue_idx == self.queue_tail {
                        // panic!("Couldn't find the message in the queue! Queue contents: {:?}", self.queue);
                        return None;
                    }
                    continue;
                }
            };

            if queue_idx == self.queue_tail {
                self.queue_tail += 1;
                if self.queue_tail >= self.queue.len() {
                    self.queue_tail = 0;
                }
            }
            self.queue[queue_idx] = response;
            self.head_generation = self.head_generation.wrapping_add(1);
            return Some(result);
        }
    }

    /// Add the given message to this server's queue.
    ///
    /// # Errors
    ///
    /// * **ServerQueueFull**: The server queue cannot accept any more messages
    pub fn queue_message(
        &mut self,
        pid: PID,
        tid: TID,
        message: xous_kernel::Message,
        original_address: Option<MemoryAddress>,
    ) -> core::result::Result<usize, xous_kernel::Error> {
        // klog!(
        //     "Queueing message: {:?} from pid: {}  tid: {}",
        //     message,
        //     pid.get(),
        //     tid
        // );
        // If the head and the tail generations will end up the same, then
        // the queue is full.
        if self.tail_generation == self.head_generation.wrapping_sub(1) {
            return Err(xous_kernel::Error::ServerQueueFull);
        }

        // Look through the queue, beginning at the queue head, for an empty slot.
        let mut discovered_index = None;
        for queue_idx in self.queue_head..self.queue.len() {
            if self.queue[queue_idx] == QueuedMessage::Empty {
                discovered_index = Some(queue_idx);
                break;
            }
        }
        if discovered_index.is_none() {
            for queue_idx in 0..self.queue_head {
                if self.queue[queue_idx] == QueuedMessage::Empty {
                    discovered_index = Some(queue_idx);
                    break;
                }
            }
        }
        if discovered_index.is_none() {
            return Err(xous_kernel::Error::ServerQueueFull);
        }
        let queue_idx = discovered_index.unwrap();
        let queue_entry = &mut self.queue[queue_idx];
        *queue_entry = match message {
            xous_kernel::Message::Scalar(msg) => QueuedMessage::ScalarMessage(
                pid.get() as _,
                tid as _,
                self.tail_generation,
                0,
                msg.id,
                msg.arg1,
                msg.arg2,
                msg.arg3,
                msg.arg4,
            ),
            xous_kernel::Message::BlockingScalar(msg) => QueuedMessage::BlockingScalarMessage(
                pid.get() as _,
                tid as _,
                self.tail_generation,
                0,
                msg.id,
                msg.arg1,
                msg.arg2,
                msg.arg3,
                msg.arg4,
            ),
            xous_kernel::Message::Move(msg) => QueuedMessage::MemoryMessageSend(
                pid.get() as _,
                tid as _,
                self.tail_generation,
                original_address.map(|x| x.get()).unwrap_or(0),
                msg.id,
                msg.buf.as_ptr() as _,
                msg.buf.len(),
                msg.offset.map(|x| x.get()).unwrap_or(0) as usize,
                msg.valid.map(|x| x.get()).unwrap_or(0) as usize,
            ),
            xous_kernel::Message::MutableBorrow(msg) => QueuedMessage::MemoryMessageRWLend(
                pid.get() as _,
                tid as _,
                self.tail_generation,
                original_address.map(|x| x.get()).unwrap_or(0),
                msg.id,
                msg.buf.as_ptr() as _,
                msg.buf.len(),
                msg.offset.map(|x| x.get()).unwrap_or(0) as usize,
                msg.valid.map(|x| x.get()).unwrap_or(0) as usize,
            ),
            xous_kernel::Message::Borrow(msg) => QueuedMessage::MemoryMessageROLend(
                pid.get() as _,
                tid as _,
                self.tail_generation,
                original_address.map(|x| x.get()).unwrap_or(0),
                msg.id,
                msg.buf.as_ptr() as _,
                msg.buf.len(),
                msg.offset.map(|x| x.get()).unwrap_or(0) as usize,
                msg.valid.map(|x| x.get()).unwrap_or(0) as usize,
            ),
        };

        // Advance the tail generation, which is used for incoming messages to keep
        // them in sequence.
        self.tail_generation = self.tail_generation.wrapping_add(1);
        if queue_idx == self.queue_head {
            self.queue_head += 1;
            if self.queue_head >= self.queue.len() {
                self.queue_head = 0;
            }
        }
        // klog!(
        //     "queue head: {}  queue_tail: {}",
        //     self.queue_head,
        //     self.queue_tail
        // );
        Ok(queue_idx)
    }

    pub fn queue_response(
        &mut self,
        pid: PID,
        tid: TID,
        message: &Message,
        client_address: Option<MemoryAddress>,
    ) -> core::result::Result<usize, xous_kernel::Error> {
        // klog!("Queueing address message: {:?} (pid: {} tid: {}) tail_gen: {}  head_gen: {}", message, pid.get(), tid, self.tail_generation, self.head_generation);
        let mut queue_idx = self.queue_head;
        loop {
            if self.queue[queue_idx] == QueuedMessage::Empty {
                break;
            }
            queue_idx += 1;
            if queue_idx >= self.queue.len() {
                queue_idx = 0;
            }
            if queue_idx == self.queue_head {
                return Err(xous_kernel::Error::ServerQueueFull);
            }
        }
        self.queue[queue_idx] = match message {
            xous_kernel::Message::Scalar(_) | xous_kernel::Message::BlockingScalar(_) => {
                QueuedMessage::WaitingReturnScalar(
                    pid.get() as _,
                    tid as _,
                    0,
                    client_address.map(|x| x.get()).unwrap_or(0),
                )
            }
            xous_kernel::Message::Move(msg) => {
                let server_address = msg.buf.as_ptr() as _;
                let len = msg.buf.len();
                QueuedMessage::WaitingForget(
                    pid.get() as _,
                    tid as _,
                    0,
                    server_address,
                    client_address.map(|x| x.get()).unwrap_or(0),
                    len,
                )
            }
            xous_kernel::Message::MutableBorrow(msg) | xous_kernel::Message::Borrow(msg) => {
                let server_address = msg.buf.as_ptr() as _;
                let len = msg.buf.len();
                QueuedMessage::WaitingReturnMemory(
                    pid.get() as _,
                    tid as _,
                    0,
                    server_address,
                    client_address.map(|x| x.get()).unwrap_or(0),
                    len,
                )
            }
        };
        if queue_idx == self.queue_head {
            self.queue_head += 1;
            if self.queue_head >= self.queue.len() {
                self.queue_head = 0;
            }
        }
        Ok(queue_idx)
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
