#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use heapless::String;
use heapless::Vec;
use heapless::consts::*;

use log::{error, info};

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();

    let name_server =
        xous::create_server_with_address(b"xousnames-server").expect("Couldn't create xousnames-server");

    info!("NS: started");
    loop {
        let envelope = xous::receive_message(name_server).unwrap();
        info!("NS: received message");
        if let xous::Message::MutableBorrow(m) = &envelope.body {
            if m.id == ID_REGISTER_NAME {
                let registration: &mut Registration = unsafe {
                    &mut *(m.buf.as_mut_ptr() as *mut Registration)
                };
                let namevec: Vec<u8, U64> = Vec::from_slice(&registration.name).unwrap();
                let name: String<U64> = String::from_utf8(namevec).unwrap();
                info!("NS: registration request for {}", name);
                registration.success = true;
                xous::return_memory(envelope.sender, m.buf).unwrap();
            } else {
                error!("NS: unknown message ID received");
            }
        } else {
            error!("NS: couldn't convert opcode");
        }
    }
}
