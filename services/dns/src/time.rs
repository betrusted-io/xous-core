/// The `time_server` is unique is that it is written for exclusive use by `libstd` to extract time.
///
/// It also has a single hook that is callable from the PDDB to initialize a time value once the
/// PDDB itself has been initialized. Because time initialization breaks several abstractions, the
/// system is forced to reboot after time is initialized.
///
/// Q: why don't we integrate this into the ticktimer?
/// A: The ticktimer must be (1) lightweight and (2) used as a dependency by almost everything.
///    Pulling this functionality into the ticktimer both makes it heavier, and also more importantly,
///    creates circular dependencies on `llio` and `pddb`.
///
/// System time is composed of:
///    "hardware `u64`"" + "offset to RT" -> SystemTime
/// "offset to RT" is composed of:
///   - offset to UTC
///   - offset to current TZ
/// "hardware `u64`" composed of:
///   - the current number of seconds counted by the RTC module
///   *or*
///   - the number of seconds counted by the RTC module at time T + ticktimer offset since T
/// The second representation is an optimization to avoid hitting the I2C module constantly to
/// read RTC, plus you get milliseconds resolution. Time "T" can be updated at any time by just
/// reading the RTC and noting the ticktimer offset at the point of reading.
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(feature="precursor", feature="renode"))]
use llio::*;
use pddb::PddbMountPoller;
use num_traits::*;
// imports for time ux
use locales::t;
use chrono::prelude::*;
use xous::{Message, send_message};
use gam::modal::*;
// ntp imports
use sntpc::{Error, NtpContext, NtpTimestampGenerator, NtpUdpSocket, Result};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::num::ParseIntError;

/// This is a "well known name" used by `libstd` to connect to the time server
/// Anyone who wants to check if time has been initialized would use this name.
pub const TIME_SERVER_PUBLIC: &'static [u8; 16] = b"timeserverpublic";

#[allow(dead_code)]
const CTL3: usize = 0;
#[allow(dead_code)]
const SECS: usize = 1;
#[allow(dead_code)]
const MINS: usize = 2;
#[allow(dead_code)]
const HOURS: usize = 3;
#[allow(dead_code)]
const DAYS: usize = 4;
#[allow(dead_code)]
const WEEKDAYS: usize = 5;
#[allow(dead_code)]
const MONTHS: usize = 6;
#[allow(dead_code)]
const YEARS: usize = 7;

use llio::RTC_PWR_MODE;

/// Do not modify the discriminants in this structure. They are used in `libstd` directly.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum TimeOp {
    /// Sync offsets to hardware RTC
    HwSync = 0,
    /// Suspend/resume call
    // SusRes = 1,
    /// Indicates the current time is precisely the provided number of ms since EPOCH
    SetUtcTimeMs = 2,
    /// Get UTC time in ms since EPOCH
    GetUtcTimeMs = 3,
    /// Get local time in ms since EPOCH
    GetLocalTimeMs = 4,
    /// Sets the timezone offset, in milliseconds.
    SetTzOffsetMs = 5,
    /// Query to see if timezone and time relative to UTC have been set.
    WallClockTimeInit = 6,
    /// Self-poll for PDDB mount
    PddbMountPoll = 7,
}

/// Do not modify the discriminants in this structure. They are used in `libstd` directly.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum PrivTimeOp {
    /// Reset the hardware RTC count
    ResetRtc = 0,
    /// Suspend/resume call
    SusRes = 1,
}

#[derive(Copy, Clone, Default)]
struct StdTimestampGen {
    duration: std::time::Duration,
}
impl NtpTimestampGenerator for StdTimestampGen {
    fn init(&mut self) {
        self.duration = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap();
    }

    fn timestamp_sec(&self) -> u64 {
        self.duration.as_secs()
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        self.duration.subsec_micros()
    }
}


#[derive(Debug)]
struct UdpSocketWrapper(UdpSocket);

