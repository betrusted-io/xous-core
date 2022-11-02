use crate::wifi;
use locales::t;
use num_traits::*;
use std::{fmt::Display, collections::HashMap};
use userprefs::Manager;

pub trait PrefHandler {
    // If handle() returns true, it has handled the operation.
    fn handle(&mut self, op: usize) -> bool;

    fn claim_menumatic_menu(&self, cid: xous::CID);
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, PartialOrd)]
enum DevicePrefsOp {
    RadioOnOnBoot,
    ConnectKnownNetworksOnBoot,
    AutobacklightOnBoot,
    AutobacklightTimeout,
    KeyboardLayout,
    WLANMenu,
}

impl Display for DevicePrefsOp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AutobacklightOnBoot => write!(f, "Autobacklight enabled on boot"),
            Self::AutobacklightTimeout => write!(f, "Autobacklight timeout"),
            Self::ConnectKnownNetworksOnBoot => write!(f, "Connect to known networks on boot"),
            Self::RadioOnOnBoot => write!(f, "Enable WiFi on boot"),
            Self::KeyboardLayout => write!(f, "Keyboard layout"),
            Self::WLANMenu => write!(f, "WLAN settings"),
        }
    }
}

#[derive(Debug)]
enum DevicePrefsError {
    PrefsError(userprefs::Error),
}

impl From<userprefs::Error> for DevicePrefsError {
    fn from(e: userprefs::Error) -> Self {
        Self::PrefsError(e)
    }
}

impl Display for DevicePrefsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DevicePrefsError::PrefsError(e) => write!(f, "Preferences engine error: {:?}", e),
        }
    }
}

struct DevicePrefs {
    up: Manager,
    modals: modals::Modals,
    gam: gam::Gam,
}

impl PrefHandler for DevicePrefs {
    fn handle(&mut self, op: usize) -> bool {
        match FromPrimitive::from_usize(op) {
            Some(other) => {
                self.consume_menu_action(other);

                true
            }
            _ => {
                log::error!("Got unknown message");
                false
            }
        }
    }

    fn claim_menumatic_menu(&self, cid: xous::CID) {
        let mut menus = self
            .actions()
            .iter()
            .map(|action| gam::MenuItem {
                name: xous_ipc::String::from_str(&action.to_string()),
                action_conn: Some(cid),
                action_opcode: action.to_u32().unwrap(),
                action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
                close_on_select: true,
            })
            .collect::<Vec<gam::MenuItem>>();

        menus.push(gam::MenuItem {
            name: xous_ipc::String::from_str(t!("mainmenu.closemenu", xous::LANG)),
            action_conn: None,
            action_opcode: 0,
            action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        });

        gam::menu_matic(menus, gam::PREFERENCES_MENU_NAME, None);
    }
}

impl DevicePrefs {
    fn new(xns: &xous_names::XousNames) -> Self {
        Self {
            up: Manager::new(),
            modals: modals::Modals::new(&xns).unwrap(),
            gam: gam::Gam::new(&xns).unwrap(),
        }
    }

    fn actions(&self) -> Vec<DevicePrefsOp> {
        use DevicePrefsOp::*;

        vec![
            RadioOnOnBoot,
            ConnectKnownNetworksOnBoot,
            AutobacklightOnBoot,
            AutobacklightTimeout,
            KeyboardLayout,
            WLANMenu,
        ]
    }

    fn consume_menu_action(&mut self, action: DevicePrefsOp) {
        let resp = match action {
            DevicePrefsOp::AutobacklightOnBoot => self.autobacklight_on_boot(),
            DevicePrefsOp::RadioOnOnBoot => self.radio_on_on_boot(),
            DevicePrefsOp::ConnectKnownNetworksOnBoot => self.connect_known_networks_on_boot(),
            DevicePrefsOp::AutobacklightTimeout => self.autobacklight_timeout(),
            DevicePrefsOp::KeyboardLayout => self.keyboard_layout(),
            DevicePrefsOp::WLANMenu => self.wlan_menu(),
        };

        resp.unwrap_or_else(|error| self.show_error_modal(error));
    }

    fn show_error_modal(&self, e: DevicePrefsError) {
        self.modals
            .show_notification(
                format!("{}: {}", t!("wlan.error", xous::LANG), e).as_str(),
                None,
            )
            .unwrap()
    }
}

