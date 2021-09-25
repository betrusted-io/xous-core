#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use num_traits::*;
use std::collections::HashMap;
use xous_ipc::{Buffer, String};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Op1 {
    Test1,
    Test2,
    Test3,
    Quit,
}

#[xous::xous_main]
fn shell_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let server1 = Arc::new(Mutex::new(xous::create_server_with_address(b"test-fooo-server")
        .expect("Couldn't create xousnames-server")));

    let handle = thread::spawn({
        let sid = Arc::clone(&server1);
        move || {
            let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
            let mut map = HashMap::<&str, i32>::new();
            let mut value = 0;
            loop {
                let msg = xous::receive_message(*sid.lock().unwrap()).unwrap();
                log::info!("received {:?}", msg);
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(Op1::Test1) => xous::msg_blocking_scalar_unpack!(msg, token, _, _, _, {
                        ticktimer.sleep_ms(1000).unwrap();
                        value = token + 1;
                        xous::return_scalar(msg.sender, value);
                    }),
                    Some(Op1::Test2) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let name = buffer.to_original::<String::<128>, _>().unwrap();
                        map.insert(name.as_str().unwrap(), value as i32);
                    },
                    Some(Op1::Test3) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let name = buffer.to_original::<String::<128>, _>().unwrap();
                        log::info!("retrieved {:?}", map.get(&name));
                    }
                    Some(Op1::Quit) => break,
                    None => {
                        log::info!("couldn't handle message");
                    }
                }
            }
        }

    });

    let server2 = xous::create_server_with_address(b"test-food-server")
        .expect("Couldn't create xousnames-server");
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
    let test_conn = xous::connect(server1.lock().unwrap()).unwrap();
    let identifier = String::<128>::from_str("test string");
    let mut local_id = 0;
    let mut map = HashMap::<&str, i32>::new();
    let mut value = 0;
    loop {
        let maybe_msg = xous::try_receive_message(server2).unwrap();
        match maybe_msg {
            Some(msg) => {
                log::info!("received {:?}", msg);
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(Op1::Test1) => xous::msg_blocking_scalar_unpack!(msg, token, _, _, _, {
                        ticktimer.sleep_ms(1000).unwrap();
                        value = token + 1;
                        xous::return_scalar(msg.sender, value);
                    }),
                    Some(Op1::Test2) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let name = buffer.to_original::<String, _>().unwrap();
                        map.insert(name.as_str().unwrap(), value);
                    },
                    Some(Op1::Test3) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let name = buffer.to_original::<String, _>().unwrap();
                        log::info!("retrieved {:?}", map.get(&name));
                    }
                    Some(Op1::Quit) => break,
                    None => {
                        log::info!("couldn't handle message");
                    }
                }
            }
            None => {
                log::info!("test pump");
                ticktimer.sleep_ms(1000).unwrap();
                let response = xous::send_message(test_conn,
                    xous::Message::new_blocking_scalar(Op1::Test1.to_usize().unwrap(), local_id, 0, 0, 0)
                ).unwrap();
                if let xous::Result::Scalar1(result) = response {
                    local_id = result;
                }
                let buf = Buffer::into_buf(identifier).unwrap();
                buf.lend(test_conn, Op1::Test2.to_u32().unwrap()).unwrap();
                let buf2 = Buffer::into_buf(identifier).unwrap();
                buf2.lend(test_conn, Op1::Test3.to_u32().unwrap()).unwrap();
            }
        }
    }

    xous::terminate_process(0)
}