impl NtpUdpSocket for UdpSocketWrapper {
    fn send_to<T: ToSocketAddrs>(
        &self,
        buf: &[u8],
        addr: T,
    ) -> Result<usize> {
        match self.0.send_to(buf, addr) {
            Ok(usize) => Ok(usize),
            Err(_) => Err(Error::Network),
        }
    }

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        match self.0.recv_from(buf) {
            Ok((size, addr)) => Ok((size, addr)),
            Err(_) => Err(Error::Network),
        }
    }
}
#[cfg(any(feature="precursor", feature="renode"))]
pub fn reset_rtc(i2c: &mut llio::I2c, start_time: u64, tt: &ticktimer_server::Ticktimer) {
    log::info!("performing rtc reset");
    i2c.i2c_mutex_acquire();
    // issue a "software reset" of the RTC
    i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL1, &[0x58]).expect("RTC access error");
    i2c.i2c_mutex_release();
    tt.sleep_ms(1100).ok(); // give the RTC 1 second to resume register operations
    // note: this readback seems necessary to get the RTC to behave after a reset
    for i in 0..0x14 {
        let mut d = [0u8; 1];
        i2c.i2c_mutex_acquire();
        i2c.i2c_read_no_repeated_start(ABRTCMC_I2C_ADR, i, &mut d).ok();
        log::info!("   reg {:x}: {:x}", i, d[0]);
        i2c.i2c_mutex_release();
    }

    let reset_vals = [
        0, // clear interrupts, normal run
        0, // clear interrupts
        RTC_PWR_MODE, // power mode
        to_bcd((start_time & 0xFF) as u8 % 60), // seconds
        to_bcd(((start_time >> 8) & 0xFF) as u8 % 60), // minutes
        to_bcd(((start_time >> 16) & 0xFF) as u8 % 24), // hours
        to_bcd(((start_time >> 24) & 0xFF) as u8 % 28 + 1), // days
        0, // sunday
        to_bcd(((start_time >> 32) & 0xFF) as u8 % 12 + 1), // months
        to_bcd(((start_time >> 40) & 0xFF) as u8 % 5 + 1), // years
    ];
    i2c.i2c_mutex_acquire();
    log::info!("writing: {:x?}", reset_vals);
    i2c.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL1, &reset_vals).expect("RTC access error");
    i2c.i2c_mutex_release();
    tt.sleep_ms(1100).ok(); // give the RTC 1 second to resume register operations
    i2c.i2c_mutex_acquire();
    let mut readback = [0u8; 10];
    // this readback seems necessary to get values to "stick" in the RTC
    i2c.i2c_read_no_repeated_start(ABRTCMC_I2C_ADR, 0, &mut readback).ok();
    log::info!("   reset readback: {:x?}", readback);
    i2c.i2c_mutex_release();
}

