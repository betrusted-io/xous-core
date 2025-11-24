use bao1x_hal::rtc::*;
use utralib::*;

use crate::api::TimeOp;

/// This is a "well known name" used by `libstd` to connect to the time server
/// Anyone who wants to check if time has been initialized would use this name.
pub const TIME_SERVER_PUBLIC: &'static [u8; 16] = b"timeserverpublic";

pub fn start_rtc_service() {
    let _ = std::thread::spawn({
        move || {
            rtc_service();
        }
    });
}

fn rtc_service() -> ! {
    // the public SID is well known and accessible by anyone who uses `libstd`
    let pub_sid =
        xous::create_server_with_address(&TIME_SERVER_PUBLIC).expect("Couldn't create Ticktimer server");
    let tt = ticktimer::Ticktimer::new().unwrap();

    let rtc_range = xous::map_memory(
        xous::MemoryAddress::new(bao1x_hal::rtc::HW_RTC_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map RTC range");
    let rtc = CSR::new(rtc_range.as_mut_ptr() as *mut u32);

    // this loop allows us to fail slightly more gracefully in the case that there is an RTC hardware
    // failure
    let mut start_rtc_secs = rtc.r(DR);
    let mut start_tt_ms = tt.elapsed_ms();
    let mut utc_offset_ms = 0;
    let mut tz_offset_ms = 0;

    let mut msg_opt = None;
    let mut return_type = 0;
    loop {
        xous::reply_and_receive_next_legacy(pub_sid, &mut msg_opt, &mut return_type).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode: Option<TimeOp> = num_traits::FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            Some(TimeOp::HwSync) => {
                start_rtc_secs = rtc.r(DR);
                start_tt_ms = tt.elapsed_ms();
            }
            Some(TimeOp::GetUtcTimeMs) => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let t = start_rtc_secs as i64 * 1000i64
                        + (tt.elapsed_ms() - start_tt_ms) as i64
                        + utc_offset_ms;
                    if t < 0 {
                        // the offset has some error in it, perhaps due to an RTC reset. reset the offset!
                        log::warn!(
                            "Time was negative, recovering from time setting error by clearing utc offset to 0"
                        );
                        utc_offset_ms = 0;
                    }
                    log::trace!("utc ms {}", t);
                    // `Scalar2` return type
                    return_type = 2;
                    scalar.arg1 = (t as u64 & 0xFFFF_FFFF) as usize;
                    scalar.arg2 = (((t as u64) >> 32) & 0xFFFF_FFFF) as usize;
                }
            }
            Some(TimeOp::GetLocalTimeMs) => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    log::trace!(
                        "current offset {}",
                        (start_rtc_secs as i64 * 1000i64 + (tt.elapsed_ms() - start_tt_ms) as i64) / 1000
                    );
                    let t = start_rtc_secs as i64 * 1000i64
                        + (tt.elapsed_ms() - start_tt_ms) as i64
                        + utc_offset_ms
                        + tz_offset_ms;
                    if t < 0 {
                        log::warn!(
                            "Time was negative, recovering from time setting error by clearing utc and timezone offsets to 0."
                        );
                        utc_offset_ms = 0;
                        tz_offset_ms = 0;
                    }
                    log::trace!("local since epoch {}", t / 1000);
                    return_type = 2;
                    scalar.arg1 = (((t as u64) >> 32) & 0xFFFF_FFFF) as usize;
                    scalar.arg2 = (t as u64 & 0xFFFF_FFFF) as usize;
                }
            }
            /*
            Set with:
                log::info!("Got NTP time: {}.{}", time.sec(), time.sec_fraction());
                let current_time = Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap()
                    + chrono::Duration::seconds(time.sec() as i64);
                xous::send_message(
                    timeserver_cid,
                    Message::new_scalar(
                        crate::time::TimeOp::SetUtcTimeMs.to_usize().unwrap(),
                        ((current_time.timestamp_millis() as u64) >> 32) as usize,
                        (current_time.timestamp_millis() as u64 & 0xFFFF_FFFF) as usize,
                        0,
                        0,
                    ),
                )
                .expect("couldn't set time");
             */
            Some(TimeOp::SetUtcTimeMs) => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let utc_hi_ms = scalar.arg1;
                    let utc_lo_ms = scalar.arg2;
                    let utc_time_ms = (utc_hi_ms as i64) << 32 | (utc_lo_ms as i64);
                    start_rtc_secs = rtc.r(DR);
                    start_tt_ms = tt.elapsed_ms();
                    log::info!("utc_time: {}", utc_time_ms / 1000);
                    log::info!("rtc_secs: {}", start_rtc_secs);
                    log::info!("start_tt_ms: {}", start_tt_ms);
                    let offset = utc_time_ms - (start_rtc_secs as i64) * 1000;
                    utc_offset_ms = offset;
                }
            }
            /*
               `tz` is a float that represents the current UTC time offset (e.g. -8 for singapore)
               see simple_kilofloat_parse() in dns/src/time.rs for an efficient string parser if needed

               let tz_offset_ms = (tz * 3600) as i64;
               xous::send_message(
                   timeserver_cid,
                   Message::new_scalar(
                       crate::time::TimeOp::SetTzOffsetMs.to_usize().unwrap(),
                       (tz_offset_ms >> 32) as usize,
                       (tz_offset_ms & 0xFFFF_FFFF) as usize,
                       0,
                       0,
                   ),
               )
               .expect("couldn't set timezone");
            */
            Some(TimeOp::SetTzOffsetMs) => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let tz_hi_ms = scalar.arg1;
                    let tz_lo_ms = scalar.arg2;
                    let tz_ms = ((tz_hi_ms as i64) << 32) | (tz_lo_ms as i64);
                    // sanity check with very broad bounds: I don't know of any time zones that are more
                    // than +/2 days from UTC 86400 seconds in a day, times 1000
                    // milliseconds, times 2 days
                    if tz_ms < -(86400 * 1000 * 2) || tz_ms > 86400 * 1000 * 2 {
                        log::warn!("Requested timezone offset {} is out of bounds, ignoring!", tz_ms);
                        continue;
                    } else {
                        tz_offset_ms = tz_ms;
                    }
                }
            }
            Some(TimeOp::WallClockTimeInit) => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    return_type = 2;
                    if utc_offset_ms == 0 {
                        scalar.arg1 = 0;
                    } else {
                        scalar.arg2 = 1;
                    }
                }
            }
            // Calling this allows the caller to store the offsets used to compute the current
            // time from the RTC clock setting. The caller would restore time through a pair
            // of calls to SetUtcTimeMs with arg1/arg2, and SetTzOffsetMs with arg3/arg4.
            Some(TimeOp::GetState) => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    scalar.arg1 = (utc_offset_ms & 0xFFFF_FFFFF) as usize;
                    scalar.arg2 = ((utc_offset_ms >> 32) & 0xFFFF_FFFF) as usize;
                    scalar.arg3 = (tz_offset_ms & 0xFFFF_FFFF) as usize;
                    scalar.arg4 = ((tz_offset_ms >> 32) & 0xFFFF_FFFF) as usize;
                }
            }
            None => log::error!("Time server public thread received unknown opcode: {:?}", msg),
        }
    }
}
