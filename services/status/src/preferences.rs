use core::{convert::TryFrom, fmt::Display};

use num_traits::*;
use userprefs::{Manager, UserPrefs};

use std::io::Write;

use locales::t;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, PartialOrd)]
enum PrefsManOp {
    RadioOnOnBoot,
    ConnectKnownNetworksOnBoot,
    AutobacklightOnBoot,
    AutobacklightTimeout,
}

impl Display for PrefsManOp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AutobacklightOnBoot => write!(f, "Autobacklight enabled on boot"),
            Self::AutobacklightTimeout => write!(f, "Autobacklight timeout"),
            Self::ConnectKnownNetworksOnBoot => write!(f, "Connect to known networks on boot"),
            Self::RadioOnOnBoot => write!(f, "Enable WiFi on boot"),
        }
    }
}

#[derive(Debug)]
enum PrefsManError {
    PrefsError(userprefs::Error),
}

impl From<userprefs::Error> for PrefsManError {
    fn from(e: userprefs::Error) -> Self {
        Self::PrefsError(e)
    }
}

impl Display for PrefsManError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PrefsManError::PrefsError(e) => write!(f, "Preferences engine error: {:?}", e),
        }
    }
}

struct PrefsMan {
    up: Manager,
    modals: modals::Modals,
}

impl PrefsMan {
    fn new(xns: &xous_names::XousNames) -> Self {
        Self {
            up: Manager::new(),
            modals: modals::Modals::new(&xns).unwrap(),
        }
    }

    fn actions(&self) -> Vec<PrefsManOp> {
        use PrefsManOp::*;

        vec![
            RadioOnOnBoot,
            ConnectKnownNetworksOnBoot,
            AutobacklightOnBoot,
            AutobacklightTimeout,
        ]
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

    fn consume_menu_action(&mut self, action: PrefsManOp) {
        let resp = match action {
            PrefsManOp::AutobacklightOnBoot => self.autobacklight_on_boot(),
            PrefsManOp::RadioOnOnBoot => self.radio_on_on_boot(),
            PrefsManOp::ConnectKnownNetworksOnBoot => self.connect_known_networks_on_boot(),
            PrefsManOp::AutobacklightTimeout => self.autobacklight_timeout(),
        };

        resp.unwrap_or_else(|error| self.show_error_modal(error));
    }

    fn show_error_modal(&self, e: PrefsManError) {
        self.modals
            .show_notification(
                format!("{}: {}", t!("wlan.error", xous::LANG), e).as_str(),
                None,
            )
            .unwrap()
    }
}

impl PrefsMan {
    fn autobacklight_on_boot(&mut self) -> Result<(), PrefsManError> {
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

    fn autobacklight_timeout(&self) -> Result<(), PrefsManError> {
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

    fn radio_on_on_boot(&mut self) -> Result<(), PrefsManError>{
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

    fn connect_known_networks_on_boot(&mut self) -> Result<(), PrefsManError>{
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

    let mut hello = PrefsMan::new(&xns);

    let menu_conn = xous::connect(sid).unwrap();
    hello.claim_menumatic_menu(menu_conn);

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        match FromPrimitive::from_usize(msg.body.id()) {
            Some(other) => {
                hello.consume_menu_action(other);
            }
            _ => {
                log::error!("Got unknown message");
            }
        }
    }
}
