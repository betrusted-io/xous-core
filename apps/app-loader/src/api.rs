use std::io::Read;
use gam::{Gam, MenuItem, UxRegistration, menu_matic, APP_NAME_APP_LOADER, APP_MENU_0_APP_LOADER, MenuMatic, TextEntryPayload};
use modals::Modals;
use xous::MemoryRange;
use num_traits::ToPrimitive;
use crate::SERVER_NAME_APP_LOADER;

pub(crate) struct AppLoader {
    gam: Gam,
    modals: Modals,
    auth: [u32; 4],
    ticktimer: ticktimer_server::Ticktimer,
    menu: MenuMatic,
    conn: xous::CID,
    apps: Vec<xous_ipc::String<64>>,
    possible_apps: Vec<xous_ipc::String<64>>,
    server: Option<String>,
}

impl AppLoader {
    pub(crate) fn new(xns: &xous_names::XousNames, sid: &xous::SID) -> AppLoader {
	// gam
	let gam = Gam::new(xns).expect("can't connect to GAM");

	// the gam token
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

	let modals = Modals::new(xns).expect("Couldn't get modals");

	// a connection to the server
	let conn = xns.request_connection_blocking(SERVER_NAME_APP_LOADER).expect("Couldn't connect to server");

	// the ticktimer
	let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

	// the menu
	let set_server_item = MenuItem{ name: xous_ipc::String::from_str("Set Server"),
					action_conn: Some(conn),
					action_opcode: Opcode::SetServer.to_u32().unwrap(),
					action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
					close_on_select: true };
	let menu = menu_matic(vec![set_server_item], APP_MENU_0_APP_LOADER, Some(xous::create_server().unwrap())).expect("Couldn't create menu");

	AppLoader{ gam, modals, auth, conn, ticktimer, menu, apps: Vec::new(), possible_apps: Vec::new(), server: None }
    }

    pub(crate) fn add_app(&mut self, index: usize) {
	let name = self.possible_apps[index];

	// load the app from the server
	let response = ureq::get(&format!("{}/{}", self.server.as_ref().expect("AddApp was somehow called without a server!"), name))
	    .call().expect("Couldn't make request");

	let len = response.header("Content-Length").expect("No Content-Length header")
	    .parse::<usize>().expect("Couldn't parse Content-Length header");

	let mut app_file = Vec::with_capacity(len);
	response.into_reader()
	    .read_to_end(&mut app_file).expect("Couldn't read");
	let app_file_slice = app_file.as_slice();

	let memory = unsafe { MemoryRange::new((app_file_slice.as_ptr() as usize) & !0xFFF,
					       len + if len & 0xFFF == 0 { 0 } else { 0x1000 - (len & 0xFFF) }).unwrap() };

	//////////////////////
	// The loading part //
	//////////////////////

	// create the spawn process
	let stub = include_bytes!("spawn.bin");
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
	xous::send_message(spawn.cid, xous::Message::new_lend(1, memory, None, None)).expect("Couldn't send a message to spawn");

	//////////////////////
	// back to graphics //
	//////////////////////

	// add its name to GAM
	self.gam.register_name(name.to_str(), self.auth).expect("Couldn't register name");

	// add it to the menu
	self.menu.insert_item(MenuItem { name,
 					 action_conn: Some(self.conn),
					 action_opcode: Opcode::DispatchApp.to_u32().unwrap(),
					 action_payload: gam::MenuPayload::Scalar([self.apps.len().try_into().unwrap(), 0, 0, 0]),
					 close_on_select: true }, 0);
	self.apps.push(name);

	log::info!("Added app `{}'!", name);

	self.redraw();
    }

    pub(crate) fn set_server(&mut self) {
	let payload = self.modals.alert_builder("Server IP Address:Port")
	// .field(None, Some(|payload: TextEntryPayload| if payload.as_str().trim_start_matches("https://").trim_start_matches("http://").chars().filter(|c| *c == ':').count() == 1 { None } else { Some(xous_ipc::String::from_str("Port is not specified")) }))
	    .field(None, None)
	    .build()
	    .ok();

	if self.server.is_none() && payload.is_some() {
	    self.menu.insert_item(MenuItem { name: xous_ipc::String::from_str("Reload App List"),
					     action_conn: Some(self.conn),
					     action_opcode: Opcode::ReloadAppList.to_u32().unwrap(),
					     action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
					     close_on_select: true }, 0);
	}

	self.server = payload.and_then(|p| Some(p.first().as_str().to_string()));
    }

    pub(crate) fn reload_app_list(&mut self) {
	// without a path, the server responds with a JSON list of strings representing the list of app names
	self.possible_apps = ureq::get(&self.server.as_ref().expect("ReloadAppList was somehow called without a server!"))
	    .call().expect("Couldn't make request")
	    .into_json::<Vec<String>>().expect("Couldn't convert into JSON")
	    .iter()
	    .map(|s| xous_ipc::String::<64>::from_str(&s))
	    .collect();

	for (i, app) in self.possible_apps.iter().enumerate() {
	    self.menu.insert_item(MenuItem { name: xous_ipc::String::from_str("Add ".to_owned()+app.to_str()),
					     action_conn: Some(self.conn),
					     action_opcode: Opcode::AddApp.to_u32().unwrap(),
					     action_payload: gam::MenuPayload::Scalar([i.try_into().unwrap(), 0, 0, 0]),
					     close_on_select: true }, 0);
	}
    }

    pub(crate) fn dispatch_app(&self, index: usize) {
	if index < self.apps.len() {
	    let name: xous_ipc::String<64> = self.apps[index];
	    log::info!("Switching to app `{}'", name);
	    self.gam.switch_to_app(name.as_ref(), self.auth).expect(&format!("Could not dispatch app `{}'", name));
	} else {
	    panic!("Unrecognized app index");
	}
    }

    pub(crate) fn redraw(&self) {
	// Properly close the app menu
	// for anyone who needs this I found this in Menu::key_event
	self.gam.relinquish_focus().unwrap();
	xous::yield_slice();

	self.ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close

	// open the submenu
	self.gam.raise_menu(APP_MENU_0_APP_LOADER).expect("Couldn't raise menu");
    }
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    /// set the server
    SetServer,

    /// get a list of apps from the server
    ReloadAppList,

    /// load an app and add it to the menu
    AddApp,

    /// dispatch the app
    DispatchApp,

    /// Redraw the UI
    Redraw
}