pub fn start_time_server() {
    let rtc_checked = Arc::new(AtomicBool::new(false));

    // the public SID is well known and accessible by anyone who uses `libstd`
    let pub_sid = xous::create_server_with_address(&TIME_SERVER_PUBLIC)
        .expect("Couldn't create Ticktimer server");
    let self_cid = xous::connect(pub_sid).unwrap();

    // this thread handles more sensitive operations on the RTC.
    #[cfg(any(feature="precursor", feature="renode"))]
    thread::spawn({
        let rtc_checked = rtc_checked.clone();
        let self_cid = self_cid.clone();
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let mut i2c = llio::I2c::new(&xns);
            let trng = trng::Trng::new(&xns).unwrap();
            let tt = ticktimer_server::Ticktimer::new().unwrap();

            // allocate the SID for priveleged ops. we expect exactly one connection from the PDDB.
            let priv_sid = xns.register_name(pddb::TIME_SERVER_PDDB, Some(1)).expect("can't register server");
            let priv_conn = xous::connect(priv_sid).unwrap();

            // register a suspend/resume listener
            // let sr_cid = xous::connect(pub_sid).expect("couldn't create suspend callback connection");
            let mut susres = susres::Susres::new(
                Some(susres::SuspendOrder::Early),
                &xns,
                PrivTimeOp::SusRes as u32,
                priv_conn
            ).expect("couldn't create suspend/resume object");

            // on boot, do the validation checks of the RTC. If it is not initialized or corrupted, fix it.
            let mut settings = [0u8; 8];
            loop {
                i2c.i2c_mutex_acquire();
                match i2c.i2c_read_no_repeated_start(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL3, &mut settings) {
                    Ok(I2cStatus::ResponseReadOk) => {
                        i2c.i2c_mutex_release();
                        break
                    },
                    _ => {
                        i2c.i2c_mutex_release();
                        log::error!("Couldn't check RTC, retrying!");
                        xous::yield_slice(); // recheck in a fast loop, we really should be able to grab this resource on boot.
                    },
                };
            }
            if is_rtc_invalid(&settings) {
                log::warn!("RTC settings were invalid. Re-initializing! {:?}", settings);
                reset_rtc(&mut i2c, trng.get_u64().unwrap(), &tt);
            }
            rtc_checked.store(true, Ordering::SeqCst);
            loop {
                let msg = xous::receive_message(priv_sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(PrivTimeOp::ResetRtc) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        log::warn!("RTC time reset command received.");
                        reset_rtc(&mut i2c, trng.get_u64().unwrap(), &tt);
                        xous::return_scalar(msg.sender, 0).unwrap();
                    }),
                    Some(PrivTimeOp::SusRes) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                        susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                        // resync time on resume, but give a little time for other processes to clear as this is not urgent
                        tt.sleep_ms(180).unwrap();
                        send_message(self_cid,
                            Message::new_scalar(TimeOp::HwSync.to_usize().unwrap(), 0, 0, 0, 0)
                        ).expect("couldn't queue sync request");
                    }),
                    _ => log::error!("Time server private thread received unknown opcode: {:?}", msg),
                }
            }
        }
    });

    // this thread handles reading & updating the time offset from the PDDB
    thread::spawn({
        let rtc_checked = rtc_checked.clone();
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let llio = llio::Llio::new(&xns);
            #[cfg(not(feature="minimal-testing"))]
            let prefs = userprefs::Manager::new();

            let tt = ticktimer_server::Ticktimer::new().unwrap();
            // this routine can't proceed until the RTC has passed its power-on sanity checks
            while !rtc_checked.load(Ordering::SeqCst) {
                tt.sleep_ms(42).unwrap();
            }
            let mut start_rtc_secs: u64;
            // this loop allows us to fail slightly more gracefully in the case that there is an RTC hardware failure
            loop {
                match llio.get_rtc_secs() {
                    Ok(s) => {
                        start_rtc_secs = s;
                        break;
                    }
                    Err(e) => {
                        log::warn!("RTC HW should be initialized, but it's not: {:?}", e);
                        tt.sleep_ms(2000).ok(); // wait before polling again
                    }
                }
            }
            let mut start_tt_ms = tt.elapsed_ms();
            log::trace!("start_rtc_secs: {}", start_rtc_secs);
            log::trace!("start_tt_ms: {}", start_tt_ms);

            #[cfg(not(feature="minimal-testing"))]
            let pddb_poller = PddbMountPoller::new();
            // enqueue a the first mount poll message
            #[cfg(not(feature="minimal-testing"))]
            xous::send_message(self_cid,
                xous::Message::new_scalar(TimeOp::PddbMountPoll.to_usize().unwrap(), 0, 0, 0, 0)
            ).expect("couldn't check mount poll");
            // an initial behavior which just retuns the raw RTC time, until the PDDB is mounted.
            let mut temp = 0;
            loop {
                #[cfg(not(feature="minimal-testing"))]
                if pddb_poller.is_mounted_nonblocking() {
                    log::debug!("PDDB mount detected, transitioning to real-time adjusted server");
                    break;
                }
                let msg = xous::receive_message(pub_sid).unwrap();
                let op: Option<TimeOp> = FromPrimitive::from_usize(msg.body.id());
                log::debug!("{:?}", op);
                match op {
                    Some(TimeOp::PddbMountPoll) => {
                        tt.sleep_ms(330).unwrap();
                        if temp < 10 {
                            log::trace!("mount poll");
                        }
                        temp += 1;
                        xous::send_message(self_cid,
                            xous::Message::new_scalar(TimeOp::PddbMountPoll.to_usize().unwrap(), 0, 0, 0, 0)
                        ).expect("couldn't check mount poll");
                    }
                    Some(TimeOp::HwSync) => {
                        start_rtc_secs = llio.get_rtc_secs().expect("couldn't read RTC offset value");
                        start_tt_ms = tt.elapsed_ms();
                    },
                    Some(TimeOp::GetUtcTimeMs) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        let t =
                            start_rtc_secs as i64 * 1000i64
                            + (tt.elapsed_ms() - start_tt_ms) as i64;
                        log::debug!("hw only UTC ms {}", t);
                        xous::return_scalar2(msg.sender,
                            (((t as u64) >> 32) & 0xFFFF_FFFF) as usize,
                            (t as u64 & 0xFFFF_FFFF) as usize,
                        ).expect("couldn't respond to GetUtcTimeMs");
                    }),
                    Some(TimeOp::GetLocalTimeMs) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        let t =
                            start_rtc_secs as i64 * 1000i64
                            + (tt.elapsed_ms() - start_tt_ms) as i64;
                        assert!(t > 0, "time result is negative, this is an error");
                        log::debug!("hw only local ms {}", t);
                        xous::return_scalar2(msg.sender,
                            (((t as u64) >> 32) & 0xFFFF_FFFF) as usize,
                            (t as u64 & 0xFFFF_FFFF) as usize,
                        ).expect("couldn't respond to GetLocalTimeMs");
                    }),
                    Some(TimeOp::WallClockTimeInit) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        // definitely not initialized
                        xous::return_scalar(msg.sender, 0).unwrap();
                    }),
                    _ => log::warn!("Time server can't handle this message yet: {:?}", msg),
                }
            }
            #[cfg(not(feature="minimal-testing"))]
            let mut utc_offset_ms = prefs.utc_offset().unwrap_or_else(|error| {
                log::error!("cannot read utc offset: {:?}", error);
                0
            });
            #[cfg(not(feature="minimal-testing"))]
            let mut tz_offset_ms = prefs.timezone_offset().unwrap_or_else(|error| {
                log::error!("cannot read timezone offset: {:?}", error);
                None
            }).unwrap_or_default();
            #[cfg(feature="minimal-testing")]
            let mut utc_offset_ms = 0;
            #[cfg(feature="minimal-testing")]
            let mut tz_offset_ms = 0;

            log::debug!("offset_key: {}", utc_offset_ms / 1000);
            log::debug!("tz_key: {}", tz_offset_ms / 1000);
            log::debug!("start_rtc_secs: {}", start_rtc_secs);
            log::debug!("start_tt_ms: {}", start_tt_ms);
            loop {
                let msg = xous::receive_message(pub_sid).unwrap();
                let opcode: Option<TimeOp> = FromPrimitive::from_usize(msg.body.id());
                log::debug!("{:?}", opcode);
                match opcode {
                    Some(TimeOp::PddbMountPoll) => {
                        // do nothing, we're mounted now.
                        continue;
                    },
                    Some(TimeOp::HwSync) => {
                        match llio.get_rtc_secs() {
                            Ok(val) => {
                                start_rtc_secs = val;
                                start_tt_ms = tt.elapsed_ms();
                            }
                            Err(e) => {
                                log::warn!("Error syncing time: {:?}; retrying!", e);
                                tt.sleep_ms(82).unwrap();
                                send_message(self_cid,
                                    Message::new_scalar(TimeOp::HwSync.to_usize().unwrap(), 0, 0, 0, 0)
                                ).expect("couldn't queue sync request");
                            }
                        }
                    },
                    Some(TimeOp::GetUtcTimeMs) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        let t =
                            start_rtc_secs as i64 * 1000i64
                            + (tt.elapsed_ms() - start_tt_ms) as i64
                            + utc_offset_ms;
                        if t < 0 { // the offset has some error in it, perhaps due to an RTC reset. reset the offset!
                            log::warn!("Time was negative, recovering from time setting error by clearing utc offset to 0");
                            prefs.set_utc_offset(0).ok();
                            utc_offset_ms = 0;
                        }
                        log::trace!("utc ms {}", t);
                        xous::return_scalar2(msg.sender,
                            (((t as u64) >> 32) & 0xFFFF_FFFF) as usize,
                            (t as u64 & 0xFFFF_FFFF) as usize,
                        ).expect("couldn't respond to GetUtcTimeMs");
                    }),
                    Some(TimeOp::GetLocalTimeMs) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        log::trace!("current offset {}", (start_rtc_secs as i64 * 1000i64 + (tt.elapsed_ms() - start_tt_ms) as i64) / 1000);
                        let t =
                            start_rtc_secs as i64 * 1000i64
                            + (tt.elapsed_ms() - start_tt_ms) as i64
                            + utc_offset_ms
                            + tz_offset_ms;
                        if t < 0 {
                            log::warn!("Time was negative, recovering from time setting error by clearing utc and timezone offsets to 0.");
                            prefs.set_utc_offset(0).ok();
                            prefs.set_timezone_offset(0).ok();
                            utc_offset_ms = 0;
                            tz_offset_ms = 0;
                        }
                        log::trace!("local since epoch {}", t / 1000);
                        xous::return_scalar2(msg.sender,
                            (((t as u64) >> 32) & 0xFFFF_FFFF) as usize,
                            (t as u64 & 0xFFFF_FFFF) as usize,
                        ).expect("couldn't respond to GetLocalTimeMs");
                    }),
                    Some(TimeOp::SetUtcTimeMs) => xous::msg_scalar_unpack!(msg, utc_hi_ms, utc_lo_ms, _, _, {
                        let utc_time_ms = (utc_hi_ms as i64) << 32 | (utc_lo_ms as i64);
                        start_rtc_secs = llio.get_rtc_secs().expect("couldn't read RTC offset value");
                        start_tt_ms = tt.elapsed_ms();
                        log::info!("utc_time: {}", utc_time_ms / 1000);
                        log::info!("rtc_secs: {}", start_rtc_secs);
                        log::info!("start_tt_ms: {}", start_tt_ms);
                        let offset =
                            utc_time_ms -
                            (start_rtc_secs as i64) * 1000;
                        utc_offset_ms = offset;
                        #[cfg(not(feature="minimal-testing"))]
                        prefs.set_utc_offset(offset).unwrap_or_else(|err| {
                            log::error!("cannot set utc offset: {:?}", err);
                        });
                    }),
                    Some(TimeOp::SetTzOffsetMs) => xous::msg_scalar_unpack!(msg, tz_hi_ms, tz_lo_ms, _, _, {
                        let tz_ms = ((tz_hi_ms as i64) << 32) | (tz_lo_ms as i64);
                        // sanity check with very broad bounds: I don't know of any time zones that are more than +/2 days from UTC
                        // 86400 seconds in a day, times 1000 milliseconds, times 2 days
                        if tz_ms < -(86400 * 1000 * 2) ||
                        tz_ms > 86400 * 1000 * 2 {
                            log::warn!("Requested timezone offset {} is out of bounds, ignoring!", tz_ms);
                            continue;
                        } else {
                            tz_offset_ms = tz_ms;
                            #[cfg(not(feature="minimal-testing"))]
                            prefs.set_timezone_offset(tz_ms).unwrap_or_else(|err| {
                                log::error!("cannot set timezone offset: {:?}", err);
                            });
                        }
                    }),
                    Some(TimeOp::WallClockTimeInit) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        if utc_offset_ms == 0 && tz_offset_ms == 0 {
                            xous::return_scalar(msg.sender, 0).unwrap();
                        } else {
                            xous::return_scalar(msg.sender, 1).unwrap();
                        }
                    }),
                    None => log::error!("Time server public thread received unknown opcode: {:?}", msg),
                }
            }
        }
    });

    #[cfg(not(target_os = "xous"))]
    thread::spawn({
        let rtc_checked = rtc_checked.clone();
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            // we expect exactly one connection from the PDDB
            let priv_sid = xns.register_name(pddb::TIME_SERVER_PDDB, Some(1)).expect("can't register server");
            rtc_checked.store(true, Ordering::SeqCst);
            loop {
                let msg = xous::receive_message(priv_sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(PrivTimeOp::ResetRtc) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        log::warn!("RTC time reset command received. This does nothing in hosted mode");
                        xous::return_scalar(msg.sender, 0).unwrap();
                    }),
                    _ => log::error!("Time server private thread received unknown opcode: {:?}", msg),
                }
            }
        }
    });
}

