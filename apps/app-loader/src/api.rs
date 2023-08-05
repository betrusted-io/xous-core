use gam::{Gam, MenuItem, UxRegistration, menu_matic, APP_NAME_APP_LOADER, APP_MENU_0_APP_LOADER, MenuMatic};
use xous_ipc::Buffer;
use num_traits::ToPrimitive;
use crate::SERVER_NAME_APP_LOADER;

pub(crate) struct AppLoader {
    gam: Gam,
    auth: [u32; 4],
    ticktimer: ticktimer_server::Ticktimer,
    menu: MenuMatic,
    conn: xous::CID,
    apps: Vec<xous_ipc::String<64>>,
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

	// a connection to the server
	let conn = xns.request_connection_blocking(SERVER_NAME_APP_LOADER).expect("Couldn't connect to server");

	// the ticktimer
	let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

	// the menu
	let menu = menu_matic(Vec::new(), APP_MENU_0_APP_LOADER, Some(xous::create_server().unwrap())).expect("Couldn't create menu");

	AppLoader{ gam, auth, conn, ticktimer, menu, apps: Vec::new() }
    }

    pub(crate) fn add_app(&mut self, name: &str) {
	let buf = Buffer::into_buf(LoadAppRequest { name: xous_ipc::String::from_str(name) })
	    .expect("Couldn't create buffer");
	buf.send(self.conn,
		 Opcode::LoadApp.to_u32().unwrap()).expect("Couldn't send message");

	// add its name to GAM
	self.gam.register_name(name, self.auth).expect("Couldn't register name");

	// add it to the menu
	self.menu.add_item(MenuItem { name: xous_ipc::String::from_str(name),
 				 action_conn: Some(self.conn),
				 action_opcode: Opcode::DispatchApp.to_u32().unwrap(),
				 action_payload: gam::MenuPayload::Scalar([self.apps.len().try_into().unwrap(), 0, 0, 0]),
				 close_on_select: true });
	self.apps.push(xous_ipc::String::from_str(name));
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
    /// to load an app into ram
    LoadApp,

    /// dispatch the app
    DispatchApp,

    /// Redraw the UI
    Redraw
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct LoadAppRequest {
    pub name: xous_ipc::String<64>
}
