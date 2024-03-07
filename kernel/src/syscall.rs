// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering::Relaxed};

use xous_kernel::*;

use crate::arch;
use crate::arch::process::Process as ArchProcess;
use crate::irq::{interrupt_claim, interrupt_free};
use crate::mem::{MemoryManager, PAGE_SIZE};
use crate::server::{SenderID, WaitingMessage};
use crate::services::SystemServices;
#[cfg(feature = "swap")]
use crate::swap::Swap;

/* Quoth Xobs:
 The idea behind SWITCHTO_CALLER was that you'd have a process act as a scheduler,
 where it would know all of its children processes. It would call SwitchTo(pid, tid)
 on its children, which would call Yield or WaitEvent as necessary that would then
 cause execution to return to the parent process.

 If the timer hit, it would call ReturnToParent() which would also return to the caller.

 Currently (as of Mar 2021) this functionality isn't being used, it's just returning
 back to the kernel, e.g. (PID,TID) = (1,1)
*/
/// This is the PID/TID of the last person that called SwitchTo
static mut SWITCHTO_CALLER: Option<(PID, TID)> = None;

/// When a process is switched to, take note of the original PID and TID.
/// That way we know whether to give the process its full quantum when
/// messages are Returned. If a process Returns messages while it's in its
/// own quantum, then don't immediately transfer control to the Client.
/// However, for processes that are running on Borrowed Quantum (i.e. when
/// another process sent them a message and they're immediately responding,)
/// return control to the Client.
static ORIGINAL_PID: AtomicU8 = AtomicU8::new(2);
static ORIGINAL_TID: AtomicUsize = AtomicUsize::new(2);

#[derive(PartialEq)]
enum ExecutionType {
    Blocking,
    NonBlocking,
}

#[cfg(baremetal)]
pub fn reset_switchto_caller() { unsafe { SWITCHTO_CALLER = None }; }

fn retry_syscall(pid: PID, tid: TID) -> SysCallResult {
    if cfg!(baremetal) {
        arch::process::Process::with_current_mut(|p| p.retry_instruction(tid))?;
        do_yield(pid, tid)
    } else {
        Ok(xous_kernel::Result::RetryCall)
    }
}

fn do_yield(_pid: PID, tid: TID) -> SysCallResult {
    // If we're not running on bare metal, treat this as a no-op.
    if !cfg!(baremetal) {
        return Ok(xous_kernel::Result::Ok);
    }

    let (parent_pid, parent_ctx) =
        unsafe { SWITCHTO_CALLER.take().expect("yielded when no parent context was present") };
    //println!("\n\r ***YIELD CALLED***");
    SystemServices::with_mut(|ss| {
        // TODO: Advance thread
        let result = ss
            .activate_process_thread(tid, parent_pid, parent_ctx, true)
            .map(|_| Ok(xous_kernel::Result::ResumeProcess))
            .unwrap_or(Err(xous_kernel::Error::ProcessNotFound));

        ss.set_last_thread(PID::new(ORIGINAL_PID.load(Relaxed)).unwrap(), ORIGINAL_TID.load(Relaxed)).ok();
        result
    })
}

