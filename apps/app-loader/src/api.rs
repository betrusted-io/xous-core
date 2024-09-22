use std::io::Read;

use gam::{
    APP_MENU_0_APP_LOADER, APP_MENU_1_APP_LOADER, APP_NAME_APP_LOADER, Gam, MenuItem, MenuMatic,
    TextEntryPayload, UxRegistration, menu_matic,
};
use locales::t;
use modals::Modals;
use num_traits::ToPrimitive;

use crate::SERVER_NAME_APP_LOADER;

pub(crate) struct AppLoader {
    gam: Gam,
    modals: Modals,
    auth: [u32; 4],
    ticktimer: ticktimer_server::Ticktimer,
    menu: MenuMatic,
    load_menu: MenuMatic,
    conn: xous::CID,
    apps: Vec<xous_ipc::String<64>>,
    possible_apps: Vec<(xous_ipc::String<64>, usize)>,
    server: Option<String>,
    current_menu: String,
}

impl AppLoader {
    pub(crate) fn new(xns: &xous_names::XousNames, sid: &xous::SID) -> AppLoader {
        // gam
        let gam = Gam::new(xns).expect("can't connect to GAM");

        // the gam token
        let auth = gam
            .register_ux(UxRegistration {
                app_name: xous_ipc::String::from_str(APP_NAME_APP_LOADER),
                ux_type: gam::UxType::Modal,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: Opcode::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                focuschange_id: None,
                rawkeys_id: None,
            })
            .expect("Couldn't register")
            .expect("Didn't get an auth token");

        let modals = Modals::new(xns).expect("Couldn't get modals");

        // a connection to the server
        let conn =
            xns.request_connection_blocking(SERVER_NAME_APP_LOADER).expect("Couldn't connect to server");

        // the ticktimer
        let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

        // the menu
        let set_server_item = MenuItem {
            name: xous_ipc::String::from_str(t!("apploader.menu.setserver", locales::LANG)),
            action_conn: Some(conn),
            action_opcode: Opcode::SetServer.to_u32().unwrap(),
            action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        };
        let close_item = MenuItem {
            name: xous_ipc::String::from_str(t!("apploader.close", locales::LANG)),
            action_conn: None,
            action_opcode: 0,
            action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        };
        let menu = menu_matic(
            vec![set_server_item, close_item.clone()],
            APP_MENU_0_APP_LOADER,
            Some(xous::create_server().unwrap()),
        )
        .expect("Couldn't create menu");
        let load_menu =
            menu_matic(vec![close_item.clone()], APP_MENU_1_APP_LOADER, Some(xous::create_server().unwrap()))
                .expect("Couldn't create menu");

        AppLoader {
            gam,
            modals,
            auth,
            conn,
            ticktimer,
            menu,
            load_menu,
            apps: Vec::new(),
            possible_apps: Vec::new(),
            server: None,
            current_menu: APP_MENU_0_APP_LOADER.to_string(),
        }
    }

