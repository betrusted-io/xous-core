use crate::preferences::PrefHandler;
use core::fmt::Display;
use locales::t;
use net::ScanState;
use num_traits::*;
use std::io::Write;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, PartialOrd)]
pub enum WlanManOp {
    ScanForNetworks = 50,
    Status,
    AddNetworkManually,
    KnownNetworks,
    DeleteNetwork,
}

impl Display for WlanManOp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AddNetworkManually => write!(f, "{}", t!("wlan.manual_add", locales::LANG)),
            Self::ScanForNetworks => write!(f, "{}", t!("wlan.scan", locales::LANG)),
            Self::Status => write!(f, "{}", t!("wlan.status", locales::LANG)),
            Self::DeleteNetwork => write!(f, "{}", t!("wlan.delete", locales::LANG)),
            Self::KnownNetworks => write!(f, "{}", t!("wlan.list_known", locales::LANG)),
        }
    }
}

#[derive(Debug)]
enum WLANError {
    UnderlyingError(xous::Error),
    PDDBWriteError(usize, usize),
    PDDBIoError(std::io::Error),
}

impl Display for WLANError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WLANError::UnderlyingError(inner_error) => {
                write!(f, "Underlying Xous error: {:?}", inner_error)
            }
            WLANError::PDDBWriteError(written, total) => {
                write!(
                    f,
                    "PDDB write error, written {} of {} total",
                    written, total
                )
            }
            WLANError::PDDBIoError(err) => {
                write!(f, "PDDB IO error, {}", err)
            }
        }
    }
}

impl From<xous::Error> for WLANError {
    fn from(v: xous::Error) -> Self {
        Self::UnderlyingError(v)
    }
}

impl From<std::io::Error> for WLANError {
    fn from(v: std::io::Error) -> Self {
        Self::PDDBIoError(v)
    }
}

pub struct WLANMan {
    com: com::Com,
    netmgr: net::NetManager,
    modals: modals::Modals,
    pddb: pddb::Pddb,
}

impl PrefHandler for WLANMan {
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

    fn claim_menumatic_menu(&mut self, cid: xous::CID) {
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
            name: xous_ipc::String::from_str(t!("mainmenu.closemenu", locales::LANG)),
            action_conn: None,
            action_opcode: 0,
            action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        });

        gam::menu_matic(menus, gam::WIFI_MENU_NAME, None);
    }
}

