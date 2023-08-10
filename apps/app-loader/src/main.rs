mod api;
use api::*;
use num_traits::FromPrimitive;

const SERVER_NAME_APP_LOADER: &str = "_App Loader_";

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xns.register_name(SERVER_NAME_APP_LOADER, None).expect("Couldn't create server");

    // start off by adding hello world
    let mut app_loader = AppLoader::new(&xns, &sid);

    loop {
	let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()).expect("Couldn't load message") {
	    Opcode::SetServer => {
		app_loader.set_server();
	    },
	    Opcode::ReloadAppList => {
		app_loader.reload_app_list();
	    },
	    Opcode::AddAppMenu => {
		app_loader.open_load_menu();
	    },
	    Opcode::AddApp => {
		let index = msg.body.scalar_message().expect("Not a scalar").arg1;
		app_loader.add_app(index);
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
