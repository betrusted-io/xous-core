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
		let stub = include_bytes!("spawn-stub");
		let args = xous::ProcessArgs::new(stub, xous::MemoryAddress::new(0x2050_1000).unwrap(), xous::MemoryAddress::new(0x2050_1000).unwrap());
		let spawn = xous::create_process(args).expect("Couldn't create process!");
		log::info!("Spawn PID: {}, Spawn CID: {}", spawn.pid, spawn.cid);

		// perform a ping to make sure that spawn is running
		let result = xous::send_message(
		    spawn.cid,
		    xous::Message::new_blocking_scalar(4, 1, 2, 3, 4),
		)
		    .unwrap();
		log::info!("Result of ping: {:?}", result);
		
		// load the app from the binary file
		let bin = include_bytes!("hello");
		// make sure that this is a 32 bit elf file
		assert!(bin[0] == 0x7F);
		assert!(bin[1] == 0x45);
		assert!(bin[2] == 0x4c);
		assert!(bin[3] == 0x46);
		assert!(bin[4] == 0x01);

		let to_usize = |start, size| {
		    if size == 1 {
			return bin[start] as usize;
		    }
		    if size == 2 {
			// assumes little endianness
			return u16::from_le_bytes(bin[start..start+size].try_into().unwrap()) as usize;
		    }
		    if size == 4 {
			return u32::from_le_bytes(bin[start..start+size].try_into().unwrap()) as usize;
		    }
		    panic!("Tried to get usize of invalid size!");
		};
		
		let entry_point = to_usize(0x18, 4); 
		let ph_start = to_usize(0x1c, 4);
		let ph_size = to_usize(0x2A, 2);
		let ph_count = to_usize(0x2C, 2);

		// add the segments we should load
		for i in 0..ph_count {
		    let start = ph_start + i * ph_size;
		    // only load PT_LOAD segments
		    if  to_usize(start, 4) == 0x00000001 {
			let offset = to_usize(start+0x04, 4);
			let vaddr = to_usize(start+0x08, 4);
			let mem_size = to_usize(start+0x14, 4);

			log::info!("Loading offset {} to virtual address {} with memory size {}", offset, vaddr, mem_size);
			
			let buf = unsafe { xous::MemoryRange::new(bin[offset..offset+mem_size].as_ptr() as usize, core::mem::size_of::<u8>()).expect("Couldn't create a buffer from the segment") };
			xous::send_message(spawn.cid, xous::Message::new_lend(1, buf, xous::MemoryAddress::new(vaddr), None)).expect("Couldn't send a message to spawn");
		    }
		}

		// tell spawn to switch to the new program
		xous::send_message(spawn.cid, xous::Message::new_scalar(255, entry_point, 0, 0, 0)).expect("Couldn't send a message to spawn");

		// let run_app: fn() -> ! = unsafe { core::mem::transmute(entry_point) };
		// xous::create_thread_0(run_app).expect("Couldn't run app!");
		
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
