#![cfg_attr(target_os = "none", no_std)]

mod api;
mod buffer;

use core::sync::atomic::{AtomicU32, Ordering};
use num_traits::{FromPrimitive, ToPrimitive};

// Note that connection IDs are never 0, so this is invalid, and could map to `None`.
static SERVER_CID: AtomicU32 = AtomicU32::new(0);

/// Ensure we have a connection to our own server. If a connection does not exist,
/// have the nameserver create one for us.
fn ensure_connection() -> xous::CID {
    let mut cid = SERVER_CID.load(Ordering::Relaxed);
    if cid == 0 {
        cid = xous_names::request_connection_blocking(api::SERVER_NAME).unwrap();
        SERVER_CID.store(cid, Ordering::Relaxed);
    }
    cid
}

/// Add two numbers to gether, and return the result. Returns
/// an Error if one is detected.
pub fn add(arg1: i32, arg2: i32) -> Result<i32, api::Error> {
    // Create an opcode variant. This can be a rich structure.
    let op = api::MathOperation::Add(arg1, arg2);

    // Convert the opcode into a serialized buffer. This consumes the opcode, which will
    // exist on the heap in its own page. Furthermore, this will be in a flattened format
    // suitable for passing around as a message.
    let mut buf = buffer::Buffer::try_from(op).or(Err(api::Error::InternalError))?;

    // Mutably lend our op to the server, specifying the `api::Opcode::Mathematics` opcode.
    // This will return a structure that we can deserialize back into something we
    // can verify.
    buf.lend_mut(
        ensure_connection(),
        api::Opcode::Mathematics.to_u32().unwrap(),
    )
    .or(Err(api::Error::InternalError))?;

    // Turn the result of the response into a friendly value that can be understood
    // by the caller.
    match buf.deserialize().unwrap() {
        api::MathResult::Value(v) => Ok(v),
        api::MathResult::Error(val) => Err(val),
    }
}

/// Condensed form of the operation, that allows for reuse among other operations.
fn do_op(op: api::MathOperation) -> Result<i32, api::Error> {
    // Convert the opcode into a serialized buffer. This consumes the opcode, which will
    // exist on the heap in its own page. Furthermore, this will be in a flattened format
    // suitable for passing around as a message.
    let mut buf = buffer::Buffer::try_from(op).or(Err(api::Error::InternalError))?;

    // Lend our op to the server, specifying the `api::Opcode::Mathematics` opcode.
    // This will return a structure that we can deserialize back into something we
    // can verify.
    buf.lend_mut(
        ensure_connection(),
        api::Opcode::Mathematics.to_u32().unwrap(),
    )
    .or(Err(api::Error::InternalError))?;

    // Don't deserialize it -- use the archived version
    match *buf.try_into::<api::MathResult, _>().unwrap() {
        api::ArchivedMathResult::Value(v) => Ok(v),
        api::ArchivedMathResult::Error(api::ArchivedError::InternalError) => Err(api::Error::InternalError),
        api::ArchivedMathResult::Error(api::ArchivedError::Overflow) => Err(api::Error::Overflow),
        api::ArchivedMathResult::Error(api::ArchivedError::Underflow) => Err(api::Error::Underflow),
    }
}

pub fn subtract(arg1: i32, arg2: i32) -> Result<i32, api::Error> {
    do_op(api::MathOperation::Subtract(arg1, arg2))
}

pub fn multiply(arg1: i32, arg2: i32) -> Result<i32, api::Error> {
    do_op(api::MathOperation::Multiply(arg1, arg2))
}

pub fn divide(arg1: i32, arg2: i32) -> Result<i32, api::Error> {
    do_op(api::MathOperation::Divide(arg1, arg2))
}

/// Log the given message to the server.
/// We accept any two parameters that can be treated as strings.
pub fn log_message<S: AsRef<str>, T: AsRef<str>>(prefix: S, message: T) {
    let op = api::LogString {
        prefix: xous::String::from_str(prefix.as_ref()),
        message: xous::String::from_str(message.as_ref()),
    };

    // Convert the opcode into a serialized buffer. This consumes the opcode, which will
    // exist on the heap in its own page. Furthermore, this will be in a flattened format
    // suitable for passing around as a message.
    let buf = buffer::Buffer::try_from(op).unwrap();

    // Send the message to the server.
    buf.lend(
        ensure_connection(),
        api::Opcode::LogString.to_u32().unwrap(),
    )
    .unwrap();
}

// Callback messages will be sent to this private server.
// Ideally it would be held in a Mutex.
static mut CALLBACK_SID: Option<xous::SID> = None;
static mut CALLBACK_ARRAY: [Option<fn(&str, &str)>; 8] = [None; 8];

// This callback server runs in its own thread.
fn callback_server() {
    let sid = unsafe { CALLBACK_SID }.unwrap();
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::CallbackType::LogString) => {
                let mem = msg.body.memory_message().unwrap();
                let buffer = unsafe { buffer::Buffer::from_memory_message(mem) };
                let log_string = buffer.try_into::<api::LogString, _>().unwrap();
                unsafe {
                    for entry in CALLBACK_ARRAY.iter() {
                        if let Some(cb) = entry {
                            cb(log_string.message.as_str(), log_string.prefix.as_str());
                        }
                    }
                }
            }
            _ => (),
        }
    }
}

pub fn hook_log_messages(cb: fn(&str, &str)) {
    // Add this entry to the callback array
    unsafe {
        for entry in CALLBACK_ARRAY.iter_mut() {
            if entry.is_none() {
                *entry = Some(cb);
                break;
            }
        }
    }

    // If no thread exists to handle callbacks, create one.
    // Publish this server with the main thread so it can call us as necessary.
    if unsafe { CALLBACK_SID }.is_none() {
        let sid = xous::create_server().unwrap();
        unsafe { CALLBACK_SID = Some(sid) };
        xous::create_thread_0(callback_server).unwrap();
        let sid = sid.to_u32();
        xous::send_message(
            ensure_connection(),
            xous::Message::Scalar(xous::ScalarMessage {
                id: api::Opcode::AddLogStringCallback.to_usize().unwrap(),
                arg1: sid.0 as _,
                arg2: sid.1 as _,
                arg3: sid.2 as _,
                arg4: sid.3 as _,
            }),
        )
        .unwrap();
    }
}

pub fn double_string<const N: usize>(value: &xous::String<N>) -> xous::String<N> {
    let op = api::StringDoubler {
        value: xous::String::from_str(value.as_str().unwrap()),
    };

    // Convert the opcode into a serialized buffer. This consumes the opcode, which will
    // exist on the heap in its own page. Furthermore, this will be in a flattened format
    // suitable for passing around as a message.
    let mut buf = buffer::Buffer::try_from(op).unwrap();

    // Send the message to the server.
    buf.lend_mut(
        ensure_connection(),
        api::Opcode::DoubleString.to_u32().unwrap(),
    )
    .unwrap();

    xous::String::from_str(buf.try_into::<api::StringDoubler, _>().unwrap().value.as_str())
}
