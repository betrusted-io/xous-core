mod account;
pub mod api;
mod manager;

use crate::account::Account;
use crate::manager::{Manager, TrustMode};
pub use api::*;
use chat::Chat;
use locales::t;
use modals::Modals;
use std::io::{Error, ErrorKind};

/// PDDB Dict for sigchat keys
const SIGCHAT_ACCOUNT: &str = "sigchat.account";
const SIGCHAT_DIALOGUE: &str = "sigchat.dialogue";

const WIFI_TIMEOUT: u32 = 10; // seconds

#[cfg(not(target_os = "xous"))]
pub const HOSTED_MODE: bool = true;
#[cfg(target_os = "xous")]
pub const HOSTED_MODE: bool = false;

//#[derive(Debug)]
pub struct SigChat<'a> {
    chat: &'a Chat,
    manager: Option<Manager>,
    netmgr: net::NetManager,
    modals: Modals,
}
impl<'a> SigChat<'a> {
    pub fn new(chat: &Chat) -> SigChat {
        let xns = xous_names::XousNames::new().unwrap();
        let modals = Modals::new(&xns).expect("can't connect to Modals server");
        let pddb = pddb::Pddb::new();
        pddb.try_mount();
        SigChat {
            chat: chat,
            manager: match Account::read(SIGCHAT_ACCOUNT) {
                Ok(account) => Some(Manager::new(account, TrustMode::OnFirstUse)),
                Err(_) => None,
            },
            netmgr: net::NetManager::new(),
            modals: modals,
        }
    }

    /// Connect to the Signal servers
    ///
    pub fn connect(&mut self) -> Result<bool, Error> {
        log::info!("Attempting connect to Signal server");
        if self.wifi() {
            if self.manager.is_none() {
                log::info!("Setting up Signal Account Manager");
                let account = match Account::read(SIGCHAT_ACCOUNT) {
                    Ok(account) => account,
                    Err(_) => self.account_setup()?,
                };
                self.chat
                    .set_status_text(t!("sigchat.status.connecting", locales::LANG));
                self.chat.set_busy_state(true);
                self.manager = Some(Manager::new(account, TrustMode::OnFirstUse));
                self.chat.set_busy_state(false);
            }
            if self.manager.is_some() {
                log::info!("Signal Account Manager OK");
                self.chat
                    .set_status_text(t!("sigchat.status.online", locales::LANG));
                Ok(true)
            } else {
                log::info!("failed to setup Signal Account Manager");
                self.chat
                    .set_status_text(t!("sigchat.status.offline", locales::LANG));
                Ok(false)
            }
        } else {
            self.modals
                .show_notification(t!("sigchat.wifi.warning", locales::LANG), None)
                .expect("notification failed");
            Ok(false)
        }
    }

    /// Setup a Signal Account via Registration or Linking,
    /// or abort setup and read existing chat Dialogues in pddb
    ///
    fn account_setup(&mut self) -> Result<Account, Error> {
        log::info!("Attempting to setup a Signal Account");
        self.modals
            .add_list_item(t!("sigchat.account.link", locales::LANG))
            .expect("failed add list item");
        self.modals
            .add_list_item(t!("sigchat.account.register", locales::LANG))
            .expect("failed add list item");
        self.modals
            .add_list_item(t!("sigchat.account.offline", locales::LANG))
            .expect("failed add list item");
        self.modals
            .get_radiobutton(t!("sigchat.account.title", locales::LANG))
            .expect("failed radiobutton modal");
        match self.modals.get_radio_index() {
            Ok(index) => match index {
                0 => Ok(self.account_link()?),
                1 => Ok(self.account_register()?),
                2 => {
                    log::info!("account setup aborted");
                    Err(Error::new(ErrorKind::Other, "account setup aborted"))
                }
                _ => {
                    log::warn!("invalid index");
                    Err(Error::new(ErrorKind::Other, "invalid radio index"))
                }
            },
            Err(e) => {
                log::warn!("failed to present account setup radio buttons: {:?}", e);
                Err(Error::new(
                    ErrorKind::Other,
                    "failed to present account setup radio buttons",
                ))
            }
        }
    }