#[allow(dead_code)]
fn is_rtc_invalid(settings: &[u8]) -> bool {
    ((settings[CTL3] & 0xE0) != RTC_PWR_MODE) // power switchover setting should be initialized
    || ((settings[SECS] & 0x80) != 0)  // clock integrity should be guaranteed
    || (to_binary(settings[SECS]) > 59)
    || (to_binary(settings[MINS]) > 59)
    || (to_binary(settings[HOURS]) > 23) // 24 hour mode is default and assumed
    || (to_binary(settings[DAYS]) > 31) || (to_binary(settings[DAYS]) == 0)
    || (to_binary(settings[MONTHS]) > 12) || (to_binary(settings[MONTHS]) == 0)
    || (to_binary(settings[YEARS]) > 99)
}

#[allow(dead_code)]
fn to_binary(bcd: u8) -> u8 {
    (bcd & 0xf) + ((bcd >> 4) * 10)
}
#[allow(dead_code)]
fn to_bcd(binary: u8) -> u8 {
    let mut lsd: u8 = binary % 10;
    if lsd > 9 {
        lsd = 9;
    }
    let mut msd: u8 = binary / 10;
    if msd > 9 {
        msd = 9;
    }

    (msd << 4) | lsd
}

/// this will parse a simple decimal into an i32, multiplied by 1000
/// we do this because the full f32 parsing stuff is pretty heavy, some
/// 28kiB of code
#[inline(never)]
fn simple_kilofloat_parse(input: &str) -> core::result::Result<i32, ParseIntError> {
    if let Some((integer, fraction)) = input.split_once('.') {
        let mut result = integer.parse::<i32>()? * 1000;
        let mut significance = 100i32;
        for (place, digit) in fraction.chars().enumerate() {
            if place >= 3 {
                break;
            }
            if let Some(d) = digit.to_digit(10) {
                if result >= 0 {
                    result += (d as i32) * significance;
                } else {
                    result -= (d as i32) * significance;
                }
                significance /= 10;
            } else {
                return "z".parse::<i32>() // you can't create a ParseIntError any other way
            }
        }
        Ok(result)
    } else {
        let base = input.parse::<i32>()?;
        Ok(base * 1000)
    }
}

