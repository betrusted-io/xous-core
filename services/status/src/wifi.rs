use core::fmt::Display;

use num_traits::*;

use std::io::Write;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, PartialOrd)]
enum WlanManOp {
    TurnWlanOn = 3,
    TurnWlanOff,
    AddNetworkManually,
    ScanForNetworks,
    Status,
    KnownNetworks = 8,
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

struct WLANMan {
    com: com::Com,
    netmgr: net::NetManager,
    modals: modals::Modals,
    pddb: pddb::Pddb,
}

impl WLANMan {
    fn new(xns: &xous_names::XousNames) -> Self {
        Self {
            com: com::Com::new(&xns).unwrap(),
            netmgr: net::NetManager::new(),
            modals: modals::Modals::new(&xns).unwrap(),
            pddb: pddb::Pddb::new(),
        }
    }

    fn actions(&self) -> Vec<(&str, WlanManOp)> {
        use WlanManOp::*;

        vec![
            ("Turn WLAN on", TurnWlanOn),
            ("Turn WLAN off", TurnWlanOff),
            ("Manually add a network", AddNetworkManually),
            ("Scan for networks", ScanForNetworks),
            ("Network status", Status),
            ("List known networks", KnownNetworks),
        ]
    }

    fn set_wlan_state(&mut self, state: bool) -> Result<(), WLANError> {
        match state {
            true => {
                // on
                self.com.wlan_set_on()?;
                self.netmgr.connection_manager_run()?;
            }
            false => {
                // off
                self.netmgr.connection_manager_stop()?;
                self.com.wlan_set_off()?;
            }
        };

        Ok(())
    }

    fn add_new_ssid(&mut self) -> Result<(), WLANError> {
        let connection_data = self
            .modals
            .alert_builder("Fill this form with your connection SSID and password.")
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
                Some("Password".to_string()),
                Some(|text| {
                    if text.as_str().is_empty() {
                        return Some(xous_ipc::String::from_str("Password cannot be empty"));
                    }

                    None
                }),
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
            None,
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

    fn scan_networks(&self) -> Result<Vec<String>, WLANError> {
        let scan_result = self.netmgr.wifi_get_ssid_list()?;

        Ok(scan_result
            .iter()
            .map(|ssid| ssid.name.to_string())
            .collect())
    }

    fn show_available_networks(&mut self) -> Result<(), WLANError> {
        let networks = self.scan_networks()?;
        let networks: Vec<&str> = networks.iter().map(|s| s.as_str()).collect();

        if networks.is_empty() {
            self.modals
                .show_notification("No networks available.", None)
                .unwrap();
            return Ok(());
        }

        self.modals.add_list(networks).unwrap();

        let ssid = self.modals.get_radiobutton("Choose an action:").unwrap();

        self.fill_password_for_ssid(&ssid)
    }

    fn fill_password_for_ssid(&mut self, ssid: &str) -> Result<(), WLANError> {
        let connection_data = self
            .modals
            .alert_builder(&format!("Fill this form with the password for {ssid}."))
            .field(
                Some("Password".to_string()),
                Some(|text| {
                    if text.as_str().is_empty() {
                        return Some(xous_ipc::String::from_str("Password cannot be empty"));
                    }

                    None
                }),
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
            None => "not connected".to_string(),
        };

        let ls = status.link_state;
        let ip = &status.ipv4;

        let status_str = format!(
            "Connection status: \n\n - SSID: {}\n - Link state: {:?}\n - IP: {}\n - Gateway: {}\n - Subnet mask: {}\n - DNS 1: {}\n - DNS 2: {}\n - DHCP state: {:?}",
            ssid,
            ls,
            format_ip(ip.addr),
            format_ip(ip.gtwy),
            format_ip(ip.mask),
            format_ip(ip.dns1),
            format_ip(ip.dns2),
            ip.dhcp,
        );

        self.modals.show_notification(&status_str, None).unwrap();
        Ok(())
    }

    fn known_networks(&self) -> Result<(), WLANError> {
        let networks = self.pddb.list_keys(net::AP_DICT_NAME, None)?;

        let mut networks_string = String::from("No known networks.");

        if networks.is_empty() {
            self.modals
                .show_notification(&networks_string, None)
                .unwrap();
            return Ok(());
        }

        networks_string = String::from("Known networks:\n");

        networks_string += &networks
            .iter()
            .map(|s| format!(" - {}", s))
            .collect::<Vec<String>>()
            .join("\n");

        self.modals
            .show_notification(&networks_string, None)
            .unwrap();

        Ok(())
    }

    fn claim_menumatic_menu(&self, cid: xous::CID) {
        let mut menus = self
            .actions()
            .iter()
            .map(|action| gam::MenuItem {
                name: xous_ipc::String::from_str(action.0),
                action_conn: Some(cid),
                action_opcode: action.1.to_u32().unwrap(),
                action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
                close_on_select: true,
            })
            .collect::<Vec<gam::MenuItem>>();

        menus.push(gam::MenuItem {
            name: xous_ipc::String::from_str("Close menu"),
            action_conn: None,
            action_opcode: 0,
            action_payload: gam::MenuPayload::Scalar([0, 0, 0, 0]),
            close_on_select: true,
        });

        gam::menu_matic(menus, gam::WIFI_MENU_NAME, None);
    }

    fn consume_menu_action(&mut self, action: WlanManOp) {
        let resp = match action {
            WlanManOp::TurnWlanOff => self.set_wlan_state(false),
            WlanManOp::TurnWlanOn => self.set_wlan_state(true),
            WlanManOp::AddNetworkManually => self.add_new_ssid(),
            WlanManOp::ScanForNetworks => self.show_available_networks(),
            WlanManOp::Status => self.network_status(),
            WlanManOp::KnownNetworks => self.known_networks(),
        };

        resp.unwrap_or_else(|error| self.show_error_modal(error));
    }

    fn show_error_modal(&self, e: WLANError) {
        self.modals
            .show_notification(format!("Error: {}", e).as_str(), None)
            .unwrap()
    }
}

fn format_ip(src: [u8; 4]) -> String {
    src.iter()
        .map(|&id| id.to_string())
        .collect::<Vec<String>>()
        .join(".")
}

pub fn start_background_thread() {
    std::thread::spawn(|| run_menu_thread());
}

fn run_menu_thread() {
    let xns = xous_names::XousNames::new().unwrap();

    let sid = xous::create_server().unwrap();

    let mut hello = WLANMan::new(&xns);

    let menu_conn = xous::connect(sid).unwrap();
    hello.claim_menumatic_menu(menu_conn);

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        match FromPrimitive::from_usize(msg.body.id()) {
            Some(other) => {
                if other >= WlanManOp::TurnWlanOn && other <= WlanManOp::KnownNetworks {
                    hello.consume_menu_action(other);
                    continue;
                }
            }

            _ => {
                log::error!("Got unknown message");
            }
        }
    }
}