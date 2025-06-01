use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use num_traits::*;
use totp::PumpOp;
mod ux;
use ux::*;
mod itemcache;
use itemcache::*;
mod totp;

pub(crate) const SERVER_NAME_VAULT2: &str = "_Vault2_";

/// Top level application events.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum VaultOp {
    /// Redraw the screen
    Redraw = 0,
    KeyPress,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum VaultMode {
    Totp,
    Password,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct SelectedEntry {
    pub key_guid: String,
    pub description: String,
    pub mode: VaultMode,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Vault2 PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();

    // Register the server with xous
    let sid = xns.register_name(SERVER_NAME_VAULT2, None).expect("can't register server");
    let conn = xous::connect(sid).unwrap();

    // global shared state
    let mode = Arc::new(Mutex::new(VaultMode::Totp));
    let allow_totp_rendering = Arc::new(AtomicBool::new(true));
    let item_lists = Arc::new(Mutex::new(ItemLists::new()));

    let mut vault_ui = VaultUi::new(&xns, conn, item_lists.clone());

    // spawn the TOTP pumper
    let pump_sid = xous::create_server().unwrap();
    crate::totp::pumper(mode.clone(), pump_sid, conn, allow_totp_rendering.clone());
    let pump_conn = xous::connect(pump_sid).unwrap();

    // respond to keyboard events
    let kbd = cramium_api::keyboard::Keyboard::new(&xns).unwrap();
    kbd.register_listener(SERVER_NAME_VAULT2, VaultOp::KeyPress.to_u32().unwrap() as usize);

    // kickstart the pumper
    xous::send_message(pump_conn, xous::Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0))
        .expect("couldn't start the pumper");
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("Got message: {:?}", msg);

        match FromPrimitive::from_usize(msg.body.id()) {
            Some(VaultOp::Redraw) => {
                log::debug!("Got redraw");
                match *mode.lock().unwrap() {
                    VaultMode::Totp => {
                        vault_ui.redraw_totp();
                    }
                    _ => {
                        unimplemented!()
                    }
                }
            }
            Some(VaultOp::KeyPress) => xous::msg_scalar_unpack!(msg, k1, _k2, _k3, _k4, {
                let k = char::from_u32(k1 as u32).unwrap_or('\u{0000}');
                log::info!("got key {}", k);
            }),
            _ => {
                log::error!("Got unknown message");
            }
        }
    }
}