pub (crate) fn start_time_ux() {
    thread::spawn({
        move || {
            // some RTC UX structures
            let xns = xous_names::XousNames::new().unwrap();
            let sid = xns.register_name(crate::TIME_UX_NAME, Some(1)).unwrap();
            let modals = modals::Modals::new(&xns).unwrap();
            let timeserver_cid = xous::connect(xous::SID::from_bytes(crate::time::TIME_SERVER_PUBLIC).unwrap()).unwrap();
            let pddb_poller = pddb::PddbMountPoller::new();
            let trng = trng::Trng::new(&xns).unwrap();

            let prefs = userprefs::Manager::new();

            loop {
                let msg = xous::receive_message(sid).unwrap();
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(crate::TimeUxOp::SetTime) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                        if !pddb_poller.is_mounted_nonblocking() {
                            modals.show_notification(t!("stats.please_mount", locales::LANG), None).expect("couldn't show notification");
                            continue;
                        }
                        let mut tz_set = false;
                        let mut tz_offset_ms = match prefs.timezone_offset() {
                            Ok(offset) => match offset {
                                Some(data) => {
                                    tz_set = true;
                                    data
                                },
                                None => {
                                    tz_set = false;
                                    0i64
                                }
                            },
                            Err(error) => {
                                log::error!("cannot read timezone offset: {:?}", error);
                                0i64
                            }
                        };

                        // note that we don't do an "else" here because we also want to catch the case of
                        // a key exists, but nothing was written to it (length of key was 0 or inappropriate)
                        if !tz_set {
                            log::info!("{}RTC.TZ,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                            let tz_str = modals.alert_builder(t!("rtc.timezone", locales::LANG))
                                .field(None, Some(tz_ux_validator))
                                .build()
                                .expect("couldn't get timezone")
                                .first();
                            let tz = simple_kilofloat_parse(tz_str.as_str()).expect("pre-validated input failed to re-parse!");
                            log::info!("got tz offset {}", tz);
                            tz_offset_ms = (tz * 3600) as i64;
                            xous::send_message(timeserver_cid,
                                Message::new_scalar(
                                    crate::time::TimeOp::SetTzOffsetMs.to_usize().unwrap(),
                                    (tz_offset_ms >> 32) as usize,
                                    (tz_offset_ms & 0xFFFF_FFFF) as usize,
                                    0, 0,
                                )
                            ).expect("couldn't set timezone");
                        }

                        // see if we want to try to use NTP or not
                        log::info!("{}RTC.NTP,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                        modals.add_list_item(t!("pddb.yes", locales::LANG)).expect("couldn't build radio item list");
                        modals.add_list_item(t!("pddb.no", locales::LANG)).expect("couldn't build radio item list");
                        let mut try_ntp = true;
                        match modals.get_radiobutton(t!("rtc.try_ntp", locales::LANG)) {
                            Ok(selection) => {
                                if selection == t!("pddb.no", locales::LANG) {
                                    try_ntp = false;
                                }
                            },
                            _ => log::error!("get_radiobutton failed"),
                        }
                        if try_ntp {
                            let local_port = (trng.get_u32().unwrap() % 16384 + 49152) as u16;
                            let socket_addr = SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)), local_port);
                            let socket = UdpSocket::bind(socket_addr).expect("Unable to create UDP socket");
                            log::debug!("NTP rx socket created {:?}", socket);
                            socket
                                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                                .expect("Unable to set UDP socket read timeout");
                            let sock_wrapper = UdpSocketWrapper(socket);
                            let ntp_context = NtpContext::new(StdTimestampGen::default());
                            let result = sntpc::get_time("time.google.com:123", sock_wrapper, ntp_context);
                            match result {
                                Ok(time) => {
                                    log::info!("Got NTP time: {}.{}", time.sec(), time.sec_fraction());
                                    let current_time = Utc.ymd(1970, 1, 1).and_hms(0, 0, 0) + chrono::Duration::seconds(time.sec() as i64);
                                    log::info!("Setting UTC time: {:?}", current_time.to_string());
                                    xous::send_message(timeserver_cid,
                                        Message::new_scalar(
                                            crate::time::TimeOp::SetUtcTimeMs.to_usize().unwrap(),
                                            ((current_time.timestamp_millis() as u64) >> 32) as usize,
                                            (current_time.timestamp_millis() as u64 & 0xFFFF_FFFF) as usize,
                                            0, 0,
                                        )
                                    ).expect("couldn't set time");
                                    log::info!("{}RTC.NTPOK,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                                    continue;
                                }
                                Err(err) => {
                                    log::info!("Err: {:?}", err);
                                    log::info!("{}RTC.NTPFAIL,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                                    modals.show_notification(t!("rtc.ntp_fail", locales::LANG), None).expect("couldn't show NTP error");
                                },
                            }
                        }

                        let mut secs: u8 = 0;
                        let mut mins: u8 = 0;
                        let mut hours: u8 = 0;
                        let mut months: u8 = 0;
                        let mut days: u8 = 0;
                        let mut years: u8 = 0;

                        let date = modals.alert_builder(t!("rtc.set_time_modal", locales::LANG))
                            .field(Some(String::from(t!("rtc.month", locales::LANG))), Some(rtc_ux_validate_month))
                            .field(Some(String::from(t!("rtc.day", locales::LANG))), Some(rtc_ux_validate_day))
                            .field(Some(String::from(t!("rtc.year", locales::LANG))), Some(rtc_ux_validate_year))
                            .field(Some(String::from(t!("rtc.hour", locales::LANG))), Some(rtc_ux_validate_hour))
                            .field(Some(String::from(t!("rtc.minute", locales::LANG))), Some(rtc_ux_validate_minute))
                            .field(Some(String::from(t!("rtc.seconds", locales::LANG))), Some(rtc_ux_validate_seconds))
                            .build()
                            .expect("cannot get date from user");

                        for (index, elem) in date.content().iter().enumerate() {
                            let elem = elem.as_str().parse::<u8>().expect("pre-validated input failed to re-parse!");
                            match index {
                                0 => months = elem,
                                1 => days = elem,
                                2 => years = elem,
                                3 => hours = elem,
                                4 => mins = elem,
                                5 => secs = elem,
                                _ => {},
                            }
                        }

                        log::info!("Setting time: {}/{}/{} {}:{}:{}", months, days, years, hours, mins, secs);
                        let new_dt = chrono::FixedOffset::east((tz_offset_ms / 1000) as i32).ymd(years as i32 + 2000, months as u32, days as u32)
                        .and_hms(hours as u32, mins as u32, secs as u32);
                        xous::send_message(timeserver_cid,
                            Message::new_scalar(
                                crate::time::TimeOp::SetUtcTimeMs.to_usize().unwrap(),
                                ((new_dt.timestamp_millis() as u64) >> 32) as usize,
                                (new_dt.timestamp_millis() as u64 & 0xFFFF_FFFF) as usize,
                                0, 0,
                            )
                        ).expect("couldn't set time");
                    }),
                    Some(crate::TimeUxOp::SetTimeZone) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                        if !pddb_poller.is_mounted_nonblocking() {
                            modals.show_notification(t!("stats.please_mount", locales::LANG), None).expect("couldn't show notification");
                            continue;
                        }

                        let tz_str = modals.alert_builder(t!("rtc.timezone", locales::LANG))
                            .field(None, Some(tz_ux_validator))
                            .build()
                            .expect("couldn't get timezone")
                            .first();
                        let tz = simple_kilofloat_parse(tz_str.as_str())
                            .expect("pre-validated input failed to re-parse!");
                        log::info!("got tz offset {}", tz);
                        let tzoff_ms = (tz * 3600) as i64;
                        xous::send_message(timeserver_cid,
                            Message::new_scalar(
                                crate::time::TimeOp::SetTzOffsetMs.to_usize().unwrap(),
                                (tzoff_ms >> 32) as usize,
                                (tzoff_ms & 0xFFFF_FFFF) as usize,
                                0, 0,
                            )
                        ).expect("couldn't set timezone");
                    }),
                    Some(crate::TimeUxOp::Quit) => {
                        xous::return_scalar(msg.sender, 0).unwrap();
                        break;
                    }
                    None => {
                        log::warn!("unhandled opcode: {:?}", msg);
                    }
                }
            }
            xous::destroy_server(sid).ok();
        }
    });
}

