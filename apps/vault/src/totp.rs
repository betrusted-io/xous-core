use std::sync::{Arc, Mutex};
use std::thread;
use std::{
    convert::TryFrom,
    time::{SystemTime, SystemTimeError},
};

use hmac::{Hmac, Mac};
use num_traits::*;
use sha1::Sha1;
use xous::{Message, send_message};

use crate::VaultMode;

// Derived from https://github.com/blakesmith/xous-core/blob/xtotp-time/apps/xtotp/src/main.rs
#[derive(Clone, Copy)]
pub enum TotpAlgorithm {
    HmacSha1,
    HmacSha256,
    HmacSha512,
    None,
}

impl Default for TotpAlgorithm {
    fn default() -> Self { Self::None }
}

impl std::fmt::Debug for TotpAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TotpAlgorithm::HmacSha1 => write!(f, "SHA1"),
            TotpAlgorithm::HmacSha256 => write!(f, "SHA256"),
            TotpAlgorithm::HmacSha512 => write!(f, "SHA512"),
            TotpAlgorithm::None => write!(f, "None"),
        }
    }
}

impl TryFrom<&str> for TotpAlgorithm {
    type Error = xous::Error;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "SHA1" => Ok(TotpAlgorithm::HmacSha1),
            "SHA256" => Ok(TotpAlgorithm::HmacSha256),
            "SHA512" => Ok(TotpAlgorithm::HmacSha512),
            _ => Err(xous::Error::InvalidString),
        }
    }
}
impl core::fmt::Display for TotpAlgorithm {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            TotpAlgorithm::HmacSha1 => write!(f, "SHA1"),
            TotpAlgorithm::HmacSha256 => write!(f, "SHA256"),
            TotpAlgorithm::HmacSha512 => write!(f, "SHA512"),
            TotpAlgorithm::None => write!(f, "None"),
        }
    }
}

#[derive(Debug)]
pub struct TotpEntry {
    pub step_seconds: u64,
    pub shared_secret: Vec<u8>,
    pub digit_count: u8,
    pub algorithm: TotpAlgorithm,
}

pub fn get_current_unix_time() -> Result<u64, SystemTimeError> {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|duration| duration.as_secs())
}

fn unpack_u64(v: u64) -> [u8; 8] {
    let mask = 0x00000000000000ff;
    let mut bytes: [u8; 8] = [0; 8];
    (0..8).for_each(|i| bytes[7 - i] = (mask & (v >> (i * 8))) as u8);
    bytes
}

fn generate_hmac_bytes(unix_timestamp: u64, totp_entry: &TotpEntry) -> Result<Vec<u8>, xous::Error> {
    let mut computed_hmac = Vec::new();
    let checked_step = if totp_entry.step_seconds == 0 {
        log::warn!(
            "totp step_seconds was 0, this would cause a div-by-zero; forcing to 1. Check that this is not an HOTP record?"
        );
        1
    } else {
        totp_entry.step_seconds
    };
    match totp_entry.algorithm {
        // The OpenTitan HMAC core does not support hmac-sha1. Fall back to
        // a software implementation.
        TotpAlgorithm::HmacSha1 => {
            let mut mac: Hmac<Sha1> =
                Hmac::new_from_slice(&totp_entry.shared_secret).map_err(|_| xous::Error::InternalError)?;
            mac.update(&unpack_u64(unix_timestamp / checked_step));
            let hash: &[u8] = &mac.finalize().into_bytes();
            computed_hmac.extend_from_slice(hash);
        }
        // note: sha256/sha512 implementations not yet tested, as we have yet to find a site that uses this to
        // test against.
        TotpAlgorithm::HmacSha256 => {
            let mut mac: Hmac<sha2::Sha256> =
                Hmac::new_from_slice(&totp_entry.shared_secret).map_err(|_| xous::Error::InternalError)?;
            mac.update(&unpack_u64(unix_timestamp / checked_step));
            let hash: &[u8] = &mac.finalize().into_bytes();
            computed_hmac.extend_from_slice(hash);
        }
        TotpAlgorithm::HmacSha512 => {
            let mut mac: Hmac<sha2::Sha512> =
                Hmac::new_from_slice(&totp_entry.shared_secret).map_err(|_| xous::Error::InternalError)?;
            mac.update(&unpack_u64(unix_timestamp / checked_step));
            let hash: &[u8] = &mac.finalize().into_bytes();
            computed_hmac.extend_from_slice(hash);
        }
        TotpAlgorithm::None => {
            panic!("cannot generate hmac bytes for None algorithm")
        }
    }

    Ok(computed_hmac)
}

pub fn generate_totp_code(unix_timestamp: u64, totp_entry: &TotpEntry) -> Result<String, xous::Error> {
    let hash = generate_hmac_bytes(unix_timestamp, totp_entry)?;
    let offset: usize = (hash.last().unwrap_or(&0) & 0xf) as usize;
    let binary: u64 = (((hash[offset] & 0x7f) as u64) << 24)
        | ((hash[offset + 1] as u64) << 16)
        | ((hash[offset + 2] as u64) << 8)
        | (hash[offset + 3] as u64);

    let truncated_code = format!(
        "{:01$}",
        binary % (10_u64.pow(totp_entry.digit_count as u32)),
        totp_entry.digit_count as usize
    );

    Ok(truncated_code)
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum PumpOp {
    Pump,
    Quit,
}

pub(crate) fn pumper(
    mode: Arc<Mutex<VaultMode>>,
    sid: xous::SID,
    main_conn: xous::CID,
    allow_totp_rendering: Arc<core::sync::atomic::AtomicBool>,
) {
    let _ = thread::spawn({
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let self_conn = xous::connect(sid).unwrap();
            loop {
                let msg = xous::receive_message(sid).unwrap();
                let opcode: Option<PumpOp> = FromPrimitive::from_usize(msg.body.id());
                log::trace!("{:?}", opcode);
                match opcode {
                    Some(PumpOp::Pump) => {
                        if allow_totp_rendering.load(core::sync::atomic::Ordering::SeqCst) {
                            // don't redraw if we're in host access mode
                            xous::try_send_message(
                                main_conn,
                                Message::new_scalar(crate::VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0),
                            )
                            .ok(); // don't panic if the queue overflows
                        }
                        let mode_cache = { (*mode.lock().unwrap()).clone() };
                        {
                            // we really want mode.lock() to be in a different scope so...
                            if mode_cache == VaultMode::Totp {
                                tt.sleep_ms(2000).unwrap();
                                send_message(
                                    self_conn,
                                    Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                                )
                                .expect("couldn't restart pump");
                            }
                        }
                        // if not in Totp mode, the restart message doesn't go through, and the redraws
                        // automatically stop.
                    }
                    Some(PumpOp::Quit) => {
                        break;
                    }
                    _ => log::warn!("couldn't parse message: {:?}", msg),
                }
            }
            xous::destroy_server(sid).ok();
        }
    });
}