fn send_message(pid: PID, tid: TID, cid: CID, message: Message) -> SysCallResult {
    SystemServices::with_mut(|ss| {
        let sidx = ss.sidx_from_cid(cid).ok_or(xous_kernel::Error::ServerNotFound)?;

        let server_pid = ss.server_from_sidx(sidx).expect("server couldn't be located").pid;

        // Remember the address the message came from, in case we need to
        // return it after the borrow is through.
        let client_address = match &message {
            Message::Scalar(_) | Message::BlockingScalar(_) => None,
            Message::Move(msg) | Message::MutableBorrow(msg) | Message::Borrow(msg) => {
                MemoryAddress::new(msg.buf.as_ptr() as _)
            }
        };

        // Translate memory messages from the client process to the server
        // process. Additionally, determine whether the call is blocking. If
        // so, switch to the server context right away.
        let blocking = message.is_blocking();
        let message = match message {
            Message::Scalar(_) | Message::BlockingScalar(_) => message,
            Message::Move(msg) => {
                let new_virt = ss.send_memory(
                    msg.buf.as_mut_ptr() as *mut usize,
                    server_pid,
                    core::ptr::null_mut(),
                    msg.buf.len(),
                )?;
                Message::Move(MemoryMessage {
                    id: msg.id,
                    buf: unsafe { MemoryRange::new(new_virt as usize, msg.buf.len()) }?,
                    offset: msg.offset,
                    valid: msg.valid,
                })
            }
            Message::MutableBorrow(msg) => {
                let new_virt = ss.lend_memory(
                    msg.buf.as_mut_ptr() as *mut usize,
                    server_pid,
                    core::ptr::null_mut(),
                    msg.buf.len(),
                    true,
                )?;
                Message::MutableBorrow(MemoryMessage {
                    id: msg.id,
                    buf: unsafe { MemoryRange::new(new_virt as usize, msg.buf.len()) }?,
                    offset: msg.offset,
                    valid: msg.valid,
                })
            }
            Message::Borrow(msg) => {
                let new_virt = ss.lend_memory(
                    msg.buf.as_mut_ptr() as *mut usize,
                    server_pid,
                    core::ptr::null_mut(),
                    msg.buf.len(),
                    false,
                )?;
                // println!(
                //     "Lending {} bytes from {:08x} in PID {} to {:08x} in PID {}",
                //     msg.buf.len(),
                //     msg.buf.as_mut_ptr() as usize,
                //     pid,
                //     new_virt as usize,
                //     server_pid,
                // );
                Message::Borrow(MemoryMessage {
                    id: msg.id,
                    buf: unsafe { MemoryRange::new(new_virt as usize, msg.buf.len()) }?,
                    offset: msg.offset,
                    valid: msg.valid,
                })
            }
        };

        // If the server has an available thread to receive the message,
        // transfer it right away.
        let server = ss.server_from_sidx_mut(sidx).expect("server couldn't be located");
        if let Some(server_tid) = server.take_available_thread() {
            // klog!(
            //     "there are threads available in PID {} to handle this message -- marking as Ready",
            //     server_pid
            // );
            let sender_idx = if message.is_blocking() {
                ss.remember_server_message(sidx, pid, tid, &message, client_address).map_err(|e| {
                    klog!("error remembering server message: {:?}", e);
                    ss.server_from_sidx_mut(sidx)
                        .expect("server couldn't be located")
                        .return_available_thread(server_tid);
                    e
                })?
            } else {
                0
            };
            let sender = SenderID::new(sidx, sender_idx, Some(pid));
            klog!("server connection data: sidx: {}, idx: {}, server pid: {}", sidx, sender_idx, server_pid);
            let envelope = MessageEnvelope { sender: sender.into(), body: message };

            // Mark the server's context as "Ready". If this fails, return the context
            // to the blocking list.
            #[cfg(baremetal)]
            ss.ready_thread(server_pid, server_tid).map_err(|e| {
                ss.server_from_sidx_mut(sidx)
                    .expect("server couldn't be located")
                    .return_available_thread(server_tid);
                e
            })?;

            let runnable = ss.runnable(server_pid, Some(server_tid)).expect("server doesn't exist");
            // --- NOTE: Returning this value //
            return if blocking && cfg!(baremetal) {
                if !runnable {
                    // If it's not runnable (e.g. it's being debugged), switch to the parent.
                    let (ppid, ptid) = unsafe { SWITCHTO_CALLER.take().unwrap() };
                    klog!(
                        "Activating Server parent process (server is blocked) and switching away from Client"
                    );
                    ss.set_thread_result(
                        server_pid,
                        server_tid,
                        xous_kernel::Result::MessageEnvelope(envelope),
                    )
                    .expect("couldn't set result for server thread");
                    let result = ss
                        .activate_process_thread(tid, ppid, ptid, false)
                        .map(|_| Ok(xous_kernel::Result::ResumeProcess))
                        .unwrap_or(Err(xous_kernel::Error::ProcessNotFound));

                    // Keep track of which process owned the quantum. This ensures that the next
                    // thread in sequence gets to run when this process is activated again.
                    ss.set_last_thread(
                        PID::new(ORIGINAL_PID.load(Relaxed)).unwrap(),
                        ORIGINAL_TID.load(Relaxed),
                    )
                    .ok();

                    result
                } else {
                    // Switch to the server, since it's in a state to be run.
                    klog!("Activating Server context and switching away from Client");
                    ss.activate_process_thread(tid, server_pid, server_tid, false)
                        .map(|_| Ok(xous_kernel::Result::MessageEnvelope(envelope)))
                        .unwrap_or(Err(xous_kernel::Error::ProcessNotFound))
                }
            } else if blocking && !cfg!(baremetal) {
                klog!("Blocking client, since it sent a blocking message");
                ss.unschedule_thread(pid, tid)?;
                ss.switch_to_thread(server_pid, Some(server_tid))?;
                ss.set_thread_result(server_pid, server_tid, xous_kernel::Result::MessageEnvelope(envelope))
                    .map(|_| xous_kernel::Result::BlockedProcess)
            } else if cfg!(baremetal) {
                klog!(
                    "Setting the return value of the Server ({}:{}) to {:?} and returning to Client",
                    server_pid,
                    server_tid,
                    envelope
                );
                ss.set_thread_result(server_pid, server_tid, xous_kernel::Result::MessageEnvelope(envelope))
                    .map(|_| xous_kernel::Result::Ok)
            } else {
                klog!("setting the return value of the Server to {:?} and returning to Client", envelope);
                // "Switch to" the server PID when not running on bare metal. This ensures
                // that it's "Running".
                ss.switch_to_thread(server_pid, Some(server_tid))?;
                ss.set_thread_result(server_pid, server_tid, xous_kernel::Result::MessageEnvelope(envelope))
                    .map(|_| xous_kernel::Result::Ok)
            };
        }
        klog!("no threads available in PID {} to handle this message, so queueing", server_pid);
        // Add this message to the queue.  If the queue is full, this
        // returns an error.
        let _queue_idx = ss.queue_server_message(sidx, pid, tid, message, client_address)?;
        klog!("queued into index {:x}", _queue_idx);

        // Park this context if it's blocking.  This is roughly
        // equivalent to a "Yield".
        if blocking {
            if cfg!(baremetal) {
                // println!("Returning to parent");
                let process = ss.get_process(pid).expect("Can't get current process");
                let ppid = process.ppid;
                unsafe { SWITCHTO_CALLER = None };
                let result = ss
                    .activate_process_thread(tid, ppid, 0, false)
                    .map(|_| Ok(xous_kernel::Result::ResumeProcess))
                    .unwrap_or(Err(xous_kernel::Error::ProcessNotFound));

                ss.set_last_thread(PID::new(ORIGINAL_PID.load(Relaxed)).unwrap(), ORIGINAL_TID.load(Relaxed))
                    .ok();
                result
            } else {
                ss.unschedule_thread(pid, tid)?;
                Ok(xous_kernel::Result::BlockedProcess)
            }
        } else {
            // println!("Returning to Client with Ok result");
            Ok(xous_kernel::Result::Ok)
        }
    })
}

