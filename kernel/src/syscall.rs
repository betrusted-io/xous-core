use crate::arch;
use crate::arch::process::ProcessHandle;
use crate::irq::interrupt_claim;
use crate::mem::{MemoryManagerHandle, PAGE_SIZE};
use crate::services::SystemServicesHandle;
use core::mem;
use xous::*;

// extern "Rust" {
//     /// Allocates kernel structures for a new process, and returns the new PID.
//     /// This removes `page_count` page tables from the calling process at `origin_address`
//     /// and places them at `target_address`.
//     ///
//     /// If the process was created successfully, then the new PID is returned to
//     /// the calling process.  The child is not automatically scheduled for running.
//     ///
//     /// # Errors
//     ///
//     /// * **BadAlignment**: `origin_address` or `target_address` were not page-aligned,
//     ///                   or `address_size` was not a multiple of the page address size.
//     /// * **OutOfMemory**: The kernel couldn't allocate memory for the new process.
//     #[allow(dead_code)]
//     pub fn sys_process_spawn(
//         origin_address: MemoryAddress,
//         target_address: MemoryAddress,
//         address_size: MemorySize,
//     ) -> Result<PID, xous::Error>;

//     /// Interrupts the current process and returns control to the parent process.
//     ///
//     /// # Errors
//     ///
//     /// * **ProcessNotFound**: The provided PID doesn't exist, or is not running on the given CPU.
//     #[allow(dead_code)]
//     pub fn sysi_process_suspend(pid: PID, cpu_id: XousCpuId) -> Result<(), xous::Error>;

//     #[allow(dead_code)]
//     pub fn sys_process_resume(
//         process_id: PID,
//         stack_pointer: Option<usize>,
//         additional_contexts: &Option<&[XousContext]>,
//     ) -> Result<
//         (
//             Option<XousContext>,
//             Option<XousContext>,
//             Option<XousContext>,
//         ),
//         xous::Error,
//     >;

//     /// Causes a process to terminate immediately.
//     ///
//     /// It is recommended that this function only be called on processes that
//     /// have cleaned up after themselves, e.g. shut down any servers and
//     /// flushed any file descriptors.
//     ///
//     /// # Errors
//     ///
//     /// * **ProcessNotFound**: The requested process does not exist
//     /// * **ProcessNotChild**: The requested process is not our child process
//     #[allow(dead_code)]
//     pub fn sys_process_terminate(process_id: PID) -> Result<(), xous::Error>;

//     /// Equivalent to the Unix `sbrk` call.  Adjusts the
//     /// heap size to be equal to the specified value.  Heap
//     /// sizes start out at 0 bytes in new processes.
//     ///
//     /// # Errors
//     ///
//     /// * **OutOfMemory**: The region couldn't be extended.
//     #[allow(dead_code)]
//     pub fn sys_heap_resize(size: MemorySize) -> Result<(), xous::Error>;

//     ///! Message Passing Functions

//     /// Create a new server with the given name.  This enables other processes to
//     /// connect to this server to send messages.  Only one server name may exist
//     /// on a system at a time.
//     ///
//     /// # Errors
//     ///
//     /// * **ServerExists**: A server has already registered with that name
//     /// * **InvalidString**: The name was not a valid UTF-8 string
//     #[allow(dead_code)]
//     pub fn sys_server_create(server_name: usize) -> Result<XousSid, xous::Error>;

//     /// Suspend the current process until a message is received.  This thread will
//     /// block until a message is received.
//     ///
//     /// # Errors
//     ///
//     #[allow(dead_code)]
//     pub fn sys_server_receive(server_id: XousSid) -> Result<XousMessageReceived, xous::Error>;

//     /// Reply to a message received.  The thread will be unblocked, and will be
//     /// scheduled to run sometime in the future.
//     ///
//     /// If the message that we're responding to is a Memory message, then it should be
//     /// passed back directly to the destination without modification -- the actual contents
//     /// will be passed in the `out` address pointed to by the structure.
//     ///
//     /// # Errors
//     ///
//     /// * **ProcessTerminated**: The process we're replying to doesn't exist any more.
//     /// * **BadAddress**: The message didn't pass back all the memory it should have.
//     #[allow(dead_code)]
//     pub fn sys_server_reply(
//         destination: XousMessageSender,
//         message: XousMessage,
//     ) -> Result<(), xous::Error>;

//     /// Look up a server name and connect to it.
//     ///
//     /// # Errors
//     ///
//     /// * **ServerNotFound**: No server is registered with that name.
//     #[allow(dead_code)]
//     pub fn sys_client_connect(server_name: usize) -> Result<XousConnection, xous::Error>;

//     /// Send a message to a server.  This thread will block until the message is responded to.
//     /// If the message type is `Memory`, then the memory addresses pointed to will be
//     /// unavailable to this process until this function returns.
//     ///
//     /// # Errors
//     ///
//     /// * **ServerNotFound**: The server does not exist so the connection is now invalid
//     /// * **BadAddress**: The client tried to pass a Memory message using an address it doesn't own
//     /// * **Timeout**: The timeout limit has been reached
//     #[allow(dead_code)]
//     pub fn sys_client_send(
//         server: XousConnection,
//         message: XousMessage,
//     ) -> Result<XousMessage, xous::Error>;
// }

