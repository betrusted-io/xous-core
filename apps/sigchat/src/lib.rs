mod account;
pub mod api;
mod manager;

use crate::account::Account;
pub use api::*;
use chat::Chat;
use locales::t;
use modals::Modals;
use std::io::{Error, ErrorKind};

/// PDDB Dict for sigchat keys
const SIGCHAT_DIALOGUE: &str = "sigchat.dialogue";

const WIFI_TIMEOUT: u32 = 10; // seconds

#[cfg(not(target_os = "xous"))]
pub const HOSTED_MODE: bool = true;
#[cfg(target_os = "xous")]
pub const HOSTED_MODE: bool = false;

//#[derive(Debug)]
pub struct SigChat<'a> {
    chat: &'a Chat,
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
            netmgr: net::NetManager::new(),
            modals: modals,
        }
    }


    /// Connect to the Signal servers
    ///
    pub fn connect(&mut self) -> bool {
        log::info!("Attempting connect to Signal server");
        if self.wifi() {
        } else {
            self.modals
                .show_notification(t!("sigchat.wifi.warning", locales::LANG), None)
                .expect("notification failed");
        }
        self.dialogue_set(None);
        false
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