fn return_memory(
    server_pid: PID,
    server_tid: TID,
    in_irq: bool,
    sender: MessageSender,
    buf: MemoryRange,
    offset: Option<MemorySize>,
    valid: Option<MemorySize>,
) -> SysCallResult {
    SystemServices::with_mut(|ss| {
        let sender = SenderID::from(sender);

        let server = ss.server_from_sidx_mut(sender.sidx).ok_or(xous_kernel::Error::ServerNotFound)?;
        if server.pid != server_pid {
            return Err(xous_kernel::Error::ServerNotFound);
        }
        let result = server.take_waiting_message(sender.idx, Some(&buf))?;
        klog!("waiting message was: {:?}", result);
        let (client_pid, client_tid, _server_addr, client_addr, len) = match result {
            WaitingMessage::BorrowedMemory(client_pid, client_ctx, server_addr, client_addr, len) => {
                (client_pid, client_ctx, server_addr, client_addr, len)
            }
            WaitingMessage::MovedMemory => {
                return Ok(xous_kernel::Result::Ok);
            }
            WaitingMessage::ForgetMemory(range) => {
                return MemoryManager::with_mut(|mm| {
                    let mut result = Ok(xous_kernel::Result::Ok);
                    let virt = range.as_ptr() as usize;
                    let size = range.len();
                    if cfg!(baremetal) && virt & 0xfff != 0 {
                        klog!("VIRT NOT DIVISIBLE BY 4: {:08x}", virt);
                        return Err(xous_kernel::Error::BadAlignment);
                    }
                    for addr in (virt..(virt + size)).step_by(PAGE_SIZE) {
                        if let Err(e) = mm.unmap_page(addr as *mut usize) {
                            if result.is_ok() {
                                result = Err(e);
                            }
                        }
                    }
                    result
                });
            }
            WaitingMessage::ScalarMessage(_pid, _tid) => {
                klog!("WARNING: Tried to wait on a message that was a scalar");
                return Err(xous_kernel::Error::DoubleFree);
            }
            WaitingMessage::None => {
                klog!("WARNING: Tried to wait on a message that didn't exist -- return memory");
                return Err(xous_kernel::Error::DoubleFree);
            }
        };
        // println!(
        //     "KERNEL({}): Returning {} bytes from {:08x} in PID {} to {:08x} in PID {} in context {}",
        //     pid,
        //     len,
        //     _server_addr.get(),
        //     pid,
        //     client_addr.get(),
        //     client_pid,
        //     client_tid
        // );
        #[cfg(baremetal)]
        let src_virt = _server_addr.get() as _;
        #[cfg(not(baremetal))]
        let src_virt = buf.as_ptr() as _;

        let return_value = xous_kernel::Result::MemoryReturned(offset, valid);

        // Return the memory to the calling process
        ss.return_memory(src_virt, client_pid, client_tid, client_addr.get() as _, len.get())?;

        if cfg!(baremetal) {
            ss.ready_thread(client_pid, client_tid)?;
        }

        // Return to the server if any of the following are true:
        //
        // 1. We're running in hosted mode -- hosted mode runs all threads simultaneously anyway
        // 2. We're in an interrupt -- interrupts cannot cross process boundaries
        // 3. The client isn't runnable -- it may be being debugged
        // 4. We're in the quantum assigned to the server -- this prevents pipeline blockages
        if !cfg!(baremetal)
            || in_irq
            || !ss.runnable(client_pid, Some(client_tid))?
            || (ORIGINAL_PID.load(Relaxed) == server_pid.get() && ORIGINAL_TID.load(Relaxed) == client_tid)
        {
            // In a hosted environment, `switch_to_thread()` doesn't continue
            // execution from the new thread. Instead it continues in the old
            // thread. Therefore, we need to instruct the client to resume, and
            // return to the server.
            #[cfg(not(baremetal))]
            ss.switch_to_thread(client_pid, Some(client_tid))?;

            // In a baremetal environment, the opposite is true -- we instruct
            // the server to resume and return to the client.
            ss.set_thread_result(client_pid, client_tid, return_value)?;
            Ok(xous_kernel::Result::Ok)
        } else {
            // Switch away from the server, but leave it as Runnable
            if cfg!(baremetal) {
                ss.unschedule_thread(server_pid, server_tid)?;
                ss.ready_thread(server_pid, server_tid)?
            }
            ss.set_thread_result(server_pid, server_tid, xous_kernel::Result::Ok)?;

            // Switch to the client
            ss.switch_to_thread(client_pid, Some(client_tid))?;
            Ok(return_value)
        }
    })
}