impl DevicePrefs {
    fn autobacklight_on_boot(&mut self) -> Result<(), DevicePrefsError> {
        let cv = self.up.autobacklight_on_boot_or_default()?;

        self.modals.add_list(vec!["Yes", "No"]).unwrap();

        let new_result = yes_no_to_bool(
            self.modals
                .get_radiobutton(&format!("Current status: {}", bool_to_yes_no(cv)))
                .unwrap()
                .as_str(),
        );

        Ok(self.up.set_autobacklight_on_boot(new_result)?)
    }

    fn autobacklight_timeout(&self) -> Result<(), DevicePrefsError> {
        let cv = {
            let mut res = self.up.autobacklight_timeout_or_default()?;

            log::debug!("backlight timeout in store: {}", res);

            if res == 0 {
                res = 10;
            }

            res
        };

        log::debug!("backlight timeout in store after closure: {}", cv);

        let raw_timeout = self
            .modals
            .alert_builder("Autobacklight timeout in seconds:")
            .field(
                Some(cv.to_string()),
                Some(|tf| match tf.as_str().parse::<u64>() {
                    Ok(_) => None,
                    Err(_) => Some(xous_ipc::String::from_str(
                        "Timeout must be a positive number!",
                    )),
                }),
            )
            .build()
            .unwrap();

        let new_timeout = raw_timeout.first().as_str().parse::<u64>().unwrap(); // we know this is a number, we checked with validator;

        Ok(self.up.set_autobacklight_timeout(new_timeout)?)
    }

    fn radio_on_on_boot(&mut self) -> Result<(), DevicePrefsError> {
        let cv = self.up.radio_on_on_boot_or_default()?;

        self.modals.add_list(vec!["Yes", "No"]).unwrap();

        let new_result = yes_no_to_bool(
            self.modals
                .get_radiobutton(&format!("Current status: {}", bool_to_yes_no(cv)))
                .unwrap()
                .as_str(),
        );

        Ok(self.up.set_radio_on_on_boot(new_result)?)
    }

    fn connect_known_networks_on_boot(&mut self) -> Result<(), DevicePrefsError> {
        let cv = self.up.connect_known_networks_on_boot_or_default()?;

        self.modals.add_list(vec!["Yes", "No"]).unwrap();

        let new_result = yes_no_to_bool(
            self.modals
                .get_radiobutton(&format!("Current status: {}", bool_to_yes_no(cv)))
                .unwrap()
                .as_str(),
        );

        Ok(self.up.set_connect_known_networks_on_boot(new_result)?)
    }

    fn wlan_menu(&self) -> Result<(), DevicePrefsError> {
        log::info!("wlan menu invoked");
        std::thread::sleep(std::time::Duration::from_millis(100));
        self.gam.raise_menu(gam::WIFI_MENU_NAME).unwrap();

        Ok(())
    }

    fn keyboard_layout(&mut self) -> Result<(), DevicePrefsError> {
        let kl = self.up.keyboard_layout_or_default()?;

        let mut mappings = HashMap::new();

        mappings.insert("QWERTY", 0 as usize);
        mappings.insert("AZERTY", 1);
        mappings.insert("QWERTZ", 2);
        mappings.insert("Dvorak", 3);

        self.modals
            .add_list(mappings.keys().cloned().collect())
            .unwrap();

        let new_result = self
            .modals
            .get_radiobutton(&format!("Current layout: {}", keyboard::KeyMap::from(kl)))
            .unwrap();
        
        let new_result = mappings[new_result.as_str()];

        Ok(self.up.set_keyboard_layout(new_result)?)
    }
}

fn yes_no_to_bool(val: &str) -> bool {
    match val.to_ascii_lowercase().as_str() {
        "yes" => true,
        "no" => false,
        _ => unreachable!("cannot go here!"),
    }
}

fn bool_to_yes_no(val: bool) -> String {
    match val {
        true => "yes".to_owned(),
        false => "no".to_owned(),
    }
}

pub fn start_background_thread() {
    std::thread::spawn(|| run_menu_thread());
}

fn run_menu_thread() {
    let xns = xous_names::XousNames::new().unwrap();

    let sid = xous::create_server().unwrap();
    let menu_conn = xous::connect(sid).unwrap();

    let mut handlers: Vec<Box<dyn PrefHandler>> = vec![
        Box::new(DevicePrefs::new(&xns)),
        Box::new(wifi::WLANMan::new(&xns)),
    ];

    // claim menumatic's on all prefhandlers for this thread
    for handler in handlers.iter_mut() {
        handler.claim_menumatic_menu(menu_conn);
    }

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        let op = msg.body.id();

        for handler in handlers.iter_mut() {
            if handler.handle(op) {
                log::debug!("handler found!");
                break;
            }

            log::debug!("handler not found, iterating...");
        }
    }
}
