#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
mod buffer;
use num_traits::{FromPrimitive, ToPrimitive};

fn value_or(val: Option<i32>, default: api::MathResult) -> api::MathResult {
    val.map(|v| api::MathResult::Value(v)).unwrap_or(default)
}

fn handle_math_withcopy(mem: &mut xous::MemoryMessage) {
    let buffer = unsafe { buffer::Buffer::from_memory_message(mem) };
    let response = {
        use api::MathOperation::*;
        match buffer.deserialize().unwrap() {
            Add(a, b) => value_or(
                a.checked_add(b),
                api::MathResult::Error(api::Error::Overflow),
            ),
            Subtract(a, b) => value_or(
                a.checked_sub(b),
                api::MathResult::Error(api::Error::Underflow),
            ),
            Multiply(a, b) => value_or(
                a.checked_mul(b),
                api::MathResult::Error(api::Error::Overflow),
            ),
            Divide(a, b) => value_or(
                a.checked_div(b),
                api::MathResult::Error(api::Error::Underflow),
            ),
        }
    };
    buffer.serialize_from(response).unwrap();
}

// This doesn't deserialize the struct, and therefore operates entirely
// on the archived data. This saves a copy step.
fn handle_math_zerocopy(mem: &mut xous::MemoryMessage) {
    let buffer = unsafe { buffer::Buffer::from_memory_message(mem) };
    let response = {
        use api::ArchivedMathOperation::*;
        match *buffer.try_into::<api::MathOperation, _>().unwrap() {
            Add(a, b) => value_or(
                a.checked_add(b),
                api::MathResult::Error(api::Error::Overflow),
            ),
            Subtract(a, b) => value_or(
                a.checked_sub(b),
                api::MathResult::Error(api::Error::Underflow),
            ),
            Multiply(a, b) => value_or(
                a.checked_mul(b),
                api::MathResult::Error(api::Error::Overflow),
            ),
            Divide(a, b) => value_or(
                a.checked_div(b),
                api::MathResult::Error(api::Error::Underflow),
            ),
        }
    };
    buffer.serialize_from(response).unwrap();
}

fn handle_log_string(mem: &xous::MemoryMessage) {
    let buffer = unsafe { buffer::Buffer::from_memory_message(mem) };
    let log_string = buffer.try_into::<api::LogString, _>().unwrap();
    log::info!(
        "Prefix: {}  Message: {}",
        log_string.prefix.as_str(),
        log_string.message.as_str()
    );
}

#[xous::xous_main]
fn test_main() -> ! {
    let mut callback_conn = None;
    log_server::init_wait().unwrap();

    log::info!(
        "Hello, world! This is the server, PID {}",
        xous::current_pid().unwrap()
    );
    let sid = xous_names::register_name(api::SERVER_NAME).unwrap();

    loop {
        let mut msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::Mathematics) => {
                handle_math_withcopy(msg.body.memory_message_mut().unwrap())
            }
            Some(api::Opcode::LogString) => {
                let memory = msg.body.memory_message().unwrap();
                if let Some(callback_sid) = callback_conn {
                    let buffer = unsafe { buffer::Buffer::from_memory_message(memory) };
                    buffer
                        .lend(callback_sid, api::CallbackType::LogString.to_u32().unwrap())
                        .unwrap();
                }
                handle_log_string(msg.body.memory_message().unwrap())
            }
            Some(api::Opcode::AddLogStringCallback) => {
                if let xous::Message::Scalar(xous::ScalarMessage {
                    id: _id,
                    arg1,
                    arg2,
                    arg3,
                    arg4,
                }) = msg.body
                {
                    let sid = xous::SID::from_u32(arg1 as _, arg2 as _, arg3 as _, arg4 as _);
                    callback_conn = Some(xous::connect(sid).unwrap());
                }
            }
            None => (),
        }
    }
}