fn return_result(
    server_pid: PID,
    server_tid: TID,
    in_irq: bool,
    sender: MessageSender,
    return_value: xous_kernel::Result,
) -> SysCallResult {
    SystemServices::with_mut(|ss| {
        let sender = SenderID::from(sender);

        let server = ss.server_from_sidx_mut(sender.sidx).ok_or(xous_kernel::Error::ServerNotFound)?;
        if server.pid != server_pid {
            return Err(xous_kernel::Error::ServerNotFound);
        }
        let result = server.take_waiting_message(sender.idx, None)?;
        let (client_pid, client_tid) = match result {
            WaitingMessage::ScalarMessage(pid, tid) => (pid, tid),
            WaitingMessage::ForgetMemory(_) => {
                klog!("WARNING: Tried to wait on a scalar message that was actually forgettingmemory");
                return Err(xous_kernel::Error::DoubleFree);
            }
            WaitingMessage::BorrowedMemory(_, _, _, _, _) => {
                klog!("WARNING: Tried to wait on a scalar message that was actually borrowed memory");
                return Err(xous_kernel::Error::DoubleFree);
            }
            WaitingMessage::MovedMemory => {
                klog!("WARNING: Tried to wait on a scalar message that was actually moved memory");
                return Err(xous_kernel::Error::DoubleFree);
            }
            WaitingMessage::None => {
                klog!(
                    "WARNING ({}:{}): Tried to wait on a message that didn't exist (irq? {}) -- return {:?}",
                    server_pid.get(),
                    server_tid,
                    if in_irq { "yes" } else { "no" },
                    result
                );
                return Err(xous_kernel::Error::DoubleFree);
            }
        };

        if cfg!(baremetal) {
            ss.ready_thread(client_pid, client_tid)?;
        }

        // Return to the server if any of the following are true:
        //
        // 1. We're running in hosted mode -- hosted mode runs all threads simultaneously anyway
        // 2. We're in an interrupt -- interrupts cannot cross process boundaries
        // 3. The client isn't runnable -- it may be being debugged
        // 4. We're in the quantum assigned to the server -- this prevents pipeline blockages
        if !cfg!(baremetal)
            || in_irq
            || !ss.runnable(client_pid, Some(client_tid))?
            || (ORIGINAL_PID.load(Relaxed) == server_pid.get() && ORIGINAL_TID.load(Relaxed) == client_tid)
        {
            // In a hosted environment, `switch_to_thread()` doesn't continue
            // execution from the new thread. Instead it continues in the old
            // thread. Therefore, we need to instruct the client to resume, and
            // return to the server.
            // In a baremetal environment, the opposite is true -- we instruct
            // the server to resume and return to the client.
            ss.set_thread_result(client_pid, client_tid, return_value)?;
            Ok(xous_kernel::Result::Ok)
        } else {
            if cfg!(baremetal) {
                ss.unschedule_thread(server_pid, server_tid)?;
                ss.ready_thread(server_pid, server_tid)?
            }
            // Switch away from the server, but leave it as Runnable
            ss.set_thread_result(server_pid, server_tid, xous_kernel::Result::Ok)?;

            // Switch to the client
            ss.switch_to_thread(client_pid, Some(client_tid))?;
            Ok(return_value)
        }
    })
}

fn reply_and_receive_next(
    server_pid: PID,
    server_tid: TID,
    in_irq: bool,
    sender: MessageSender,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    scalar_type: usize,
) -> SysCallResult {
    let sender = SenderID::from(sender);

    SystemServices::with_mut(|ss| {
        struct MessageResponse {
            pid: PID,
            tid: TID,
            result: xous_kernel::Result,
        }

        let (result, next_message) = {
            let server = ss.server_from_sidx_mut(sender.sidx).ok_or(xous_kernel::Error::ServerNotFound)?;
            if server.pid != server_pid {
                println!(
                    "WARNING: PIDs don't match!  The server is from PID {}, but our PID is {}",
                    server.pid, server_pid
                );
                return Err(xous_kernel::Error::ServerNotFound);
            }

            let waiting_message = server.take_waiting_message(sender.idx, None)?;

            let next_message = server.take_next_message(sender.sidx);
            // If there is no message, park the server thread. We do this here because
            // we cannot hold the `Server` object while we also modify process state.
            if next_message.is_none() {
                server.park_thread(server_tid);
            }

            (waiting_message, next_message)
        };

        // TODO: Have errors turn into calls to `ReceiveMessage`
        let response = match result {
            WaitingMessage::ScalarMessage(pid, tid) => {
                let result = match scalar_type {
                    1 => xous_kernel::Result::Scalar1(arg1),
                    2 => xous_kernel::Result::Scalar2(arg1, arg2),
                    _ => xous_kernel::Result::Scalar5(arg0, arg1, arg2, arg3, arg4),
                };
                MessageResponse { pid, tid, result }
            }
            WaitingMessage::ForgetMemory(_) => {
                klog!("WARNING: Tried to wait on a scalar message that was actually forgetting memory");
                return Err(xous_kernel::Error::DoubleFree);
            }
            WaitingMessage::BorrowedMemory(pid, tid, _server_addr, client_addr, len) => {
                #[cfg(baremetal)]
                let src_virt = _server_addr.get() as _;
                #[cfg(not(baremetal))]
                let src_virt = arg1 as _;

                // Return the memory to the calling process
                ss.return_memory(src_virt, pid, tid, client_addr.get() as _, len.get())?;

                MessageResponse {
                    pid,
                    tid,
                    result: xous_kernel::Result::MemoryReturned(MemorySize::new(arg3), MemorySize::new(arg4)),
                }
            }
            WaitingMessage::MovedMemory => {
                klog!("WARNING: Tried to wait on a scalar message that was actually moved memory");
                return Err(xous_kernel::Error::DoubleFree);
            }
            WaitingMessage::None => {
                klog!("WARNING: Tried to wait on a message that didn't exist -- receive and return scalar");
                return Err(xous_kernel::Error::DoubleFree);
            }
        };
        let client_pid = response.pid;
        let client_tid = response.tid;

        if cfg!(baremetal) {
            ss.ready_thread(client_pid, client_tid)?;
        }

        // If there is a pending message, fetch it and schedule the thread to run
        if let Some(msg) = next_message {
            if !cfg!(baremetal)
                || in_irq
                || !ss.runnable(client_pid, Some(client_tid))?
                || (ORIGINAL_PID.load(Relaxed) == server_pid.get()
                    && ORIGINAL_TID.load(Relaxed) == client_tid)
            {
                // Switch to the client and return the result
                ss.set_thread_result(response.pid, response.tid, response.result)?;

                // Return the new message envelope to the server
                Ok(xous_kernel::Result::MessageEnvelope(msg))
            } else {
                if cfg!(baremetal) {
                    ss.unschedule_thread(server_pid, server_tid)?;
                    ss.ready_thread(server_pid, server_tid)?
                }

                // When the server is resumed, it will receive this as a return value.
                ss.set_thread_result(server_pid, server_tid, xous_kernel::Result::MessageEnvelope(msg))?;

                // Switch to the client
                ss.switch_to_thread(response.pid, Some(response.tid))?;
                Ok(response.result)
            }
        } else {
            // For baremetal targets, switch away from this process.
            if cfg!(baremetal) {
                // Set the thread result for the client
                ss.set_thread_result(response.pid, response.tid, response.result)?;

                // Activate the client thread and switch to it
                ss.activate_process_thread(server_tid, response.pid, response.tid, false)
                    .map(|_| Ok(xous_kernel::Result::ResumeProcess))
                    .unwrap_or(Err(xous_kernel::Error::ProcessNotFound))
            }
            // For hosted targets, simply return `BlockedProcess` indicating we'll make
            // a callback to their socket at a later time.
            else {
                ss.unschedule_thread(server_pid, server_tid)?;
                // Switch to the client and return the result
                ss.switch_to_thread(response.pid, Some(response.tid))?;
                ss.set_thread_result(response.pid, response.tid, response.result)?;

                // Indicate that the server should block its process
                Ok(xous_kernel::Result::BlockedProcess)
            }
        }
    })
}

