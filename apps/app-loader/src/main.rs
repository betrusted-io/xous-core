mod api;
use api::*;
use num_traits::FromPrimitive;
use xous_ipc::Buffer;

const SERVER_NAME_APP_LOADER: &str = "_App Loader_";

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(SERVER_NAME_APP_LOADER, None).expect("Couldn't create server");

    // start off by adding hello world
    let mut app_loader = AppLoader::new(&xns, &sid);
    app_loader.add_app("Hello");

    loop {
	let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()).expect("Couldn't load message") {
	    Opcode::LoadApp => {
		let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
		let load_app_req = buffer.to_original::<LoadAppRequest, _>().unwrap();

		// create the spawn process
		let stub = include_bytes!("../../../target/riscv32imac-unknown-xous-elf/debug/spawn.bin");
		let args = xous::ProcessArgs::new(stub, xous::MemoryAddress::new(0x2050_1000).unwrap(), xous::MemoryAddress::new(0x2050_1000).unwrap());
		let spawn = xous::create_process(args).expect("Couldn't create spawn process");
		log::info!("Spawn PID: {}, Spawn CID: {}", spawn.pid, spawn.cid);

		// perform a ping to make sure that spawn is running
		let result = xous::send_message(
		    spawn.cid,
		    xous::Message::new_blocking_scalar(2, 1, 2, 3, 4),
		)
		    .unwrap();
		assert_eq!(xous::Result::Scalar1(2), result);

		// load the app from the binary file
		let bin = include_bytes!("../../../target/riscv32imac-unknown-xous-elf/debug/hello");
		let bin_len = bin.len();
		let bin_loc = bin.as_ptr() as usize;
		let buf = unsafe { xous::MemoryRange::new(bin_loc & !0xFFF, bin_len + if bin_len & 0xFFF == 0 { 0 } else {0x1000 - (bin_len & 0xFFF)}).expect("Couldn't create a buffer from the segment") };
		xous::send_message(spawn.cid, xous::Message::new_lend(1, buf, None, None)).expect("Couldn't send a message to spawn");

		log::info!("Loaded app `{}'!", load_app_req.name);
	    },
	    Opcode::DispatchApp => {
		let index = msg.body.scalar_message().expect("Not a scalar").arg1;
		app_loader.dispatch_app(index);
	    },
	    Opcode::Redraw => {
		app_loader.redraw();
	    },
	}
    }
}
