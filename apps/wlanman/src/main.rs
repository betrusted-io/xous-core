#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use core::fmt::{Display, Write};
use gam::modal;
use graphics_server::api::GlyphStyle;
use graphics_server::{DrawStyle, Gid, PixelColor, Point, Rectangle, TextBounds, TextView};
use locales::t;
use num_traits::*;
use std::collections::HashMap;
use std::io::Write as IoWrite;

pub(crate) const SERVER_NAME_WLANMAN: &str = "_WLAN manager_";

/// Top level application events.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum HelloOp {
    /// Redraw the screen
    Redraw = 0,

    Char,
    Quit,
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
    content: Gid,
    gam: gam::Gam,
    _gam_token: [u32; 4],
    screensize: Point,

    // Deps for wlan management
    com: com::Com,
    netmgr: net::NetManager,
    modals: modals::Modals,
    pddb: pddb::Pddb,
}

impl WLANMan {
    fn new(xns: &xous_names::XousNames, sid: xous::SID) -> Self {
        let gam = gam::Gam::new(&xns).expect("Can't connect to GAM");
        let gam_token = gam
            .register_ux(gam::UxRegistration {
                app_name: xous_ipc::String::<128>::from_str(gam::APP_NAME_WLANMAN),
                ux_type: gam::UxType::Framebuffer,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: HelloOp::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                rawkeys_id: Some(HelloOp::Char.to_u32().unwrap()),
                focuschange_id: None,
            })
            .expect("Could not register GAM UX")
            .unwrap();

        let content = gam
            .request_content_canvas(gam_token)
            .expect("Could not get content canvas");
        let screensize = gam
            .get_canvas_bounds(content)
            .expect("Could not get canvas dimensions");
        Self {
            gam,
            _gam_token: gam_token,
            content,
            screensize,

            com: com::Com::new(&xns).unwrap(),
            netmgr: net::NetManager::new(),
            modals: modals::Modals::new(&xns).unwrap(),
            pddb: pddb::Pddb::new(),
        }
    }

    fn root_actions(&self) -> Vec<&str> {
        vec![
            "Turn WLAN on",
            "Turn WLAN off",
            "Manually add a network",
            "Scan for networks",
            "Network status",
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
            "Connection status: \n\n- SSID: {}\n- Link state: {:?}\n- IP: {}\n- Gateway: {}\n- Subnet mask: {}\n- DNS 1: {}\n- DNS 2: {}\n- DHCP state: {:?}",
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

    fn draw_menu(&self) -> usize {
        let actions = self.root_actions();

        self.modals.add_list(actions).unwrap();
        self.modals.get_radiobutton("Choose an action:").unwrap();

        self.modals.get_radio_index().unwrap()
    }

    fn consume_menu_index(&mut self, idx: usize) {
        let resp = match idx {
            0 => self.set_wlan_state(true),
            1 => self.set_wlan_state(false),
            2 => self.add_new_ssid(),
            3 => self.show_available_networks(),
            4 => self.network_status(),
            _ => panic!("idx {idx} not covered"),
        };

        resp.unwrap_or_else(|error| self.show_error_modal(error));
    }

    fn show_error_modal(&self, e: WLANError) {
        self.modals
            .show_notification(format!("Error: {}", e).as_str(), None)
            .unwrap()
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Hello world PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();

    // Register the server with xous
    let sid = xns
        .register_name(SERVER_NAME_WLANMAN, None)
        .expect("can't register server");

    let mut hello = WLANMan::new(&xns, sid);

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        match FromPrimitive::from_usize(msg.body.id()) {
            Some(HelloOp::Redraw) => {
                log::debug!("Got redraw");
            }
            Some(HelloOp::Quit) => {
                log::info!("Quitting application");
                break;
            }
            Some(HelloOp::Char) => {
                let idx = hello.draw_menu();
                hello.consume_menu_index(idx);
            }
            _ => {
                log::error!("Got unknown message");
            }
        }
    }

    log::info!("Quitting");
    xous::terminate_process(0)
}

fn format_ip(src: [u8; 4]) -> String {
    src.iter()
        .map(|&id| id.to_string())
        .collect::<Vec<String>>()
        .join(".")
}