// RTC Ux helper functions
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum ValidatorOp {
    UxMonth,
    UxDay,
    UxYear,
    UxHour,
    UxMinute,
    UxSeconds,
}

fn tz_ux_validator(input: TextEntryPayload) -> Option<ValidatorErr> {
    let text_str = input.as_str();

    match simple_kilofloat_parse(text_str) {
        Ok(input) => if input < -12_000 || input > 14_000 {
            return Some(ValidatorErr::from_str(t!("rtc.range_err", locales::LANG)));
        },
        _ => return Some(ValidatorErr::from_str(t!("rtc.integer_err", locales::LANG))),
    }
    None
}

fn rtc_ux_validate_month(input: TextEntryPayload) -> Option<ValidatorErr> {
    let text_str = input.as_str();

    let input = match text_str.parse::<u32>() {
        Ok(input_int) => input_int,
        _ => return Some(ValidatorErr::from_str(t!("rtc.integer_err", locales::LANG))),
    };

    if input < 1 || input > 12 {
        return Some(ValidatorErr::from_str(t!("rtc.range_err", locales::LANG)))
    }

    None
}

fn rtc_ux_validate_day(input: TextEntryPayload) -> Option<ValidatorErr> {
    let text_str = input.as_str();

    let input = match text_str.parse::<u32>() {
        Ok(input_int) => input_int,
        _ => return Some(ValidatorErr::from_str(t!("rtc.integer_err", locales::LANG))),
    };

    if input < 1 || input > 31 {
        return Some(ValidatorErr::from_str(t!("rtc.range_err", locales::LANG)))
    }

    None
}