pub fn handle(call: SysCall) -> core::result::Result<xous::Result, xous::Error> {
    let pid = arch::current_pid();

    println!("PID{} Syscall: {:?}", pid, call);
    match call {
        SysCall::MapMemory(phys, virt, size, req_flags) => {
            let mut mm = MemoryManagerHandle::get();
            // Don't let the address exceed the user area (unless it's PID 1)
            if pid != 1 && (virt as usize) != 0 && (virt as usize) >= arch::mem::USER_AREA_END {
                return Err(xous::Error::BadAddress);

            // Don't allow mapping non-page values
            } else if size & (PAGE_SIZE - 1) != 0 {
                // println!("map: bad alignment of size {:08x}", size);
                return Err(xous::Error::BadAlignment);
            }
            // println!(
            //     "Mapping {:08x} -> {:08x} ({} bytes, flags: {:?})",
            //     phys as u32, virt as u32, size, req_flags
            // );
            let range = mm.map_range(phys, virt, size, pid, req_flags, MemoryType::Default)?;

            // If we're handing back an address in main RAM, zero it out. If
            // phys is 0, then the page will be lazily allocated, so we
            // don't need to do this.
            if phys as usize != 0 {
                if mm.is_main_memory(phys) {
                    println!(
                        "Going to zero out {} bytes @ {:08x}",
                        range.size, range.base as usize
                    );
                    unsafe {
                        range
                            .base
                            .write_bytes(0, range.size / mem::size_of::<usize>())
                    };
                    println!("Done zeroing out");
                }
                for offset in
                    ((range.base as usize)..(range.base as usize + range.size)).step_by(PAGE_SIZE)
                {
                    println!("Handing page to user");
                    crate::arch::mem::hand_page_to_user(offset as *mut usize)
                        .expect("couldn't hand page to user");
                }
            }

            Ok(xous::Result::MemoryRange(range))
        }
        SysCall::IncreaseHeap(delta, flags) => {
            if delta & 0xfff != 0 {
                return Err(xous::Error::BadAlignment);
            }
            let start = {
                let mut process = ProcessHandle::get();

                if process.inner.mem_heap_size + delta > process.inner.mem_heap_max {
                    return Err(xous::Error::OutOfMemory);
                }

                let start = process.inner.mem_heap_base + process.inner.mem_heap_size;
                process.inner.mem_heap_size += delta;
                start as *mut usize
            };
            let mut mm = MemoryManagerHandle::get();
            Ok(xous::Result::MemoryRange(
                mm.reserve_range(start, delta, flags)?,
            ))
        }
        SysCall::DecreaseHeap(delta) => {
            if delta & 0xfff != 0 {
                return Err(xous::Error::BadAlignment);
            }
            let start = {
                let mut process = ProcessHandle::get();

                if process.inner.mem_heap_size + delta > process.inner.mem_heap_max {
                    return Err(xous::Error::OutOfMemory);
                }

                let start = process.inner.mem_heap_base + process.inner.mem_heap_size;
                process.inner.mem_heap_size -= delta;
                start
            };
            let mut mm = MemoryManagerHandle::get();
            for page in ((start - delta)..start).step_by(crate::arch::mem::PAGE_SIZE) {
                mm.unmap_page(page as *mut usize)
                    .expect("unable to unmap page");
            }
            Ok(xous::Result::Ok)
        }
        SysCall::SwitchTo(pid, context) => {
            let mut ss = SystemServicesHandle::get();
            ss.activate_process_context(pid, context, true, false)
                .map(|ctx| {
                    // println!("switchto ({}, {})", pid, ctx);
                    xous::Result::ResumeProcess
                })
        }
        SysCall::ClaimInterrupt(no, callback, arg) => {
            interrupt_claim(no, pid as definitions::PID, callback, arg).map(|_| xous::Result::Ok)
        }
        SysCall::Yield => {
            let mut ss = SystemServicesHandle::get();
            let ppid = ss.get_process(pid).expect("can't get current process").ppid;
            assert_ne!(ppid, 0, "no parent process id");
            ss.activate_process_context(ppid, 0, true, true)
                .map(|_| Ok(xous::Result::ResumeProcess))
                .unwrap_or(Err(xous::Error::ProcessNotFound))
        }
        SysCall::ReceiveMessage(sid) => {
            let mut ss = SystemServicesHandle::get();
            let context_nr = ss.current_context_nr();
            // See if there is a pending message.  If so, return immediately.
            let server = ss.server_mut(sid).ok_or(xous::Error::ServerNotFound)?;

            // Ensure the server is for this PID
            if server.pid != pid {
                return Err(xous::Error::ServerNotFound);
            }

            // If there is a pending message, return it immediately.
            if let Some(msg) = server.take_next_message() {
                return Ok(xous::Result::Message(msg.0));
            }

            // There is no pending message, so return control to the parent process
            // and mark ourselves as awaiting an event.
            server.park_context(context_nr);

            let ppid = ss.get_process(pid).expect("Can't get current process").ppid;
            assert_ne!(ppid, 0, "no parent process id");
            ss.activate_process_context(ppid, 0, false, true)
                .map(|_| Ok(xous::Result::ResumeProcess))
                .unwrap_or(Err(xous::Error::ProcessNotFound))
        }
        SysCall::WaitEvent => {
            let mut ss = SystemServicesHandle::get();
            let process = ss.get_process(pid).expect("Can't get current process");
            let ppid = process.ppid;
            assert_ne!(ppid, 0, "no parent process id");
            ss.activate_process_context(ppid, 0, false, true)
                .map(|_| Ok(xous::Result::ResumeProcess))
                .unwrap_or(Err(xous::Error::ProcessNotFound))
        }
        SysCall::SpawnThread(entrypoint, stack_pointer, argument) => {
            let mut ss = SystemServicesHandle::get();
            ss.spawn_thread(entrypoint, stack_pointer, argument)
                .map(|ctx| xous::Result::ThreadID(ctx))
        }
        SysCall::CreateServer(name) => {
            let mut ss = SystemServicesHandle::get();
            ss.create_server(name).map(|x| xous::Result::ServerID(x))
        }
        SysCall::Connect(sid) => {
            let mut ss = SystemServicesHandle::get();
            ss.connect_to_server(sid)
                .map(|x| xous::Result::ConnectionID(x))
        }
        SysCall::SendMessage(cid, message) => {
            let mut ss = SystemServicesHandle::get();
            let sidx = ss.sidx_from_cid(cid).ok_or(xous::Error::ServerNotFound)?;

            let server_pid = ss
                .server_from_sidx(sidx)
                .expect("server couldn't be located")
                .pid;

            // Translate memory messages from the client process to the server
            // process. Additionally, determine whether the call is blocking.
            // If so, switch to the server context right away.
            let (message, blocking) = match message {
                Message::Scalar(_) => (message, false),
                Message::Move(msg) => {
                    let new_virt = if let Some(virt) = msg.buf {
                        if msg.buf_size.is_none() {
                            return Err(xous::Error::BadAddress);
                        }
                        let len = msg.buf_size.unwrap().get();
                        MemoryAddress::new(ss.send_memory(
                            virt.get() as *mut usize,
                            server_pid,
                            len,
                            true,
                            false,
                        )?)
                    } else {
                        None
                    };
                    (
                        Message::Move(MemoryMessage {
                            id: msg.id,
                            buf: new_virt,
                            buf_size: msg.buf_size,
                            offset: msg.offset,
                            valid: msg.valid,
                        }),
                        false,
                    )
                }
                Message::MutableBorrow(_) => unimplemented!(),
                Message::ImmutableBorrow(_) => unimplemented!(),
            };

            let envelope = MessageEnvelope {
                sender: pid as usize,
                message,
            };

            // If the server has an available context to receive the message,
            // transfer it right away.
            if let Some(ctx_number) = ss
                .server_from_sidx(sidx)
                .expect("server couldn't be located")
                .take_available_context()
            {
                println!(
                    "There are contexts available to handle this message.  Marking PID {} as Ready",
                    server_pid
                );
                ss.ready_context(server_pid, ctx_number)?;
                if blocking {
                    println!("Activating Server context and switching away from Client");
                    ss.activate_process_context(server_pid, ctx_number, !blocking, blocking)
                        .map(|_| Ok(xous::Result::Message(envelope)))
                        .unwrap_or(Err(xous::Error::ProcessNotFound))
                } else {
                    println!("Setting the return value of the Server and returning to Client");
                    ss.set_context_result(server_pid, ctx_number, xous::Result::Message(envelope))
                        .map(|_| xous::Result::Ok)
                }
            } else {
                println!("No contexts available to handle this.  Queueing message.");
                // There is no server context we can use, so add the message to
                // the queue.
                let context_nr = ss.current_context_nr();

                // Add this message to the queue.  If the queue is full, this
                // returns an error.
                let server = ss
                    .server_from_sidx(sidx)
                    .ok_or(xous::Error::ServerNotFound)?;
                ss.queue_server_message(sidx, context_nr, envelope)?;

                // Park this context if it's blocking.  This is roughly
                // equivalent to a "Yield".
                if blocking {
                    println!("Returning to parent");
                    let process = ss.get_process(pid).expect("Can't get current process");
                    let ppid = process.ppid;
                    ss.activate_process_context(ppid, 0, !blocking, blocking)
                        .map(|_| Ok(xous::Result::ResumeProcess))
                        .unwrap_or(Err(xous::Error::ProcessNotFound))
                } else {
                    println!("Returning to Client with Ok result");
                    Ok(xous::Result::Ok)
                }
            }
        }
        _ => Err(xous::Error::UnhandledSyscall),
    }
}
