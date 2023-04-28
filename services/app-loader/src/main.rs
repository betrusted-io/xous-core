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

		// load the app from the binary file
		let bin = include_bytes!("hello");
		let elf = xmas_elf::ElfFile::new(bin.as_slice()).expect("Couldn't load elf!");
		let entry_point = elf.header.pt2.entry_point();
		let code_ph = elf
		    .program_iter()
		    .find(|ph| (ph.virtual_addr()..ph.virtual_addr() + ph.mem_size()).contains(&entry_point))
		    .expect("Couldn't find segment with entry point!");
		if let xmas_elf::program::SegmentData::Undefined(code) = code_ph.get_data(&elf).expect("Couldn't load section!") {
		    let remainder = if code.len() & 0xFFF == 0 {
			0
		    } else {
			0x1000 - (code.len() & 0xFFF)
		    };
		    let mut target_memory = xous::map_memory(
			None,
			Some(core::num::NonZeroUsize::new((code_ph.virtual_addr() & !0xFFF) as usize).unwrap()),
			code.len() + remainder,
			xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::X
		    ).expect("Couldn't allocate new memory");
		    for (src, dest) in code.iter().zip(target_memory.as_slice_mut::<u8>()) {
			*dest = *src;
		    }
		    
		    let entry_offset = entry_point - code_ph.virtual_addr();
		    let entry_point = unsafe { code.as_ptr().add(entry_offset as usize) };
		    
		    let run_app: fn() -> ! = unsafe { core::mem::transmute(entry_point) };
		    std::thread::spawn(move || {
			run_app();
		    });
		}
		
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
