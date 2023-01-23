pub use self::storage::XousStorage;
use crate::api::attestation_store::AttestationStore;
use crate::api::connection::{HidConnection, SendOrRecvError, SendOrRecvResult, SendOrRecvStatus};
use crate::api::customization::{CustomizationImpl, DEFAULT_CUSTOMIZATION};
use crate::api::firmware_protection::FirmwareProtection;
use crate::api::user_presence::{UserPresence, UserPresenceError, UserPresenceResult};
use crate::api::{attestation_store, key_store};
use crate::KEEPALIVE_DELAY_MS;
use crate::env::Env;
use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use persistent_store::Store;
use ctap_crypto::rng256::XousRng256;
use xous_names::XousNames;
use crate::env::xous::storage::XousUpgradeStorage;
use usbd_human_interface_device::device::fido::*;
use pddb::Pddb;
use modals::Modals;
use locales::t;
use std::io::{Read, Write};
use num_traits::*;

use crate::ctap::hid::{CtapHid, KeepaliveStatus, ProcessedPacket, CtapHidCommand};
use crate::{basis_change, deserialize_app_info, AppInfo, serialize_app_info};

pub const U2F_APP_DICT: &'static str = "fido.u2fapps";
const KEEPALIVE_DELAY: Duration = Duration::from_millis(KEEPALIVE_DELAY_MS);
mod storage;

pub struct XousHidConnection {
    pub endpoint: usb_device_xous::UsbHid,
}
impl XousHidConnection {
    pub fn recv_with_timeout(
        &mut self,
        buf: &mut [u8; 64],
        timeout_delay: Duration,
    ) -> SendOrRecvStatus {
        match self.endpoint.u2f_wait_incoming_timeout(timeout_delay.as_millis() as u64) {
            Ok(msg) => {
                buf.copy_from_slice(&msg.packet);
                SendOrRecvStatus::Received
            }
            Err(_e) => {
                SendOrRecvStatus::Timeout
            }
        }
    }
}

impl HidConnection for XousHidConnection {
    fn send_and_maybe_recv(
        &mut self,
        buf: &mut [u8; 64],
        _timeout: Duration,
    ) -> SendOrRecvResult {
        let mut reply = RawFidoMsg::default();
        reply.packet.copy_from_slice(buf);
        match self.endpoint.u2f_send(reply) {
            Ok(()) => Ok(SendOrRecvStatus::Sent),
            Err(e) => {
                log::error!("FIDO error in sending: {:?}", e);
                Err(SendOrRecvError)
            }
        }
    }
}

pub struct XousEnv {
    rng: XousRng256,
    store: Store<XousStorage>,
    main_connection: XousHidConnection,
    #[cfg(feature = "vendor_hid")]
    vendor_connection: XousHidConnection,
    modals: Modals,
    pddb: Pddb,
    main_cid: xous::CID,
}

impl XousEnv {
    /// Returns the unique instance of the Xous environment.
    /// Blocks until the PDDB is mounted
    pub fn new(conn: xous::CID) -> Self {
        // We rely on `take_storage` to ensure that this function is called only once.
        let storage = XousStorage {};
        let store = Store::new(storage).ok().unwrap();
        let xns = XousNames::new().unwrap();
        XousEnv {
            rng: XousRng256::new(&xns),
            store,
            main_connection: XousHidConnection {
                endpoint: usb_device_xous::UsbHid::new(),
            },
            #[cfg(feature = "vendor_hid")]
            vendor_connection: XousHidConnection {
                endpoint: UsbEndpoint::VendorHid,
            },
            modals: modals::Modals::new(&xns).unwrap(),
            pddb: pddb::Pddb::new(),
            main_cid: conn,
        }
    }
    /// Checks if the SoC is compatible with USB drivers (older versions of Precursor's FPGA don't have the USB device core)
    pub fn is_soc_compatible(&self) -> bool {
        self.main_connection.endpoint.is_soc_compatible()
    }

