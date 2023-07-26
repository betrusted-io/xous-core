#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use gam::Gam;
mod api;
use api::{Opcode, Return, LoadAppRequest, App, AppRequest};
use xous_ipc::Buffer;
use num_traits::FromPrimitive;

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(api::SERVER_NAME_APP_LOADER, None).expect("Couldn't create server");

    let gam = Gam::new(&xns).expect("can't connect to GAM");

    // the loaded apps
    let mut apps = vec![];

    loop {
	let mut msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()).expect("Couldn't load message") {
	    Opcode::LoadApp => {
		let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
		let load_app_req = buffer.to_original::<LoadAppRequest, _>().unwrap();
		// a dummy server for testing purposes
		let name = "_Hello World_"; // note: this MUST match up with the gam name

		// create the spawn process
		let stub = include_bytes!("../../../target/riscv32imac-unknown-xous-elf/debug/spawn.bin");
		let args = xous::ProcessArgs::new(stub, xous::MemoryAddress::new(0x2050_1000).unwrap(), xous::MemoryAddress::new(0x2050_1000).unwrap());
		let spawn = xous::create_process(args).expect("Couldn't create spawn process");
		log::info!("Spawn PID: {}, Spawn CID: {}", spawn.pid, spawn.cid);

		// perform a ping to make sure that spawn is running
		let result = xous::send_message(
		    6,
		    xous::Message::new_blocking_scalar(2, 1, 2, 3, 4),
		)
		    .unwrap();
		log::info!("Result of ping: {:?}", result);
		
		// load the app from the binary file
		let bin = include_bytes!("hello");
		let bin_len = bin.len();
		let bin_loc = bin.as_ptr() as usize;
		let buf = unsafe { xous::MemoryRange::new(bin_loc & !0xFFF, bin_len + if bin_len & 0xFFF == 0 { 0 } else {0x1000 - (bin_len & 0xFFF)}).expect("Couldn't create a buffer from the segment") };
		xous::send_message(6, xous::Message::new_lend(1, buf, None, None)).expect("Couldn't send a message to spawn");
		
		apps.push(xous_ipc::String::from_str(name));
		log::info!("Added app `{}'!", load_app_req.name);
	    },
	    Opcode::FetchAppData => {
		log::info!("Data requested.");
		let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
		let req = buffer.to_original::<AppRequest, _>().unwrap();
		let mut response = Return::Failure;
		if req.index < apps.len() {
		    let name = apps[req.index];
		    let retstruct = App {
			name: xous_ipc::String::from_str(name)
		    };
		    response = Return::Info(retstruct);
		}
		buffer.replace(response).unwrap();
	    },
	    Opcode::DispatchApp => {
		let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
		let req = buffer.to_original::<AppRequest, _>().unwrap();
		if req.index < apps.len() {
		    let name: xous_ipc::String<64> = apps[req.index];
		    gam.switch_to_app(name.as_ref(), req.auth.expect("No auth token given!")).expect(&format!("Could not dispatch app `{}'", name));
		}
	    },
	}
    }
}
