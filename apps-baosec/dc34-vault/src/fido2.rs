use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Instant;

use dc34_vault::Transport;
use dc34_vault::ctap::main_hid::HidIterType;
use dc34_vault::env::Env;
use dc34_vault::env::xous::XousEnv;
use xous_usb_hid::device::fido::*;

use crate::vendor_commands;
use crate::vendor_commands::VendorSession;

pub(crate) fn fido2_handler(
    conn: xous::CID,
    allow_host: Arc<AtomicBool>,
    opensk_mutex: Arc<Mutex<i32>>,
    animate: Arc<AtomicBool>,
) {
    // spawn the FIDO2 USB handler
    let _ = thread::spawn({
        move || {
            let mut vendor_session = VendorSession::default();
            // block until the PDDB is mounted
            let pddb = pddb::Pddb::new();
            pddb.is_mounted_blocking();

            let env = XousEnv::new(conn, animate);
            let mut ctap = dc34_vault::Ctap::new(env, Instant::now());
            loop {
                match ctap.env().main_hid_connection().u2f_wait_incoming() {
                    Ok(msg) => {
                        ctap.update_timeouts(Instant::now());
                        let mutex = opensk_mutex.lock().unwrap();
                        log::trace!("Received U2F packet");
                        let typed_reply =
                            ctap.process_hid_packet(&msg.packet, Transport::MainHid, Instant::now());
                        match typed_reply {
                            HidIterType::Ctap(reply) => {
                                for pkt_reply in reply {
                                    let mut reply = RawFidoReport::default();
                                    reply.packet.copy_from_slice(&pkt_reply);
                                    let status = ctap.env().main_hid_connection().u2f_send(reply);
                                    // This sleep is necessary because Baochip's high speed bus can overwhelm
                                    // a receiver. Most hosts envision a slow, full-speed device, and this is
                                    // a fast, high-speed device. Without this sleep, you end up with missed
                                    // sequence numbers on the receiver side if the host can't drain the
                                    // packets fast enough!
                                    std::thread::sleep(std::time::Duration::from_millis(2));
                                    match status {
                                        Ok(()) => {
                                            log::trace!("Sent U2F packet");
                                        }
                                        Err(e) => {
                                            log::error!("Error sending U2F packet: {:?}", e);
                                        }
                                    }
                                }
                            }
                            HidIterType::Vendor(msg) => {
                                let reply = match vendor_commands::handle_vendor_data(
                                    msg.cmd as u8,
                                    msg.cid,
                                    msg.payload,
                                    &mut vendor_session,
                                ) {
                                    Ok(return_payload) => {
                                        // if None, this means we've finished parsing all that
                                        // was needed, and we handle/respond with real data

                                        match return_payload {
                                            Some(data) => data,
                                            None => {
                                                log::debug!("starting processing of vendor data...");
                                                let resp = vendor_commands::handle_vendor_command(
                                                    &mut vendor_session,
                                                    allow_host.load(Ordering::SeqCst),
                                                );
                                                log::debug!("finished processing of vendor data!");

                                                match vendor_session.is_backup() {
                                                    true => {
                                                        if vendor_session.has_backup_data() {
                                                            resp
                                                        } else {
                                                            vendor_session = VendorSession::default();
                                                            resp
                                                        }
                                                    }
                                                    false => {
                                                        vendor_session = VendorSession::default();
                                                        resp
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(session_error) => {
                                        // reset the session
                                        vendor_session = VendorSession::default();

                                        session_error.ctaphid_error(msg.cid)
                                    }
                                };
                                for pkt_reply in reply {
                                    let mut reply = RawFidoReport::default();
                                    reply.packet.copy_from_slice(&pkt_reply);
                                    let status = ctap.env().main_hid_connection().u2f_send(reply);
                                    // if Vendor code also complains of incorrect sequence numbers malformed
                                    // data or inconsistent timeouts, try uncommenting the below. It could
                                    // also be because the host is silently dropping HID packets from the
                                    // vendor interface!
                                    //
                                    // std::thread::sleep(std::time::Duration::from_millis(1));
                                    match status {
                                        Ok(()) => {
                                            log::trace!("Sent U2F packet");
                                        }
                                        Err(e) => {
                                            log::error!("Error sending U2F packet: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }
                        drop(mutex);
                    }
                    Err(e) => match e {
                        _ => {
                            log::warn!("FIDO listener got an error: {:?}", e);
                        }
                    },
                }
            }
        }
    });
}
