#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::{error, info};

use core::fmt::Write;

use rkyv::Unarchive;
use rkyv::archived_value;
use core::pin::Pin;

use ime_plugin_api::{ImeFrontEndApi, ImeFrontEnd};

use heapless::spsc::Queue;
use heapless::consts::U4;

fn repl(history: &mut Queue<xous::String<1024>, U4>) -> Result<(), xous::Error> {
    for &s in history.iter() {
        info!("SHCH: command history {}", s);
    }

    Ok(())
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    info!("SHCH: my PID is {}", xous::process::id());

    let shch_sid = xous_names::register_name(xous::names::SERVER_NAME_SHELLCHAT).expect("SHCH: can't register server");
    info!("SHCH: registered with NS -- {:?}", shch_sid);

    let imef_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_IME_FRONT).expect("SHCH: can't connect to IMEF");
    let imef = ImeFrontEnd { connection: Some(imef_conn) };
    imef.register_listener(xous::names::SERVER_NAME_IME_FRONT).expect("SHCH: couldn't request events from IMEF");

    let mut history: Queue<xous::String<1024>, U4> = Queue::new(); // this has 2^4 elements = 16???

    info!("SHCH: starting main loop");
    loop {
        let envelope = xous::receive_message(shch_sid).unwrap();
        if let xous::Message::Move(m) = &envelope.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<ime_plugin_api::ImefOpcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<ime_plugin_api::ImefOpcode>::GotInputLine(rkyv_s) => {
                    let s: xous::String<4000> = rkyv_s.unarchive();
                    let mut local: xous::String<1024> = xous::String::new();
                    write!(local, "{}", s.as_str().expect("SHCH: couldn't convert incoming string")).expect("SHCH: probably exceeded input length");

                    if history.len() == history.capacity() {
                        history.dequeue().expect("SHCH: couldn't dequeue historye");
                    }
                    history.enqueue(local).expect("SHCH: couldn't store input line");

                    repl(&mut history).expect("SHCH: repl experienced an error");
                },
                _ => panic!("SHCH: invalid response from server -- corruption occurred in MemoryMessage")
            };
        } else {
            error!("SHCH: couldn't convert message");
        }
    }
}
