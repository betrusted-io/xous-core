use crate::arch;
use crate::arch::process::ProcessHandle;
use crate::irq::interrupt_claim;
use crate::mem::{MemoryManagerHandle, PAGE_SIZE};
use crate::server::{SenderID, WaitingMessage};
use crate::services::SystemServicesHandle;
use core::mem;
use xous::*;

/// This is the context that called SwitchTo
static mut SWITCHTO_CALLER: Option<(PID, CtxID)> = None;

pub fn handle(pid: PID, call: SysCall) -> core::result::Result<xous::Result, xous::Error> {
    // let pid = arch::current_pid();

    // println!("PID{} Syscall: {:?}", pid, call);
    match call {
        SysCall::MapMemory(phys, virt, size, req_flags) => {
            let mut mm = MemoryManagerHandle::get();
            let phys_ptr = phys
                .map(|x| x.get() as *mut usize)
                .unwrap_or(core::ptr::null_mut::<usize>());
            let virt_ptr = virt
                .map(|x| x.get() as *mut usize)
                .unwrap_or(core::ptr::null_mut::<usize>());

            // Don't let the address exceed the user area (unless it's PID 1)
            if pid != 1
                && virt
                    .map(|x| x.get() >= arch::mem::USER_AREA_END)
                    .unwrap_or(false)
            {
                return Err(xous::Error::BadAddress);

            // Don't allow mapping non-page values
            } else if size.get() & (PAGE_SIZE - 1) != 0 {
                // println!("map: bad alignment of size {:08x}", size);
                return Err(xous::Error::BadAlignment);
            }
            // println!(
            //     "Mapping {:08x} -> {:08x} ({} bytes, flags: {:?})",
            //     phys as u32, virt as u32, size, req_flags
            // );
            let range = mm.map_range(
                phys_ptr,
                virt_ptr,
                size.get(),
                pid,
                req_flags,
                MemoryType::Default,
            )?;

            // If we're handing back an address in main RAM, zero it out. If
            // phys is 0, then the page will be lazily allocated, so we
            // don't need to do this.
            if phys.is_some() {
                if mm.is_main_memory(phys_ptr) {
                    println!(
                        "Going to zero out {} bytes @ {:08x}",
                        range.size.get(),
                        range.addr.get()
                    );
                    unsafe {
                        range
                            .as_mut_ptr()
                            .write_bytes(0, range.size.get() / mem::size_of::<usize>())
                    };
                    // println!("Done zeroing out");
                }
                for offset in
                    (range.addr.get()..(range.addr.get() + range.size.get())).step_by(PAGE_SIZE)
                {
                    // println!("Handing page to user");
                    crate::arch::mem::hand_page_to_user(offset as *mut usize)
                        .expect("couldn't hand page to user");
                }
            }

            Ok(xous::Result::MemoryRange(range))
        }
        SysCall::UnmapMemory(virt, size) => {
            let mut mm = MemoryManagerHandle::get();
            let mut result = Ok(xous::Result::Ok);
            let virt = virt.get();
            let size = size.get();
            if virt & 0xfff != 0 {
                return Err(xous::Error::BadAlignment);
            }
            for addr in (virt..(virt + size)).step_by(PAGE_SIZE) {
                if let Err(e) = mm.unmap_page(addr as *mut usize) {
                    if result.is_ok() {
                        result = Err(e);
                    }
                }
            }
            result
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
        SysCall::SwitchTo(new_pid, new_context) => {
            let mut ss = SystemServicesHandle::get();
            let context_nr = ss.current_context_nr();
            unsafe {
                assert!(
                    SWITCHTO_CALLER.is_none(),
                    "SWITCHTO_CALLER was not None, indicating SwitchTo was called twice"
                );
                SWITCHTO_CALLER = Some((pid, context_nr));
            }
            ss.activate_process_context(new_pid, new_context, true, false)
                .map(|_ctx| {
                    // println!("switchto ({}, {})", pid, _ctx);
                    xous::Result::ResumeProcess
                })
        }
        SysCall::ClaimInterrupt(no, callback, arg) => {
            interrupt_claim(no, pid as definitions::PID, callback, arg).map(|_| xous::Result::Ok)
        }
        SysCall::Yield => {
            // If we're not running on bare metal, treat this as a no-op.
            if !cfg!(baremetal) {
                return Ok(xous::Result::Ok);
            }

            let (parent_pid, parent_ctx) = unsafe {
                SWITCHTO_CALLER
                    .take()
                    .expect("yielded when no parent context was present")
            };
            let mut ss = SystemServicesHandle::get();
            // let ppid = ss.get_process(pid).expect("can't get current process").ppid;
            // assert_ne!(ppid, 0, "no parent process id");
            ss.activate_process_context(parent_pid, parent_ctx, true, true)
                .map(|_| Ok(xous::Result::ResumeProcess))
                .unwrap_or(Err(xous::Error::ProcessNotFound))
        }
        SysCall::ReturnToParentI(_pid, _cpuid) => {
            let _ss = SystemServicesHandle::get();
            unsafe {
                let (_current_pid, _current_ctx) = crate::arch::irq::take_isr_return_pair()
                    .expect("couldn't get the isr return pair");
                // ss.ready_context(current_pid, current_ctx).unwrap();
                let (parent_pid, parent_ctx) = SWITCHTO_CALLER
                    .take()
                    .expect("ReturnToParentI called with no existing parent present");
                crate::arch::irq::set_isr_return_pair(parent_pid, parent_ctx);
            };
            Ok(xous::Result::ResumeProcess)
        }
        SysCall::ReceiveMessage(sid) => {
            // See if there is a pending message.  If so, return immediately.
            let mut ss = SystemServicesHandle::get();
            let context_nr = ss.current_context_nr();
            let sidx = ss.server_sidx(sid).ok_or(xous::Error::ServerNotFound)?;
            let server = ss
                .server_from_sidx(sidx)
                .ok_or(xous::Error::ServerNotFound)?;
            // server.print_queue();

            // Ensure the server is for this PID
            if server.pid != pid {
                return Err(xous::Error::ServerNotFound);
            }

            // If there is a pending message, return it immediately.
            if let Some(msg) = server.take_next_message(sidx) {
                // println!("PID {} had a message ready -- returning it", pid);
                return Ok(xous::Result::Message(msg));
            }

            // There is no pending message, so return control to the parent
            // process and mark ourselves as awaiting an event.  When a message
            // arrives, our return value will already be set to the
            // MessageEnvelope of the incoming message.
            println!(
                "PID {} did not have any waiting messages -- parking context {}",
                pid, context_nr
            );
            server.park_context(context_nr);

            // For baremetal targets, switch away from this process.
            if cfg!(baremetal) {
                unsafe { SWITCHTO_CALLER = None };

                let ppid = ss.get_process(pid).expect("Can't get current process").ppid;
                assert_ne!(ppid, 0, "no parent process id");
                ss.activate_process_context(ppid, 0, false, true)
                    .map(|_| Ok(xous::Result::ResumeProcess))
                    .unwrap_or(Err(xous::Error::ProcessNotFound))
            }
            // For hosted targets, simply return `BlockedProcess` indicating we'll make
            // a callback to their socket at a later time.
            else {
                ss.switch_from(pid, context_nr, false)
                    .map(|_| xous::Result::BlockedProcess)
            }
        }
        SysCall::WaitEvent => {
            let mut ss = SystemServicesHandle::get();
            let process = ss.get_process(pid).expect("Can't get current process");
            let ppid = process.ppid;
            unsafe { SWITCHTO_CALLER = None };
            assert_ne!(ppid, 0, "no parent process id");
            ss.activate_process_context(ppid, 0, false, true)
                .map(|_| Ok(xous::Result::ResumeProcess))
                .unwrap_or(Err(xous::Error::ProcessNotFound))
        }
        SysCall::SpawnThread(entrypoint, stack_pointer, argument) => {
            let mut ss = SystemServicesHandle::get();
            ss.spawn_thread(entrypoint, stack_pointer, argument)
                .map(xous::Result::ThreadID)
        }
        SysCall::CreateServer(name) => {
            let mut ss = SystemServicesHandle::get();
            ss.create_server(name).map(xous::Result::ServerID)
        }
        SysCall::Connect(sid) => {
            // ::debug_here::debug_here!();
            let mut ss = SystemServicesHandle::get();
            ss.connect_to_server(sid).map(xous::Result::ConnectionID)
        }
        SysCall::ReturnMemory(sender, addr, size) => {
            let mut ss = SystemServicesHandle::get();
            let sender = SenderID::from_usize(sender)?;

            let server = ss
                .server_from_sidx(sender.sidx)
                .ok_or(xous::Error::ServerNotFound)?;
            if server.pid != pid {
                return Err(xous::Error::ServerNotFound);
            }
            let result = server.take_waiting_message(sender.tidx)?;
            let (client_pid, client_ctx, server_addr, client_addr, len) = match result {
                WaitingMessage::BorrowedMemory(
                    client_pid,
                    client_ctx,
                    server_addr,
                    client_addr,
                    len,
                ) => (client_pid, client_ctx, server_addr, client_addr, len),
                WaitingMessage::MovedMemory => {
                    return Ok(xous::Result::Ok);
                }
                WaitingMessage::None => {
                    println!("WARNING: Tried to wait on a message that didn't exist");
                    return Err(xous::Error::ProcessNotFound);
                }
            };
            // println!(
            //     "Returning {} bytes from {:08x} in PID {} to {:08x} in PID {} in context {}",
            //     len,
            //     server_addr.get(),
            //     pid,
            //     client_addr.get(),
            //     client_pid,
            //     client_ctx
            // );

            // Return the memory to the calling process
            ss.return_memory(
                server_addr.get() as *mut usize,
                client_pid,
                client_addr.get() as *mut usize,
                len.get(),
            )?;

            // Unblock the client context to allow it to continue.
            // println!("Unblocking PID {} CTX {}", client_pid, client_ctx);
            ss.ready_context(client_pid, client_ctx)?;
            ss.set_context_result(client_pid, client_ctx, xous::Result::Ok)?;
            Ok(xous::Result::Ok)
        }
        SysCall::SendMessage(cid, message) => {
            let mut ss = SystemServicesHandle::get();
            let context_nr = ss.current_context_nr();
            let sidx = ss.sidx_from_cid(cid).ok_or(xous::Error::ServerNotFound)?;

            let server_pid = ss
                .server_from_sidx(sidx)
                .expect("server couldn't be located")
                .pid;

            // Remember the address the message came from, in case we need to
            // return it after the borrow is through.
            let client_address = match &message {
                Message::Scalar(_) => None,
                Message::Move(msg)
                | Message::MutableBorrow(msg)
                | Message::ImmutableBorrow(msg) => Some(msg.buf.addr),
            };

            // Translate memory messages from the client process to the server
            // process. Additionally, determine whether the call is blocking. If
            // so, switch to the server context right away.
            let (message, blocking) = match message {
                Message::Scalar(_) => (message, false),
                Message::Move(msg) => {
                    let new_virt = ss.send_memory(
                        msg.buf.as_mut_ptr(),
                        server_pid,
                        core::ptr::null_mut::<usize>(),
                        msg.buf.len(),
                    )?;
                    (
                        Message::Move(MemoryMessage {
                            id: msg.id,
                            buf: MemoryRange::new(new_virt as usize, msg.buf.len()),
                            offset: msg.offset,
                            valid: msg.valid,
                        }),
                        false,
                    )
                }
                Message::MutableBorrow(msg) => {
                    let new_virt = ss.lend_memory(
                        msg.buf.as_mut_ptr(),
                        server_pid,
                        core::ptr::null_mut::<usize>(),
                        msg.buf.len(),
                        true,
                    )?;
                    (
                        Message::MutableBorrow(MemoryMessage {
                            id: msg.id,
                            buf: MemoryRange::new(new_virt as usize, msg.buf.len()),
                            offset: msg.offset,
                            valid: msg.valid,
                        }),
                        true,
                    )
                }
                Message::ImmutableBorrow(msg) => {
                    let new_virt = ss.lend_memory(
                        msg.buf.as_mut_ptr(),
                        server_pid,
                        core::ptr::null_mut::<usize>(),
                        msg.buf.len(),
                        false,
                    )?;
                    (
                        Message::ImmutableBorrow(MemoryMessage {
                            id: msg.id,
                            buf: MemoryRange::new(new_virt as usize, msg.buf.len()),
                            offset: msg.offset,
                            valid: msg.valid,
                        }),
                        true,
                    )
                }
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
                let sender = match message {
                    Message::Scalar(_) | Message::Move(_) => 0,
                    Message::ImmutableBorrow(_) | Message::MutableBorrow(_) => ss
                        .remember_server_message(sidx, pid, context_nr, &message, client_address)
                        .or_else(|e| {
                            ss.server_from_sidx(sidx)
                                .expect("server couldn't be located")
                                .return_available_context(context_nr);
                            Err(e)
                        })?,
                };
                let envelope = MessageEnvelope { sender, message };

                // Mark the server's context as "Ready". If this fails, return the context
                // to the blocking list.
                ss.ready_context(server_pid, ctx_number).or_else(|e| {
                    ss.server_from_sidx(sidx)
                        .expect("server couldn't be located")
                        .return_available_context(context_nr);
                    Err(e)
                })?;

                if blocking && cfg!(baremetal) {
                    println!("Activating Server context and switching away from Client");
                    ss.activate_process_context(server_pid, ctx_number, !blocking, blocking)
                        .map(|_| Ok(xous::Result::Message(envelope)))
                        .unwrap_or(Err(xous::Error::ProcessNotFound))
                } else if blocking && !cfg!(baremetal) {
                    ss.set_context_result(server_pid, ctx_number, xous::Result::Message(envelope))
                        .map(|_| xous::Result::BlockedProcess)
                } else if cfg!(baremetal) {
                    println!("Setting the return value of the Server and returning to Client");
                    ss.set_context_result(server_pid, ctx_number, xous::Result::Message(envelope))
                        .map(|_| xous::Result::Ok)
                } else {
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
                ss.queue_server_message(sidx, pid, context_nr, message, client_address)?;

                // Park this context if it's blocking.  This is roughly
                // equivalent to a "Yield".
                if blocking {
                    // println!("Returning to parent");
                    let process = ss.get_process(pid).expect("Can't get current process");
                    let ppid = process.ppid;
                    unsafe { SWITCHTO_CALLER = None };
                    ss.activate_process_context(ppid, 0, !blocking, blocking)
                        .map(|_| Ok(xous::Result::ResumeProcess))
                        .unwrap_or(Err(xous::Error::ProcessNotFound))
                } else {
                    // println!("Returning to Client with Ok result");
                    Ok(xous::Result::Ok)
                }
            }
        }
        _ => Err(xous::Error::UnhandledSyscall),
    }
}
