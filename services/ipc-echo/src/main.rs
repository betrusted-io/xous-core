//! Minimal MutableBorrow echo server for the IPC Renode CI suite.
//!
//! Accepts an `EchoPayload` lend, deserializes it, and writes back a response
//! payload with the same tag but a *differently sized* heap payload (the label
//! is dropped, data is inverted). The size change is deliberate: it forces the
//! kernel to report a new `used` offset back through `lend_mut`, which is the
//! offset-update path in `Buffer::lend_mut`. Keep this server as small as a
//! skeleton: it exists only so the suite can push multi-page rkyv records
//! through a real kernel lend.

use ipc_echo::*;
use log::LevelFilter;

/// The large-payload tests lend a ~300 kB payload; the default 512 kB heap
/// cannot hold request + response `Vec`s, so raise the heap ceiling (same
/// idiom as services/swaptest1).
fn bump_heap() {
    const HEAP_LIMIT: usize = 1024 * 1024 * 2;
    let result = xous::rsyscall(xous::SysCall::AdjustProcessLimit(
        xous::Limits::HeapMaximum as usize,
        0,
        HEAP_LIMIT,
    ));
    if let Ok(xous::Result::Scalar2(1, current_limit)) = result {
        xous::rsyscall(xous::SysCall::AdjustProcessLimit(
            xous::Limits::HeapMaximum as usize,
            current_limit,
            HEAP_LIMIT,
        ))
        .expect("couldn't raise heap limit");
        log::info!("heap limit raised to {}", HEAP_LIMIT);
    } else {
        panic!("Unsupported syscall!");
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());
    bump_heap();

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(SERVER_NAME, None).expect("can't register server");
    log::info!("ipc-echo ready");

    let mut msg_opt = None;
    loop {
        xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
        let msg_id = msg_opt.as_ref().unwrap().body.id();
        match msg_id {
            OP_ECHO => {
                let mut buf = unsafe {
                    xous_ipc::Buffer::from_memory_message_mut(
                        msg_opt.as_mut().unwrap().body.memory_message_mut().unwrap(),
                    )
                };
                let req = buf.to_original::<EchoPayload, _>().expect("echo: bad request");
                // respond with same tag, reversed data, no label: the serialized
                // response is a different length than the request.
                let resp = EchoPayload {
                    tag: req.tag,
                    data: req.data.into_iter().rev().collect(),
                    label: String::new(),
                };
                buf.replace(resp).expect("echo: response does not fit the buffer");
            }
            _ => {
                log::warn!("ipc-echo: unknown opcode {}", msg_id);
            }
        }
    }
}
