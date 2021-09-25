#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use num_traits::*;
use std::collections::HashMap;
use xous_ipc::{Buffer, String};
use std::sync::{Arc, Mutex};
use std::thread;
use utralib::generated::*;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Op1 {
    Test1,
    Test2,
    Test3,
    Quit,
}
pub enum UartType {
    Kernel = 0,
    Log = 1,
    Application = 2,
    Invalid = 3,
}
// from/to for Xous messages
impl From<usize> for UartType {
    fn from(code: usize) -> Self {
        match code {
            0 => UartType::Kernel,
            1 => UartType::Log,
            2 => UartType::Application,
            _ => UartType::Invalid,
        }
    }
}
impl Into<usize> for UartType {
    fn into(self) -> usize {
        match self {
            UartType::Kernel => 0,
            UartType::Log => 1,
            UartType::Application => 2,
            UartType::Invalid => 3,
        }
    }
}
// for the actual bitmask going to hardware
impl Into<u32> for UartType {
    fn into(self) -> u32 {
        match self {
            UartType::Kernel => 0,
            UartType::Log => 1,
            UartType::Application => 2,
            UartType::Invalid => 3,
        }
    }
}

#[xous::xous_main]
fn shell_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let gpio_base = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map GPIO CSR range");
    let mut gpio_csr = CSR::new(gpio_base.as_mut_ptr() as *mut u32);
    // setup the initial logging output
    gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, UartType::Log as u32);

    let server1 = Arc::new(Mutex::new(xous::create_server_with_address(b"test-fooo-server")
        .expect("Couldn't create xousnames-server")));

    let _handle = thread::spawn({
        let sid = Arc::clone(&server1);
        move || {
            let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
            let mut map = HashMap::<usize, i32>::new();
            let mut value = 0;
            log::info!("test target started");
            loop {
                let msg = xous::receive_message(*sid.lock().unwrap()).unwrap();
                log::info!("received {:?}", msg);
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(Op1::Test1) => xous::msg_blocking_scalar_unpack!(msg, token, _, _, _, {
                        log::info!("target test1");
                        //icktimer.sleep_ms(1000).unwrap();
                        value = token + 1;
                        xous::return_scalar(msg.sender, value).unwrap();
                    }),
                    Some(Op1::Test2) => {
                        log::info!("target test2");
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let name = buffer.to_original::<String::<128>, _>().unwrap();
                        map.insert(name.len(), value as i32);
                    },
                    Some(Op1::Test3) => {
                        log::info!("target test3");
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let name = buffer.to_original::<String::<128>, _>().unwrap();
                        log::info!("retrieved {:?}", map.get(&name.len()));
                    }
                    Some(Op1::Quit) => break,
                    None => {
                        log::info!("couldn't handle message");
                    }
                }
            }
        }

    });

    let test_conn = xous::connect(*server1.lock().unwrap()).unwrap();
    let _handle2 = thread::spawn({
        move || {
            let server2 = xous::create_server_with_address(b"test-food-server")
                .expect("Couldn't create xousnames-server");
            let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
            let identifier = String::<128>::from_str("test string");
            let mut local_id = 0;
            let mut map = HashMap::<usize, i32>::new();
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
                                xous::return_scalar(msg.sender, value).unwrap();
                            }),
                            Some(Op1::Test2) => {
                                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                                let name = buffer.to_original::<String::<128>, _>().unwrap();
                                map.insert(name.len(), value as i32);
                            },
                            Some(Op1::Test3) => {
                                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                                let name = buffer.to_original::<String::<128>, _>().unwrap();
                                log::info!("retrieved {:?}", map.get(&name.len()));
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
                        log::info!("sending msg {}", local_id);

                        /*
                        let response = xous::send_message(test_conn,
                            xous::Message::new_blocking_scalar(Op1::Test1.to_usize().unwrap(), local_id, 0, 0, 0)
                        ).unwrap();
                        if let xous::Result::Scalar1(result) = response {
                            local_id = result;
                        }*/
                        log::info!("updated {}", local_id);
                        let buf = Buffer::into_buf(identifier).unwrap();
                        log::info!("test2");
                        buf.lend(test_conn, Op1::Test2.to_u32().unwrap()).unwrap();
                        let buf2 = Buffer::into_buf(identifier).unwrap();
                        log::info!("test3");
                        buf2.lend(test_conn, Op1::Test3.to_u32().unwrap()).unwrap();
                    }
                }
            }
        }
    });
    loop {
        xous::yield_slice();
    }

    xous::terminate_process(0)
}