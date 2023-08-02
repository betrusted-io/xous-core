use gam::{Gam, MenuItem, UxRegistration, menu_matic, APP_NAME_APP_LOADER, APP_MENU_0_APP_LOADER};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive, FromPrimitive};

const SERVER_NAME_APP_LOADER: &str = "_App Loader_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum Opcode {
    /// to load an app into ram
    LoadApp,

    /// dispatch the app
    DispatchApp,

    /// Redraw the UI
    Redraw
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct LoadAppRequest {
    pub name: xous_ipc::String<64>
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(SERVER_NAME_APP_LOADER, None).expect("Couldn't create server");
    let self_conn = xns.request_connection_blocking(SERVER_NAME_APP_LOADER).expect("Couldn't connect to server");

    // the ticktimer
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
    
    let gam = Gam::new(&xns).expect("can't connect to GAM");
    let auth = gam.register_ux(UxRegistration {
	app_name: xous_ipc::String::from_str(APP_NAME_APP_LOADER),
	ux_type: gam::UxType::Modal,
	predictor: None,
	listener: sid.to_array(),
	redraw_id: Opcode::Redraw.to_u32().unwrap(),
	gotinput_id: None,
        audioframe_id: None,
	focuschange_id: None,
	rawkeys_id: None
    }).expect("Couldn't register").expect("Didn't get an auth token");
    
    let menu = menu_matic(Vec::new(), APP_MENU_0_APP_LOADER, Some(xous::create_server().unwrap())).expect("Couldn't create menu");

    let mut apps = Vec::new();

    // start off by adding hello world
    let buf = Buffer::into_buf(LoadAppRequest { name: xous_ipc::String::from_str("hello") }).expect("Couldn't create buffer");
    buf.send(self_conn,
	     Opcode::LoadApp.to_u32().unwrap()).expect("Couldn't send message");

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
		log::info!("Result of ping: {:?}", result);
		
		// load the app from the binary file
		let bin = include_bytes!("hello");
		let bin_len = bin.len();
		let bin_loc = bin.as_ptr() as usize;
		let buf = unsafe { xous::MemoryRange::new(bin_loc & !0xFFF, bin_len + if bin_len & 0xFFF == 0 { 0 } else {0x1000 - (bin_len & 0xFFF)}).expect("Couldn't create a buffer from the segment") };
		xous::send_message(spawn.cid, xous::Message::new_lend(1, buf, None, None)).expect("Couldn't send a message to spawn");

		menu.add_item(MenuItem { name: load_app_req.name,
					 action_conn: Some(self_conn),
					 action_opcode: Opcode::DispatchApp.to_u32().unwrap(),
					 action_payload: gam::MenuPayload::Scalar([apps.len().try_into().unwrap(), 0, 0, 0]),
					 close_on_select: true });
		apps.push(load_app_req.name);
		log::info!("Added app `{}'!", load_app_req.name);
	    },
	    Opcode::DispatchApp => {
		let index = msg.body.scalar_message().expect("Not a scalar").arg1;
		if index < apps.len() {
		    let name: xous_ipc::String<64> = apps[index];
		    gam.switch_to_app(name.as_ref(), auth).expect(&format!("Could not dispatch app `{}'", name));
		} else {
		    panic!("Unrecognized app index");
		}
	    },
	    Opcode::Redraw => {
		// Properly close the app menu
		// for anyone who needs this I found this in Menu::key_event
		gam.relinquish_focus().unwrap();
		xous::yield_slice();
		
		ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close

		// open the submenu
		gam.raise_menu(APP_MENU_0_APP_LOADER).expect("Couldn't raise menu");
	    },
	}
    }
}
