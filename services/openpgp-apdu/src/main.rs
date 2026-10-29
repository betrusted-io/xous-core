//! Minimal OpenPGP CCID APDU test harness for dabao-ccid.

#[cfg(not(target_os = "xous"))]
fn main() {
    eprintln!("openpgp-apdu is a Xous process; build with --target riscv32imac-unknown-xous-elf");
}

#[cfg(target_os = "xous")]
use openpgp_apdu::apdu::{ApduError, CommandApdu, ResponseApdu, StatusWord, dispatch_apdu};
#[cfg(target_os = "xous")]
use openpgp_apdu::ccid::{
    CcidError, CcidStatus, PcToRdr, parse_pc_to_rdr, rdr_to_pc_data_block, rdr_to_pc_slot_status,
};
#[cfg(target_os = "xous")]
use openpgp_apdu::openpgp::{CardState, FIXTURE_V1_TEST};
#[cfg(target_os = "xous")]
use openpgp_apdu::usb_link::{CcidLink, UsbDeviceState};

#[cfg(target_os = "xous")]
fn main() -> ! { ccid_main(); }

#[cfg(target_os = "xous")]
fn ccid_main() -> ! {
    // Initialize logging, but don't panic if it fails.
    if log_server::init_wait().is_err() {
        // Log-server isn't available; continue anyway (logging will be unavailable).
        // This shouldn't happen in a normal build, but handle it gracefully.
    } else {
        log::set_max_level(log::LevelFilter::Info);
        log::info!("openpgp-apdu starting (PID {})", xous::process::id());
    }

    // Connect to USB driver, with retry on failure.
    let link = loop {
        match CcidLink::connect_to_usb_driver() {
            Ok(l) => break l,
            Err(e) => {
                log::warn!("USB driver connect failed: {:?}, retrying", e);
                xous::yield_slice();
                // Retry indefinitely (log-safe loop; USB driver is essential).
            }
        }
    };
    let mut card = CardState::new(&FIXTURE_V1_TEST);
    let mut last_link = link.link_status();
    let tt = ticktimer::Ticktimer::new().ok();

    // Do not park CcidRxDeferred until the composite has enumerated. A deferred
    // waiter plus bulk-OUT prime during SET_ADDRESS leaves the host at -110/-71.
    loop {
        let st = link.link_status();
        if st != last_link {
            log::info!("USB link status: {st:?}");
            last_link = st;
        }
        if st == UsbDeviceState::Configured {
            break;
        }
        if let Some(ref tt) = tt {
            tt.sleep_ms(20).ok();
        } else {
            xous::yield_slice();
        }
    }

    loop {
        let st = link.link_status();
        if st != last_link {
            log::info!("USB link status: {st:?}");
            last_link = st;
        }

        let frame = match link.receive_rx() {
            Ok(f) => f,
            Err(xous::Error::ProcessTerminated) => {
                log::info!("CcidRxDeferred hangup (USB reset); re-park when Configured");
                card.applet_selected = false;
                card.clear_chunk_state();
                // Brief settle for usb-bao1x reset bookkeeping.
                if let Some(ref tt) = tt {
                    tt.sleep_ms(50).ok();
                } else {
                    xous::yield_slice();
                }
                // pcsc bus resets often keep LinkStatus at Configured — re-park
                // immediately then. Only spin when status actually dropped.
                let mut waits: u32 = 0;
                loop {
                    let st = link.link_status();
                    if st != last_link {
                        log::info!("USB link status: {st:?}");
                        last_link = st;
                    }
                    if st == UsbDeviceState::Configured {
                        break;
                    }
                    waits = waits.saturating_add(1);
                    if waits > 250 {
                        log::warn!("Configured wait timeout after hangup (st={st:?}); re-parking");
                        break;
                    }
                    if let Some(ref tt) = tt {
                        tt.sleep_ms(20).ok();
                    } else {
                        xous::yield_slice();
                    }
                }
                continue;
            }
            Err(e) => {
                log::warn!("CcidRxDeferred error: {e:?}");
                // USB server panic / IPC failure: do not spin the UART.
                if let Some(ref tt) = tt {
                    tt.sleep_ms(50).ok();
                } else {
                    xous::yield_slice();
                }
                continue;
            }
        };

        let tx_frame = match parse_pc_to_rdr(&frame) {
            Ok(PcToRdr::IccPowerOn { slot, seq, .. })
            | Ok(PcToRdr::GetSlotStatus { slot, seq })
            | Ok(PcToRdr::IccPowerOff { slot, seq }) => {
                // Answered inline in usb-bao1x; skip CcidTx.
                log::debug!("inline CCID slot={slot} seq={seq}: usb-bao1x answers; skip CcidTx");
                continue;
            }
            Ok(PcToRdr::Abort { slot, seq }) => rdr_to_pc_slot_status(slot, seq, CcidStatus::ok_active()),
            Ok(PcToRdr::XfrBlock { slot, seq, apdu }) => {
                log::debug!("XfrBlock slot={slot} seq={seq} apdu_len={}", apdu.len());
                let resp = match CommandApdu::parse(&apdu) {
                    Ok(cmd) => dispatch_apdu(cmd, &mut card),
                    Err(ApduError::Empty | ApduError::TooShort | ApduError::InconsistentLengths) => {
                        ResponseApdu::error(StatusWord::WrongLength)
                    }
                };
                let apdu_bytes = resp.to_bytes();
                log::info!("APDU response len={} sw={:02x}{:02x}", apdu_bytes.len(), resp.sw1, resp.sw2);
                rdr_to_pc_data_block(slot, seq, CcidStatus::ok_active(), &apdu_bytes)
            }
            Err(
                CcidError::LengthMismatch
                | CcidError::TooShort
                | CcidError::UnknownMessageType
                | CcidError::PayloadTooLarge,
            ) => {
                let slot = frame.get(5).copied().unwrap_or(0);
                let seq = frame.get(6).copied().unwrap_or(0);
                log::warn!("malformed CCID frame");
                rdr_to_pc_slot_status(slot, seq, CcidStatus::cmd_not_supported())
            }
        };

        if let Err(e) = link.send_tx(tx_frame) {
            log::warn!("CcidTx failed: {e:?}");
        }
    }
}