impl WLANMan {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        Self {
            com: com::Com::new(&xns).unwrap(),
            netmgr: net::NetManager::new(),
            modals: modals::Modals::new(&xns).unwrap(),
            pddb: pddb::Pddb::new(),
        }
    }

    pub fn actions(&self) -> Vec<WlanManOp> {
        use WlanManOp::*;

        vec![
            ScanForNetworks,
            Status,
            AddNetworkManually,
            KnownNetworks,
            DeleteNetwork,
        ]
    }
    #[allow(dead_code)] // just in case we need this later
    fn set_wlan_state(&mut self, state: bool) -> Result<(), WLANError> {
        match state {
            true => {
                // on
                self.netmgr.connection_manager_wifi_on_and_run()?;
            }
            false => {
                // off
                self.netmgr.connection_manager_wifi_off_and_stop()?;
            }
        };

        Ok(())
    }

    fn add_new_ssid(&mut self) -> Result<(), WLANError> {
        let connection_data = self
            .modals
            .alert_builder(t!("wlan.ssid_entry", locales::LANG))
            .field(
                Some("SSID".to_string()),
                Some(|text| {
                    if text.as_str().is_empty() {
                        return Some(xous_ipc::String::from_str("SSID cannot be empty"));
                    }

                    None
                }),
            )
            .field(
                Some(t!("wlan.password", locales::LANG).to_string()),
                None,
            )
            .build()
            .unwrap();

        let content = connection_data.content();

        self.store_connection_info(content[0].as_str(), content[1].as_str())
    }

    fn store_connection_info(&mut self, ssid: &str, pass: &str) -> Result<(), WLANError> {
        self.netmgr.connection_manager_stop().unwrap();
        match self.pddb.get(
            net::AP_DICT_NAME,
            &ssid,
            Some(".System"),
            true,
            true,
            Some(com::api::WF200_PASS_MAX_LEN),
            Some(|| {}),
        ) {
            Ok(mut entry) => {
                match entry.write(&pass.as_bytes()) {
                    Ok(len) => {
                        if len != pass.len() {
                            Err(WLANError::PDDBWriteError(len, pass.len()))
                        } else {
                            // for now, we should always call flush at the end of a routine; perhaps in the
                            // future we'll have a timer that automatically syncs the pddb
                            entry.flush().expect("couldn't sync pddb cache");
                            // restart the connection manager now that the key combo has been committed
                            self.netmgr.connection_manager_run().unwrap();
                            Ok(())
                        }
                    }
                    Err(e) => Err(WLANError::PDDBIoError(e)),
                }
            }
            Err(e) => Err(WLANError::PDDBIoError(e)),
        }
    }

    fn scan_networks(&self) -> Result<(Vec<String>, ScanState), WLANError> {
        let (scan_result, state) = self.netmgr.wifi_get_ssid_list()?;

        Ok((scan_result
            .iter()
            .map(|ssid| ssid.name.to_string())
            .collect(),
            state
            )
        )
    }

    fn show_available_networks(&mut self) -> Result<(), WLANError> {
        let mut networks: Vec<String>;
        let mut state: ScanState;
        let tt = ticktimer_server::Ticktimer::new().unwrap();
        let mut showing_wait = false;
        loop {
            (networks, state) = self.scan_networks()?;
            // note to self: if we wanted to "scan more", probably this is the place
            // where we would want to insert multiple calls to scan_networks() and
            // accumulate the results.
            match state {
                ScanState::Updating => {
                    let mut progress = t!("wlan.ssid_scanning", locales::LANG).to_string();
                    let networks: Vec<&str> = networks.iter().map(|s| s.as_str()).collect();
                    progress.push_str("\n\n");
                    for network in networks {
                        progress.push_str(&format!("\t{}\n", network));
                    }

                    if !showing_wait {
                        self.modals.dynamic_notification(Some(&progress), None).unwrap();
                        showing_wait = true;
                    } else {
                        self.modals.dynamic_notification_update(Some(&progress), None).unwrap();
                    };
                    tt.sleep_ms(1000).ok();
                },
                ScanState::Idle => break,
                ScanState::Off => {
                    if showing_wait {
                        self.modals.dynamic_notification_close().unwrap();
                    }
                    self.modals.show_notification(t!("wlan.ssid_off_error", locales::LANG), None).unwrap();
                    return Ok(());
                }
            }
        }
        if showing_wait {
            self.modals.dynamic_notification_close().ok();
        }
        let mut networks: Vec<&str> = networks.iter().map(|s| s.as_str()).collect();
        // don't show empty strings
        networks.retain(|&n| n.len() != 0);
        // limit the total number displayed so that the "okay" button does not disappear off the bottom
        let max_entries = match gam::SYSTEM_STYLE {
            graphics_server::GlyphStyle::Tall => 13,
            graphics_server::GlyphStyle::Regular => 16,
            _ => 12,
        };
        networks.truncate(max_entries);

        if networks.is_empty() {
            self.modals
                .show_notification(t!("wlan.no_networks", locales::LANG), None)
                .unwrap();
            return Ok(());
        }

        networks.push(t!("wlan.cancel", locales::LANG));

        self.modals.add_list(networks).unwrap();

        let ssid = self
            .modals
            .get_radiobutton(t!("wlan.ssid_choose", locales::LANG))
            .unwrap();

        if ssid == t!("wlan.cancel", locales::LANG) {
            return Ok(());
        }

        self.fill_password_for_ssid(&ssid)
    }

    fn fill_password_for_ssid(&mut self, ssid: &str) -> Result<(), WLANError> {
        let connection_data = self
            .modals
            .alert_builder(&t!("wlan.ssid_password", locales::LANG).replace("{ssid}", ssid))
            .field(
                Some(t!("wlan.password", locales::LANG).to_string()),
                None,
            )
            .build()
            .unwrap();

        let content = connection_data.content();

        self.store_connection_info(ssid, content[0].as_str())
    }

    fn network_status(&mut self) -> Result<(), WLANError> {
        let status = self.com.wlan_status()?;
        let ssid = match status.ssid {
            Some(s) => s.name.to_string(),
            None => t!("stats.disconnected", locales::LANG).to_string(),
        };

        let ls = status.link_state;
        let ip = &status.ipv4;

        // TODO: make a proper translation for this. But, I think for now, this is a fairly
        // technical screen that we can leave in English.
        let status_str = format!(
            "Connection status: \n\n ▪ SSID: {}\n ▪ Link state: {:?}\n ▪ IP: {}\n ▪ Gateway: {}\n ▪ Subnet mask: {}\n ▪ DNS 1: {}\n ▪ DNS 2: {}\n ▪ DHCP state: {:?}\n ▪ MAC: {:x?}",
            ssid,
            ls,
            format_ip(ip.addr),
            format_ip(ip.gtwy),
            format_ip(ip.mask),
            format_ip(ip.dns1),
            format_ip(ip.dns2),
            ip.dhcp,
            ip.mac
        );

        self.modals.show_notification(&status_str, None).unwrap();
        Ok(())
    }

    fn known_networks(&self) -> Result<(), WLANError> {
        let networks = match self.pddb.list_keys(net::AP_DICT_NAME, None) {
            Ok(list) => list,
            Err(_) => Vec::new(),
        };

        let mut networks_string = String::from(t!("wlan.no_known_networks", locales::LANG));

        if networks.is_empty() {
            self.modals
                .show_notification(&networks_string, None)
                .unwrap();
            return Ok(());
        }

        networks_string = String::from(t!("wlan.known_networks", locales::LANG));

        networks_string += &networks
            .iter()
            .map(|s| format!(" ▪ {}", s))
            .collect::<Vec<String>>()
            .join("\n");

        self.modals
            .show_notification(&networks_string, None)
            .unwrap();

        Ok(())
    }

    fn delete_network(&mut self) -> Result<(), WLANError> {
        let networks = match self.pddb.list_keys(net::AP_DICT_NAME, None) {
            Ok(list) => list,
            Err(_) => Vec::new(),
        };

        if networks.is_empty() {
            self.modals
                .show_notification(t!("wlan.no_known_networks", locales::LANG), None)
                .unwrap();
            return Ok(());
        }

        let cancel_item = t!("wlan.cancel", locales::LANG);
        self.modals
            .add_list(networks.iter().map(|s| s.as_str()).collect())
            .unwrap();
        self.modals.add_list_item(cancel_item).unwrap();

        let ssid_to_be_deleted = self
            .modals
            .get_radiobutton(t!("wlan.choose_delete", locales::LANG))
            .unwrap();

        if ssid_to_be_deleted.eq(cancel_item) {
            return Ok(());
        }

        self.pddb
            .delete_key(net::AP_DICT_NAME, &ssid_to_be_deleted, None)
            .map_err(|e| WLANError::PDDBIoError(e))?;

        self.pddb.sync().map_err(|e| WLANError::PDDBIoError(e))
    }

    fn consume_menu_action(&mut self, action: WlanManOp) {
        let resp = match action {
            WlanManOp::AddNetworkManually => self.add_new_ssid(),
            WlanManOp::ScanForNetworks => self.show_available_networks(),
            WlanManOp::Status => self.network_status(),
            WlanManOp::DeleteNetwork => self.delete_network(),
            WlanManOp::KnownNetworks => self.known_networks(),
        };

        resp.unwrap_or_else(|error| self.show_error_modal(error));
    }

    fn show_error_modal(&self, e: WLANError) {
        self.modals
            .show_notification(
                format!("{}: {}", t!("wlan.error", locales::LANG), e).as_str(),
                None,
            )
            .unwrap()
    }
}

fn format_ip(src: [u8; 4]) -> String {
    src.iter()
        .map(|&id| id.to_string())
        .collect::<Vec<String>>()
        .join(".")
}