fn receive_message(pid: PID, tid: TID, sid: SID, blocking: ExecutionType) -> SysCallResult {
    SystemServices::with_mut(|ss| {
        assert!(ss.thread_is_running(pid, tid), "current thread is not running");
        // See if there is a pending message.  If so, return immediately.
        let sidx = ss.sidx_from_sid(sid, pid).ok_or(xous_kernel::Error::ServerNotFound)?;
        let server = ss.server_from_sidx_mut(sidx).ok_or(xous_kernel::Error::ServerNotFound)?;
        // server.print_queue();

        // Ensure the server is for this PID
        if server.pid != pid {
            return Err(xous_kernel::Error::ServerNotFound);
        }

        // If there is a pending message, return it immediately.
        if let Some(msg) = server.take_next_message(sidx) {
            klog!("waiting messages found -- returning {:x?}", msg);
            return Ok(xous_kernel::Result::MessageEnvelope(msg));
        }

        if blocking == ExecutionType::NonBlocking {
            klog!("nonblocking message -- returning None");
            return Ok(xous_kernel::Result::None);
        }

        // There is no pending message, so return control to the parent
        // process and mark ourselves as awaiting an event.  When a message
        // arrives, our return value will already be set to the
        // MessageEnvelope of the incoming message.
        klog!("did not have any waiting messages -- parking thread {}", tid);
        server.park_thread(tid);

        // For baremetal targets, switch away from this process.
        if cfg!(baremetal) {
            unsafe { SWITCHTO_CALLER = None };
            let ppid = ss.get_process(pid).expect("Can't get current process").ppid;
            // TODO: Advance thread
            let result = ss
                .activate_process_thread(tid, ppid, 0, false)
                .map(|_| Ok(xous_kernel::Result::ResumeProcess))
                .unwrap_or(Err(xous_kernel::Error::ProcessNotFound));
            ss.set_last_thread(PID::new(ORIGINAL_PID.load(Relaxed)).unwrap(), ORIGINAL_TID.load(Relaxed))
                .ok();
            result
        }
        // For hosted targets, simply return `BlockedProcess` indicating we'll make
        // a callback to their socket at a later time.
        else {
            ss.unschedule_thread(pid, tid).map(|_| xous_kernel::Result::BlockedProcess)
        }
    })
}

pub fn handle(pid: PID, tid: TID, in_irq: bool, call: SysCall) -> SysCallResult {
    klog!("KERNEL({}:{}): Syscall {:x?}, in_irq={}", pid, tid, call, in_irq);
    // let call_string = format!("{:x?}", call);
    // let start_time = std::time::Instant::now();
    #[allow(clippy::let_and_return)]
    let result = if in_irq && !call.can_call_from_interrupt() {
        klog!("[!] Called {:?} that's cannot be called from the interrupt handler!", call);
        Err(xous_kernel::Error::InvalidSyscall)
    } else {
        handle_inner(pid, tid, in_irq, call)
    };

    // println!("KERNEL [{:2}:{:2}] Syscall took {:7} usec: {}", pid, tid, start_time.elapsed().as_micros(),
    // call_string);

    klog!(
        " -> ({}:{}) {:x?}",
        crate::arch::current_pid(),
        crate::arch::process::Process::current().current_tid(),
        result
    );
    result
}