    pub(crate) fn add_app(&mut self, index: usize) {
        let (name, menus) = self.possible_apps[index];

        self.modals
            .start_progress(t!("apploader.addapp.loading", locales::LANG), 0, 3, 0)
            .expect("Couldn't set up progress bar");

        //////////////////////////
        // The downloading part //
        //////////////////////////
        let response = match ureq::get(&format!(
            "{}/{}",
            self.server.as_ref().expect("AddApp was somehow called without a server!"),
            name
        ))
        .call()
        {
            Ok(response) => response,
            Err(e) => {
                self.modals
                    .show_notification(
                        &format!("{}{}", t!("apploader.addapp.server_error", locales::LANG), e),
                        None,
                    )
                    .expect("Couldn't show modal");
                return;
            }
        };

        let len = match response.header("Content-Length") {
            Some(len) => len.parse::<usize>().expect("Couldn't parse Content-Length header"),
            None => {
                self.modals
                    .show_notification(t!("apploader.addapp.content_length_error", locales::LANG), None)
                    .expect("Couldn't show modal");
                return;
            }
        };

        let mut memory = xous::map_memory(
            None,
            None,
            len + if len & 0xFFF == 0 { 0 } else { 0x1000 - (len & 0xFFF) },
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map memory");
        response
            .into_reader()
            .read_exact(&mut unsafe { memory.as_slice_mut() }[..len])
            .expect("Couldn't read");

        self.modals.update_progress(1).expect("Couldn't update progress");
        //////////////////////
        // The loading part //
        //////////////////////

        // create the spawn process
        let stub = include_bytes!("spawn.bin");
        let args = xous::ProcessArgs::new(
            stub,
            xous::MemoryAddress::new(0x2050_1000).unwrap(),
            xous::MemoryAddress::new(0x2050_1000).unwrap(),
        );
        let spawn = xous::create_process(args).expect("Couldn't create spawn process");
        log::info!("Spawn PID: {}, Spawn CID: {}", spawn.pid, spawn.cid);

        // perform a ping to make sure that spawn is running
        let result =
            xous::send_message(spawn.cid, xous::Message::new_blocking_scalar(2, 1, 2, 3, 4)).unwrap();
        assert_eq!(xous::Result::Scalar1(2), result);

        self.modals.update_progress(2).expect("Couldn't update progress");

        // load the app from the binary file
        let res = xous::send_message(spawn.cid, xous::Message::new_lend_mut(1, memory, None, None))
            .expect("Couldn't send a message to spawn");
        // we are just going to do some very basic error handling: if the "offset" is None, we are good,
        // otherwise there was a problem TODO: make this better. Perhaps Buffer::from_raw_parts?
        match res {
            xous::Result::MemoryReturned(None, _) => {
                self.modals.update_progress(3).expect("Couldn't update progress")
            }
            _ => {
                self.modals.finish_progress().expect("Couldn't close progressbar");
                self.modals
                    .show_notification(t!("apploader.addapp.error", locales::LANG), None)
                    .expect("Couldn't show modal");
                return;
            }
        }

        //////////////////////
        // back to graphics //
        //////////////////////

        // add its name to GAM
        self.gam.register_name(name.to_str(), self.auth).expect("Couldn't register name");
        for menu in 0..menus {
            self.gam
                .register_name(&format!("{} Submenu {}", name, menu), self.auth)
                .expect("Couldn't register name");
        }

        // add it to the menu
        self.menu.insert_item(
            MenuItem {
                name,
                action_conn: Some(self.conn),
                action_opcode: Opcode::DispatchApp.to_u32().unwrap(),
                action_payload: gam::MenuPayload::Scalar([self.apps.len().try_into().unwrap(), 0, 0, 0]),
                close_on_select: true,
            },
            0,
        );
        self.apps.push(name);

        log::info!("Added app `{}'!", name);

        self.modals.finish_progress().expect("Couldn't close progressbar");
        let _ = self.gam.switch_to_app(APP_NAME_APP_LOADER, self.auth); // try to switch back to the menu
    }

    pub(crate) fn set_server(&mut self) {
        let payload = self
            .modals
            .alert_builder("Server Address")
            .field(
                Some("e.g. http://ip:port".to_string()),
                Some(|payload: TextEntryPayload| match url::Url::parse(payload.as_str()) {
                    Ok(_) => None,
                    Err(e) => Some(xous_ipc::String::from_str(&format!(
                        "{}{}",
                        t!("apploader.setserver.error", locales::LANG),
                        e
                    ))),
                }),
            )
            .build()
            .ok();

        if self.server.is_none() && payload.is_some() {
            self.menu.insert_item(
                MenuItem {
                    name: xous_ipc::String::from_str(t!("apploader.menu.reloadapplist", locales::LANG)),
                    action_conn: Some(self.conn),
                    action_opcode: Opcode::ReloadAppList.to_u32().unwrap(),
                    action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
                    close_on_select: true,
                },
                0,
            );
            self.menu.insert_item(
                MenuItem {
                    name: xous_ipc::String::from_str(t!("apploader.menu.addapp", locales::LANG)),
                    action_conn: Some(self.conn),
                    action_opcode: Opcode::AddAppMenu.to_u32().unwrap(),
                    action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
                    close_on_select: true,
                },
                0,
            );
        }

        self.server = payload.and_then(|p| Some(p.first().as_str().to_string()));
        self.reload_app_list();
    }

    pub(crate) fn open_load_menu(&mut self) {
        self.current_menu = APP_MENU_1_APP_LOADER.to_string();
        let _ = self.gam.switch_to_app(APP_NAME_APP_LOADER, self.auth);
    }

    pub(crate) fn reload_app_list(&mut self) {
        // without a path, the server responds with a JSON list of strings representing the list of app names
        self.modals
            .start_progress(t!("apploader.reloadapplist.loading", locales::LANG), 0, 3, 0)
            .expect("Couldn't start progressbar");

        let old = self.possible_apps.clone();

        // this... disgusting error handling is so that if there is an error on the server side there isn't a
        // panic
        self.possible_apps = match match ureq::get(
            &self.server.as_ref().expect("ReloadAppList was somehow called without a server!"),
        )
        .call()
        {
            Ok(c) => {
                self.modals.update_progress(1).expect("Couldn't update progress");
                c
            }
            Err(e) => {
                self.modals
                    .show_notification(
                        &format!("{}{}", t!("apploader.reloadapplist.connection_error", locales::LANG), e),
                        None,
                    )
                    .expect("Couldn't show modal");
                return;
            }
        }
        .into_json::<Vec<(String, usize)>>()
        {
            Ok(json) => {
                self.modals.update_progress(2).expect("Couldn't update progress");
                json
            }
            Err(e) => {
                self.modals
                    .show_notification(
                        &format!("{}{}", t!("apploader.reloadapplist.json_error", locales::LANG), e),
                        None,
                    )
                    .expect("Couldn't show modal");
                return;
            }
        }
        .iter()
        .map(|(name, menus)| (xous_ipc::String::<64>::from_str(&name), *menus))
        .collect();

        for (old_name, _) in old {
            self.load_menu.delete_item(old_name.as_str().unwrap());
        }

        for (i, (app, _)) in self.possible_apps.iter().enumerate() {
            self.load_menu.insert_item(
                MenuItem {
                    name: xous_ipc::String::from_str(app.to_str()),
                    action_conn: Some(self.conn),
                    action_opcode: Opcode::AddApp.to_u32().unwrap(),
                    action_payload: gam::MenuPayload::Scalar([i.try_into().unwrap(), 0, 0, 0]),
                    close_on_select: true,
                },
                0,
            );
        }
        self.modals.finish_progress().expect("Couldn't close progress bar");

        let _ = self.gam.switch_to_app(APP_NAME_APP_LOADER, self.auth); // try to switch back to the menu
    }

    pub(crate) fn dispatch_app(&self, index: usize) {
        if index < self.apps.len() {
            let name: xous_ipc::String<64> = self.apps[index];
            log::info!("Switching to app `{}'", name);
            self.gam
                .switch_to_app(name.as_ref(), self.auth)
                .expect(&format!("Could not dispatch app `{}'", name));
        } else {
            panic!("Unrecognized app index");
        }
    }

    pub(crate) fn redraw(&mut self) {
        // Properly close the app menu
        // for anyone who needs this I found this in Menu::key_event
        self.gam.relinquish_focus().unwrap();
        xous::yield_slice();

        self.ticktimer.sleep_ms(100).ok(); // yield for a moment to allow the previous menu to close

        // open the submenu if possible
        let _ = self.gam.raise_menu(&self.current_menu);
        // we only ever need the other menu for one thing
        self.current_menu = APP_MENU_0_APP_LOADER.to_string();
    }
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    /// set the server
    SetServer,

    /// get a list of apps from the server
    ReloadAppList,

    /// open the menu for adding apps
    AddAppMenu,

    /// load an app and add it to the menu
    AddApp,

    /// dispatch the app
    DispatchApp,

    /// Redraw the UI
    Redraw,
}