    /// Link this device to an existing Signal Account
    ///
    /// Signal allows to link additional devices to your primary device (e.g. Signal-Android).
    /// Note that currently Signal allows up to three linked devices per primary.
    ///
    pub fn account_link(&self) {
        self.modals
            .show_notification(&"sorry - linking is not implemented yet", None)
            .expect("notification failed");
    }

    /// Register a new Signal Account with this as the primary device.
    ///
    /// A Signal Account requires a phone number to receive SMS or incoming calls for registration & validation.
    /// The phone number must include the country calling code, i.e. the number must start with a "+" sign.
    /// Warning: this will disable the authentication of your phone as a primary device.
    ///
    pub fn account_register(&mut self) -> Result<Account, Error> {
        log::info!("Attempting to Register a new Signal Account");
        self.modals
            .show_notification(&"sorry - registration is not implemented yet", None)
            .expect("notification failed");
        match self.number_modal() {
            Ok(number) => {
                log::info!("registration phone number = {:?}", number);
                match Account::new(SIGCHAT_ACCOUNT) {
                    Ok(mut account) => match account.set_number(&number.to_string()) {
                        Ok(_number) => {
                            self.manager = Some(Manager::new(account, TrustMode::OnFirstUse));
                        }
                        Err(e) => log::warn!("failed to set Account number: {e}"),
                    },
                    Err(e) => log::warn!("failed to create new Account: {e}"),
                }
            }
            Err(e) => log::warn!("failed to get phone number: {e}"),
        }
        Err(Error::new(ErrorKind::Other, "not implmented"))
    }

    /// Attempts to obtain a phone number from the user.
    ///
    /// A Signal Account requires a phone number to receive SMS or incoming calls for registration & validation.
    /// The phone number must include the country calling code, i.e. the number must start with a "+" sign.
    ///
    #[allow(dead_code)]
    fn number_modal(&mut self) -> Result<String, Error> {
        let mut builder = self
            .modals
            .alert_builder(t!("sigchat.number.title", locales::LANG));
        let builder = builder.field(Some(t!("sigchat.number", locales::LANG).to_string()), None);
        match builder.build() {
            Ok(payloads) => match payloads.content()[0].content.as_str() {
                Ok(number) => {
                    log::info!("registration phone number = {:?}", number);
                    Ok(number.to_string())
                }
                Err(e) => Err(Error::new(ErrorKind::InvalidData, e)),
            },
            Err(_) => Err(Error::from(ErrorKind::ConnectionRefused)),
        }
    }

    pub fn redraw(&self) {
        self.chat.redraw();
    }

    pub fn dialogue_set(&self, room_alias: Option<&str>) {
        self.chat
            .dialogue_set(SIGCHAT_DIALOGUE, room_alias)
            .expect("failed to set dialogue");
    }

    pub fn help(&self) {
        self.chat.help();
    }

    /// Returns true if wifi is connected
    ///
    /// If wifi is not connected then a modal offers to "Connect to wifi?"
    /// and tries for 10 seconds before representing.
    ///
    pub fn wifi(&self) -> bool {
        if HOSTED_MODE {
            return true;
        }

        if let Some(conf) = self.netmgr.get_ipv4_config() {
            if conf.dhcp == com_rs::DhcpState::Bound {
                return true;
            }
        }

        while self.wifi_try_modal() {
            self.netmgr.connection_manager_wifi_on_and_run().unwrap();
            self.modals
                .start_progress("Connecting ...", 0, 10, 0)
                .expect("no progress bar");
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            for wait in 0..WIFI_TIMEOUT {
                tt.sleep_ms(1000).unwrap();
                self.modals
                    .update_progress(wait)
                    .expect("no progress update");
                if let Some(conf) = self.netmgr.get_ipv4_config() {
                    if conf.dhcp == com_rs::DhcpState::Bound {
                        self.modals
                            .finish_progress()
                            .expect("failed progress finish");
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Returns true if "Connect to WiFi?" yes option is chosen
    ///
    fn wifi_try_modal(&self) -> bool {
        self.modals.add_list_item("yes").expect("failed radio yes");
        self.modals.add_list_item("no").expect("failed radio no");
        self.modals
            .get_radiobutton("Connect to WiFi?")
            .expect("failed radiobutton modal");
        match self.modals.get_radio_index() {
            Ok(button) => button == 0,
            _ => false,
        }
    }
}