    fn send_keepalive_up_needed(
        &mut self,
        timeout: Duration,
        cid: [u8; 4]
    ) -> Result<(), UserPresenceError> {
        let keepalive_msg = CtapHid::keepalive(cid, KeepaliveStatus::UpNeeded);
        for mut pkt in keepalive_msg {
            match self.main_connection.send_and_maybe_recv(&mut pkt, timeout) {
                Ok(SendOrRecvStatus::Timeout) => {
                    log::debug!("Sending a KEEPALIVE packet timed out");
                    // TODO: abort user presence test?
                }
                Err(_) => panic!("Error sending KEEPALIVE packet"),
                Ok(SendOrRecvStatus::Sent) => {
                    log::debug!("Sent KEEPALIVE packet");
                }
                Ok(SendOrRecvStatus::Received) => {
                    // We only parse one packet, because we only care about CANCEL.
                    let (received_cid, processed_packet) = CtapHid::process_single_packet(&pkt);
                    if received_cid != cid {
                        log::debug!(
                            "Received a packet on channel ID {:?} while sending a KEEPALIVE packet",
                            received_cid,
                        );
                        return Ok(());
                    }
                    match processed_packet {
                        ProcessedPacket::InitPacket { cmd, .. } => {
                            if cmd == CtapHidCommand::Cancel as u8 {
                                // We ignore the payload, we can't answer with an error code anyway.
                                log::debug!("User presence check cancelled");
                                return Err(UserPresenceError::Canceled);
                            } else {
                                log::debug!(
                                    "Discarded packet with command {} received while sending a KEEPALIVE packet",
                                    cmd,
                                );
                            }
                        }
                        ProcessedPacket::ContinuationPacket { .. } => {
                            log::debug!(
                                "Discarded continuation packet received while sending a KEEPALIVE packet",
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl UserPresence for XousEnv {
    fn check_init(&mut self) {

    }
    /// Implements FIDO behavior (CTAP2 protocol)
    fn wait_with_timeout(&mut self, timeout: Duration, reason: Option::<String>, cid: [u8; 4]) -> UserPresenceResult {
        let reason = reason.unwrap_or(String::new());
        let kbhit = Arc::new(AtomicU32::new(0));
        let expiration = Instant::now().checked_add(timeout).expect("duration bug");
        self.modals.dynamic_notification(
            Some(t!("vault.u2freq", xous::LANG)),
            None,
        ).unwrap();
        // start the keyboard hit listener thread
        let _ = std::thread::spawn({
            let token = self.modals.token().clone();
            let conn = self.modals.conn().clone();
            let kbhit = kbhit.clone();
            move || {
                // note that if no key is hit, we get None back on dialog box close automatically
                match modals::dynamic_notification_blocking_listener(token, conn) {
                    Ok(Some(c)) => {
                        log::trace!("kbhit got {}", c);
                        kbhit.store(c as u32, Ordering::SeqCst)
                    },
                    Ok(None) => {
                        log::trace!("kbhit exited or had no characters");
                        kbhit.store(0, Ordering::SeqCst)
                    },
                    Err(e) => log::error!("error waiting for keyboard hit from blocking listener: {:?}", e),
                }
            }
        });

        let mut last_remaining = u64::MAX;
        loop {
            let mut request_str = String::from(&reason);
            let remaining = expiration.duration_since(Instant::now()).as_secs();
            if last_remaining != remaining {
                // only update the UX once per second
                request_str.push_str(
                    &format!("\n\n⚠   {}{}   ⚠\n",
                    remaining,
                    t!("vault.fido.countdown", xous::LANG)
                ));
                self.modals.dynamic_notification_update(
                    Some(t!("vault.u2freq", xous::LANG)),
                    Some(&request_str),
                ).unwrap();
                last_remaining = remaining;
            }

            // handle exit cases
            if remaining == 0 {
                self.modals.dynamic_notification_close().ok();
                return Err(UserPresenceError::Timeout)
            }
            let key_hit = kbhit.load(Ordering::SeqCst);
            if key_hit != 0 && key_hit != 0x11 { // 0x11 is the F1 key
                self.modals.dynamic_notification_close().ok();
                return Ok(())
            } else if key_hit == 0x11 {
                self.modals.dynamic_notification_close().ok();
                return Err(UserPresenceError::Declined)
            }

            // delay, and keepalive
            self.send_keepalive_up_needed(KEEPALIVE_DELAY, cid)
            .map_err(|e| e.into())?;
            std::thread::sleep(KEEPALIVE_DELAY);
        }
    }

    /// Wait for user approval of a CTAP1-type credential
    /// In this flow, we try to map the `app_id` to a user-provided, memorable string
    /// We also give users the option to abort out of approving.
    fn wait_ctap1(&mut self, reason: String, app_id: [u8; 32]) -> bool {
        let app_id_str = hex::encode(app_id);
        let mut first_time = false;
        let mut info = {
            // fetch the application info, if it exists
            let app_id_str = hex::encode(app_id);
            log::info!("querying U2F record {}", app_id_str);
            // add code to query the PDDB here to look for the k/v mapping of this app ID
            match self.pddb.get(
                U2F_APP_DICT,
                &app_id_str,
                None, true, false,
                Some(256), Some(basis_change)
            ) {
                Ok(mut app_data) => {
                    let app_attr = app_data.attributes().unwrap();
                    if app_attr.len != 0 {
                        let mut descriptor = Vec::<u8>::new();
                        match app_data.read_to_end(&mut descriptor) {
                            Ok(_) => {
                                deserialize_app_info(descriptor)
                            }
                            Err(e) => {log::error!("Couldn't read app info: {:?}", e); None}
                        }
                    } else {
                        None
                    }
                }
                _ => {
                    log::info!("couldn't find key {}", app_id_str);
                    None
                }
            }
        }.unwrap_or_else(
            || {
                // otherwise, create it
            match self.modals
                .alert_builder(t!("vault.u2f.give_app_name", xous::LANG))
                .field(None, None)
                .build()
                {
                    Ok(name) => {
                        first_time = true;
                        let info = AppInfo {
                            name: name.content()[0].content.to_string(),
                            notes: t!("vault.notes", xous::LANG).to_string(),
                            id: app_id,
                            ctime: crate::utc_now().timestamp() as u64,
                            atime: 0,
                            count: 0,
                        };
                        info
                    }
                    _ => {
                        log::error!("couldn't get name for app");
                        panic!("couldn't get name for app");
                    }
                }
            }
        );
        // request approval, if we didn't just create the record
        if !first_time {
            let mut request_str = String::from(&reason);
            // we have some prior record of the app, human-format it
            request_str.push_str(&format!("\n{}{}",
                t!("vault.u2f.appinfo.name", xous::LANG), info.name
            ));
            request_str.push_str(&format!("\n{}",
                crate::atime_to_str(info.atime)
            ));
            request_str.push_str(&format!("\n{}{}",
                t!("vault.u2f.appinfo.authcount", xous::LANG),
                info.count,
            ));

            self.modals.dynamic_notification(
                Some(t!("vault.u2freq", xous::LANG)),
                Some(&request_str),
            ).unwrap();

            // block until the user hits a key
            match modals::dynamic_notification_blocking_listener(self.modals.token(), self.modals.conn()) {
                Ok(Some(c)) => {
                    if c as u32 == 0x11 { // this is the F1 key
                        return false;
                    } else {
                        // any other key shall accept
                    }
                },
                Ok(None) => {
                    log::trace!("kbhit exited or had no characters");
                    return false;
                },
                Err(e) => {
                    log::error!("error waiting for keyboard hit from blocking listener: {:?}", e);
                    return false;
                },
            }
        }

        // note the access
        info.atime = crate::utc_now().timestamp() as u64;
        info.count = info.count.saturating_add(1);
        let ser = serialize_app_info(&info);

        // update the access time, by deleting the key and writing it back into the PDDB
        let basis = match self.pddb.get(
            U2F_APP_DICT,
            &app_id_str,
            None, true, true,
            Some(256), Some(basis_change)
        ) {
            Ok(app_data) => {
                let attr = app_data.attributes().expect("couldn't get attributes");
                attr.basis
            }
            Err(e) => {
                log::error!("error updating app atime: {:?}", e);
                return false;
            }
        };
        self.pddb.delete_key(U2F_APP_DICT, &app_id_str, Some(&basis)).ok();
        match self.pddb.get(
            U2F_APP_DICT,
            &app_id_str,
            Some(&basis), true, true,
            Some(256), Some(basis_change)
        ) {
            Ok(mut app_data) => {
                app_data.write(&ser).expect("couldn't update atime");
            }
            _ => log::error!("Error updating app atime"),
        }
        self.pddb.sync().ok();

        log::info!("sycing UI state...");
        xous::send_message(
            self.main_cid,
            xous::Message::new_scalar(
                crate::VaultOp::ReloadDbAndFullRedraw.to_usize().unwrap(),
                0, 0, 0, 0)
        ).unwrap();
        true
    }

    fn check_complete(&mut self) {

    }
}

impl FirmwareProtection for XousEnv {
    fn lock(&mut self) -> bool {
        false
    }
}

impl key_store::Helper for XousEnv {}

impl AttestationStore for XousEnv {
    fn get(
        &mut self,
        id: &attestation_store::Id,
    ) -> Result<Option<attestation_store::Attestation>, attestation_store::Error> {
        if !matches!(id, attestation_store::Id::Batch) {
            return Err(attestation_store::Error::NoSupport);
        }
        attestation_store::helper_get(self)
    }

    fn set(
        &mut self,
        id: &attestation_store::Id,
        attestation: Option<&attestation_store::Attestation>,
    ) -> Result<(), attestation_store::Error> {
        if !matches!(id, attestation_store::Id::Batch) {
            return Err(attestation_store::Error::NoSupport);
        }
        attestation_store::helper_set(self, attestation)
    }
}

use core::fmt;
pub struct Console {
}
impl Console {
    pub fn new() -> Self {
        Console {  }
    }
}
impl fmt::Write for Console {
    fn write_str(&mut self, string: &str) -> Result<(), fmt::Error> {
        log::info!("{}", string);
        Ok(())
    }
}

impl Env for XousEnv {
    type Rng = XousRng256;
    type UserPresence = Self;
    type Storage = XousStorage;
    type KeyStore = Self;
    type AttestationStore = Self;
    type FirmwareProtection = Self;
    type Write = Console;
    type Customization = CustomizationImpl;
    type HidConnection = XousHidConnection;
    type UpgradeStorage = XousUpgradeStorage;

    fn rng(&mut self) -> &mut Self::Rng {
        &mut self.rng
    }

    fn user_presence(&mut self) -> &mut Self::UserPresence {
        self
    }

    fn store(&mut self) -> &mut Store<Self::Storage> {
        &mut self.store
    }

    fn key_store(&mut self) -> &mut Self {
        self
    }

    fn attestation_store(&mut self) -> &mut Self {
        self
    }

    fn upgrade_storage(&mut self) -> Option<&mut Self::UpgradeStorage> {
        None
    }

    fn firmware_protection(&mut self) -> &mut Self::FirmwareProtection {
        self
    }

    fn write(&mut self) -> Self::Write {
        Console::new()
    }

    fn customization(&self) -> &Self::Customization {
        &DEFAULT_CUSTOMIZATION
    }

    fn main_hid_connection(&mut self) -> &mut Self::HidConnection {
        &mut self.main_connection
    }

    #[cfg(feature = "vendor_hid")]
    fn vendor_hid_connection(&mut self) -> &mut Self::HidConnection {
        &mut self.vendor_connection
    }
}

pub const KEEPALIVE_DELAY_XOUS: Duration = Duration::from_millis(KEEPALIVE_DELAY_MS);