pub fn handle_inner(pid: PID, tid: TID, in_irq: bool, call: SysCall) -> SysCallResult {
    match call {
        SysCall::MapMemory(phys, virt, size, req_flags) => {
            MemoryManager::with_mut(|mm| {
                let phys_ptr = phys.map(|x| x.get() as *mut u8).unwrap_or(core::ptr::null_mut());
                let virt_ptr = virt.map(|x| x.get() as *mut u8).unwrap_or(core::ptr::null_mut());

                // Don't let the address exceed the user area (unless it's PID 1)
                if pid.get() != 1 && virt.map(|x| x.get() >= arch::mem::USER_AREA_END).unwrap_or(false) {
                    klog!("Exceeded user area");
                    return Err(xous_kernel::Error::BadAddress);

                // Don't allow mapping non-page values
                } else if size.get() & (PAGE_SIZE - 1) != 0 {
                    // println!("map: bad alignment of size {:08x}", size);
                    return Err(xous_kernel::Error::BadAlignment);
                }
                // println!(
                //     "Mapping {:08x} -> {:08x} ({} bytes, flags: {:?})",
                //     phys_ptr as u32, virt_ptr as u32, size, req_flags
                // );
                let range =
                    mm.map_range(phys_ptr, virt_ptr, size.get(), pid, req_flags, MemoryType::Default)?;

                if !phys_ptr.is_null() {
                    if mm.is_main_memory(phys_ptr) {
                        let range_start = range.as_mut_ptr() as *mut usize;
                        let range_end = range_start.wrapping_add(range.len() / core::mem::size_of::<usize>());
                        unsafe {
                            crate::mem::bzero(range_start, range_end);
                        };
                    }
                    for offset in
                        (range.as_ptr() as usize..(range.as_ptr() as usize + range.len())).step_by(PAGE_SIZE)
                    {
                        // println!("Handing page to user");
                        crate::arch::mem::hand_page_to_user(offset as *mut u8)
                            .expect("couldn't hand page to user");
                    }
                }

                Ok(xous_kernel::Result::MemoryRange(range))
            })
        }
        SysCall::UnmapMemory(range) => MemoryManager::with_mut(|mm| {
            let mut result = Ok(xous_kernel::Result::Ok);
            let virt = range.as_ptr() as usize;
            let size = range.len();
            if cfg!(baremetal) && virt & 0xfff != 0 {
                return Err(xous_kernel::Error::BadAlignment);
            }
            for addr in (virt..(virt + size)).step_by(PAGE_SIZE) {
                if let Err(e) = mm.unmap_page(addr as *mut usize) {
                    if result.is_ok() {
                        result = Err(e);
                    }
                }
            }
            result
        }),
        SysCall::IncreaseHeap(delta, flags) => {
            if delta & 0xfff != 0 {
                return Err(xous_kernel::Error::BadAlignment);
            }
            // Special case for a delta of 0 -- just return the current heap size
            if delta == 0 {
                let (start, length) = ArchProcess::with_inner_mut(|process_inner| {
                    (process_inner.mem_heap_base, process_inner.mem_heap_size)
                });
                return Ok(xous_kernel::Result::MemoryRange(unsafe {
                    MemoryRange::new(
                        start,
                        // 0-length MemoryRanges are disallowed -- so return 4096 as the minimum in any case,
                        // even though it's a lie
                        if length == 0 { 4096 } else { length },
                    )
                    .unwrap()
                }));
            }

            let start = {
                ArchProcess::with_inner_mut(|process_inner| {
                    if process_inner.mem_heap_size + delta > process_inner.mem_heap_max {
                        return Err(xous_kernel::Error::OutOfMemory);
                    }

                    let start = process_inner.mem_heap_base + process_inner.mem_heap_size;
                    process_inner.mem_heap_size += delta;
                    Ok(start as *mut u8)
                })?
            };

            // Mark the new pages as "reserved"
            MemoryManager::with_mut(|mm| {
                Ok(xous_kernel::Result::MemoryRange(mm.reserve_range(start, delta, flags)?))
            })
        }
        SysCall::DecreaseHeap(delta) => {
            if delta & 0xfff != 0 {
                return Err(xous_kernel::Error::BadAlignment);
            }
            let (start, length, end) = ArchProcess::with_inner_mut(|process_inner| {
                // Don't allow decreasing the heap beyond the current allocation
                if delta > process_inner.mem_heap_size {
                    return Err(xous_kernel::Error::OutOfMemory);
                }

                let end = process_inner.mem_heap_base + process_inner.mem_heap_size;
                process_inner.mem_heap_size -= delta;
                Ok((process_inner.mem_heap_base, process_inner.mem_heap_size, end))
            })?;

            // Unmap the pages from the heap
            MemoryManager::with_mut(|mm| {
                for page in ((end - delta)..end).step_by(crate::arch::mem::PAGE_SIZE) {
                    mm.unmap_page(page as *mut usize).expect("unable to unmap page");
                }
            });

            // Return the new size of the heap
            Ok(xous_kernel::Result::MemoryRange(unsafe { MemoryRange::new(start, length).unwrap() }))
        }
        SysCall::SwitchTo(new_pid, new_tid) => SystemServices::with_mut(|ss| {
            unsafe {
                assert!(
                    SWITCHTO_CALLER.is_none(),
                    "SWITCHTO_CALLER was {:?} and not None, indicating SwitchTo was called twice",
                    SWITCHTO_CALLER,
                );
                SWITCHTO_CALLER = Some((pid, tid));
            }
            // println!(
            //     "Activating process thread {} in pid {} coming from pid {} thread {}",
            //     new_context, new_pid, pid, tid
            // );
            let new_tid = ss.activate_process_thread(tid, new_pid, new_tid, true)?;
            ORIGINAL_PID.store(new_pid.get(), Relaxed);
            ORIGINAL_TID.store(new_tid, Relaxed);
            Ok(xous_kernel::Result::ResumeProcess)
        }),
        SysCall::ClaimInterrupt(no, callback, arg) => {
            interrupt_claim(no, pid as definitions::PID, callback, arg).map(|_| xous_kernel::Result::Ok)
        }
        SysCall::FreeInterrupt(no) => {
            interrupt_free(no, pid as definitions::PID).map(|_| xous_kernel::Result::Ok)
        }
        SysCall::Yield => do_yield(pid, tid),
        SysCall::ReturnToParent(_pid, _cpuid) => {
            unsafe {
                if let Some((parent_pid, parent_ctx)) = SWITCHTO_CALLER.take() {
                    crate::arch::irq::set_isr_return_pair(parent_pid, parent_ctx)
                }
            };
            Ok(xous_kernel::Result::ResumeProcess)
        }
        SysCall::ReceiveMessage(sid) => receive_message(pid, tid, sid, ExecutionType::Blocking),
        SysCall::TryReceiveMessage(sid) => receive_message(pid, tid, sid, ExecutionType::NonBlocking),
        SysCall::WaitEvent => SystemServices::with_mut(|ss| {
            let process = ss.get_process(pid).expect("Can't get current process");
            let ppid = process.ppid;
            unsafe { SWITCHTO_CALLER = None };
            // TODO: Advance thread
            if cfg!(baremetal) {
                let result = ss
                    .activate_process_thread(tid, ppid, 0, false)
                    .map(|_| Ok(xous_kernel::Result::ResumeProcess))
                    .unwrap_or(Err(xous_kernel::Error::ProcessNotFound));
                ss.set_last_thread(PID::new(ORIGINAL_PID.load(Relaxed)).unwrap(), ORIGINAL_TID.load(Relaxed))
                    .ok();
                result
            } else {
                Ok(xous_kernel::Result::Ok)
            }
        }),
        SysCall::CreateThread(thread_init) => SystemServices::with_mut(|ss| {
            ss.create_thread(pid, thread_init).map(|new_tid| {
                // Set the return value of the existing thread to be the new thread ID
                if cfg!(baremetal) {
                    // Immediately switch to the new thread
                    ss.switch_to_thread(pid, Some(new_tid)).expect("couldn't activate new thread");
                    ss.set_thread_result(pid, tid, xous_kernel::Result::ThreadID(new_tid))
                        .expect("couldn't set new thread ID");

                    // Return `ResumeProcess` since we're switching threads
                    xous_kernel::Result::ResumeProcess
                } else {
                    xous_kernel::Result::ThreadID(new_tid)
                }
            })
        }),
        SysCall::CreateProcess(process_init) => SystemServices::with_mut(|ss| {
            ss.create_process(process_init).map(xous_kernel::Result::NewProcess)
        }),
        SysCall::CreateServerWithAddress(name) => SystemServices::with_mut(|ss| {
            ss.create_server_with_address(pid, name, true)
                .map(|(sid, cid)| xous_kernel::Result::NewServerID(sid, cid))
        }),
        SysCall::CreateServer => SystemServices::with_mut(|ss| {
            ss.create_server(pid, true).map(|(sid, cid)| xous_kernel::Result::NewServerID(sid, cid))
        }),
        SysCall::CreateServerId => {
            SystemServices::with_mut(|ss| ss.create_server_id().map(xous_kernel::Result::ServerID))
        }
        SysCall::TryConnect(sid) => {
            SystemServices::with_mut(|ss| ss.connect_to_server(sid).map(xous_kernel::Result::ConnectionID))
        }
        SysCall::ReturnMemory(sender, buf, offset, valid) => {
            return_memory(pid, tid, in_irq, sender, buf, offset, valid)
        }
        SysCall::ReturnScalar1(sender, arg) => {
            return_result(pid, tid, in_irq, sender, xous_kernel::Result::Scalar1(arg))
        }
        SysCall::ReturnScalar2(sender, arg1, arg2) => {
            return_result(pid, tid, in_irq, sender, xous_kernel::Result::Scalar2(arg1, arg2))
        }
        SysCall::ReturnScalar5(sender, arg1, arg2, arg3, arg4, arg5) => return_result(
            pid,
            tid,
            in_irq,
            sender,
            xous_kernel::Result::Scalar5(arg1, arg2, arg3, arg4, arg5),
        ),
        SysCall::ReplyAndReceiveNext(sender, a0, a1, a2, a3, a4, scalar_type) => {
            reply_and_receive_next(pid, tid, in_irq, sender, a0, a1, a2, a3, a4, scalar_type)
        }
        SysCall::TrySendMessage(cid, message) => send_message(pid, tid, cid, message),
        SysCall::TerminateProcess(_ret) => SystemServices::with_mut(|ss| {
            ss.unschedule_thread(pid, tid)?;
            ss.terminate_process(pid)?;
            // Clear out `SWITCHTO_CALLER` since we're resuming the parent process.
            unsafe { SWITCHTO_CALLER = None };
            Ok(xous_kernel::Result::ResumeProcess)
        }),
        SysCall::Shutdown => SystemServices::with_mut(|ss| ss.shutdown().map(|_| xous_kernel::Result::Ok)),
        SysCall::GetProcessId => Ok(xous_kernel::Result::ProcessID(pid)),
        SysCall::GetThreadId => Ok(xous_kernel::Result::ThreadID(tid)),

        SysCall::Connect(sid) => {
            let result = SystemServices::with_mut(|ss| {
                ss.connect_to_server(sid).map(xous_kernel::Result::ConnectionID)
            });
            match result {
                Ok(o) => Ok(o),
                Err(xous_kernel::Error::ServerNotFound) => retry_syscall(pid, tid),
                Err(e) => Err(e),
            }
        }
        SysCall::ConnectForProcess(pid, sid) => {
            let result = SystemServices::with_mut(|ss| {
                ss.connect_process_to_server(pid, sid).map(xous_kernel::Result::ConnectionID)
            });
            match result {
                Ok(o) => Ok(o),
                Err(xous_kernel::Error::ServerNotFound) => retry_syscall(pid, tid),
                Err(e) => Err(e),
            }
        }
        SysCall::SendMessage(cid, message) => {
            let result = send_message(pid, tid, cid, message);
            match result {
                Ok(o) => Ok(o),
                Err(xous_kernel::Error::ServerQueueFull) => retry_syscall(pid, tid),
                Err(e) => Err(e),
            }
        }
        SysCall::Disconnect(cid) => {
            SystemServices::with_mut(|ss| ss.disconnect_from_server(cid).and(Ok(xous_kernel::Result::Ok)))
        }
        SysCall::DestroyServer(sid) => {
            SystemServices::with_mut(|ss| ss.destroy_server(pid, sid).and(Ok(xous_kernel::Result::Ok)))
        }
        SysCall::JoinThread(other_tid) => {
            SystemServices::with_mut(|ss| ss.join_thread(pid, tid, other_tid)).map(|ret| {
                // Successfully joining a thread causes this thread to sleep while the parent process
                // is resumed. This is the same as a `Yield`
                if ret == xous_kernel::Result::ResumeProcess {
                    unsafe { SWITCHTO_CALLER = None };
                }
                ret
            })
        }
        SysCall::UpdateMemoryFlags(range, flags, pid) => {
            // We do not yet support modifying flags for other processes.
            if pid.is_some() {
                return Err(xous_kernel::Error::ProcessNotChild);
            }

            MemoryManager::with_mut(|mm| mm.update_memory_flags(range, flags))?;
            Ok(xous_kernel::Result::Ok)
        }
        SysCall::AdjustProcessLimit(index, current, new) => match index {
            1 => arch::process::Process::with_inner_mut(|p| {
                if p.mem_heap_max == current {
                    p.mem_heap_max = new;
                }
                Ok(xous_kernel::Result::Scalar2(index, p.mem_heap_max))
            }),
            2 => arch::process::Process::with_inner_mut(|p| {
                if p.mem_heap_size == current && new < p.mem_heap_max {
                    p.mem_heap_size = new;
                }
                Ok(xous_kernel::Result::Scalar2(index, p.mem_heap_size))
            }),
            _ => Err(xous_kernel::Error::InvalidLimit),
        },
        #[cfg(feature = "v2p")]
        SysCall::VirtToPhys(vaddr) => {
            let phys_addr = crate::arch::mem::virt_to_phys(vaddr as usize);
            match phys_addr {
                Ok(pa) => Ok(xous_kernel::Result::Scalar1(pa)),
                Err(_) => Err(xous_kernel::Error::BadAddress),
            }
        }
        #[cfg(feature = "v2p")]
        SysCall::VirtToPhysPid(pid, vaddr) => {
            let phys_addr = crate::arch::mem::virt_to_phys_pid(pid, vaddr as usize);
            match phys_addr {
                Ok(pa) => Ok(xous_kernel::Result::Scalar1(pa)),
                Err(_) => Err(xous_kernel::Error::BadAddress),
            }
        }
        #[cfg(feature = "swap")]
        SysCall::RegisterSwapper(s0, s1, s2, s3) => {
            Swap::with_mut(|swap| swap.register_handler(s0, s1, s2, s3))
        }
        #[cfg(feature = "swap")]
        SysCall::EvictPage(target_pid, vaddr) => {
            if pid.get() != 2 {
                klog!("Illegal caller"); // only PID 2 can call this
                return Err(xous_kernel::Error::AccessDenied);
            }
            let phys_addr = crate::arch::mem::virt_to_phys_pid(target_pid, vaddr as usize);
            match phys_addr {
                Ok(pa) => Swap::with_mut(|swap| swap.evict_page(target_pid, vaddr, pa)),
                Err(_) => Err(xous_kernel::Error::BadAddress),
            }
        }

        /* https://github.com/betrusted-io/xous-core/issues/90
        SysCall::SetExceptionHandler(pc, sp) => SystemServices::with_mut(|ss| {
            ss.set_exception_handler(pid, pc, sp)
                .and(Ok(xous_kernel::Result::Ok))
        }),
        */
        _ => Err(xous_kernel::Error::UnhandledSyscall),
    }
}