fn rtc_ux_validate_year(input: TextEntryPayload) -> Option<ValidatorErr> {
    let text_str = input.as_str();

    let input = match text_str.parse::<u32>() {
        Ok(input_int) => input_int,
        _ => return Some(ValidatorErr::from_str(t!("rtc.integer_err", locales::LANG))),
    };

    if input > 99 {
        return Some(ValidatorErr::from_str(t!("rtc.range_err", locales::LANG)))
    }

    None
}

fn rtc_ux_validate_hour(input: TextEntryPayload) -> Option<ValidatorErr> {
    let text_str = input.as_str();

    let input = match text_str.parse::<u32>() {
        Ok(input_int) => input_int,
        _ => return Some(ValidatorErr::from_str(t!("rtc.integer_err", locales::LANG))),
    };

    if input > 23 {
        return Some(ValidatorErr::from_str(t!("rtc.range_err", locales::LANG)))
    }

    None
}

fn rtc_ux_validate_minute(input: TextEntryPayload) -> Option<ValidatorErr> {
    let text_str = input.as_str();

    let input = match text_str.parse::<u32>() {
        Ok(input_int) => input_int,
        _ => return Some(ValidatorErr::from_str(t!("rtc.integer_err", locales::LANG))),
    };

    if input > 59 {
        return Some(ValidatorErr::from_str(t!("rtc.range_err", locales::LANG)))
    }

    None
}

fn rtc_ux_validate_seconds(input: TextEntryPayload) -> Option<ValidatorErr> {
    let text_str = input.as_str();

    let input = match text_str.parse::<u32>() {
        Ok(input_int) => input_int,
        _ => return Some(ValidatorErr::from_str(t!("rtc.integer_err", locales::LANG))),
    };

    if input > 59 {
        return Some(ValidatorErr::from_str(t!("rtc.range_err", locales::LANG)))
    }

    None
}

