use bao1x_hal::rtc::*;
use utralib::*;

use crate::api::TIME_SERVER_PUBLIC;
use crate::api::TimeOp;

pub fn start_rtc_service() {
    let _ = std::thread::spawn({
        move || {
            rtc_service();
        }
    });
}

/// `rtc_code` is the value of the RTC register, which represents 1/1024th of a second
/// `rollovers` is the number of times we've seen the register rollover since the beginning of time
///
/// The return value is a number that represents the number of real-time milliseconds seen
/// since an arbitrary point that is tracked by the outer loop (embodied in `utc_offset_ms`)
fn rtc_code_to_ms(rtc_code: u32, rollovers: u32) -> i64 {
    ((rtc_code as i64 + ((rollovers as i64) << 32)) * 1000) / 1024
}

fn rtc_service() -> ! {
    // the public SID is well known and accessible by anyone who uses `libstd`
    let pub_sid =
        xous::create_server_with_address(&TIME_SERVER_PUBLIC).expect("Couldn't create Ticktimer server");

    let rtc_range = xous::map_memory(
        xous::MemoryAddress::new(bao1x_hal::rtc::HW_RTC_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map RTC range");
    let rtc = CSR::new(rtc_range.as_mut_ptr() as *mut u32);

    let mut rtc_rollovers: u32 = 0; // gives us up to 168 years
    let mut last_rtc_val = rtc.r(DR); // use this to detect rollovers
    // TODO:
    //   - check backup register: is the RTC synchronized to the value on disk?
    //   - if yes, read the value and use it to initialize the time offset, instead of using 0.
    let mut utc_offset_ms = 0;
    let mut tz_offset_ms = 0;

    let mut msg_opt = None;
    let mut return_type = 0;
    loop {
        xous::reply_and_receive_next_legacy(pub_sid, &mut msg_opt, &mut return_type).unwrap();
        let msg = msg_opt.as_mut().unwrap();
        let opcode: Option<TimeOp> = num_traits::FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        // capture the value once at the top so all comparisons work off of this as "the time"
        let rtc_val_atomic = rtc.r(DR);
        // every call, check for a rollover
        if last_rtc_val > rtc_val_atomic {
            rtc_rollovers += 1;
            log::info!("RTC rollover: {}", rtc_rollovers);
        }
        last_rtc_val = rtc_val_atomic;
        match opcode {
            Some(TimeOp::HwSync) => {
                // not used as we're going direct to the hardware
            }
            Some(TimeOp::GetUtcTimeMs) => {
                if let Some(scalar) = msg.body.scalar_message_mut() {
                    let t = utc_offset_ms + rtc_code_to_ms(rtc_val_atomic, rtc_rollovers);
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
                    log::debug!("current offset {}", rtc.r(DR));
                    let t = utc_offset_ms + tz_offset_ms + rtc_code_to_ms(rtc_val_atomic, rtc_rollovers);
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
            Some(TimeOp::SetUtcTimeMs) => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let utc_hi_ms = scalar.arg1;
                    let utc_lo_ms = scalar.arg2;
                    let utc_time_ms = (utc_hi_ms as i64) << 32 | (utc_lo_ms as i64);
                    let rtc_offset_ms = rtc_code_to_ms(rtc_val_atomic, rtc_rollovers);
                    log::info!("utc_time: {}", utc_time_ms / 1000);
                    log::info!("rtc_secs: {}", rtc_offset_ms / 1000);
                    let offset = utc_time_ms - rtc_offset_ms;
                    utc_offset_ms = offset;
                    // TODO:
                    //  - commit the UTC offset to disk
                    //  - set the flag on the backup register for time sync to `true`
                }
            }
            Some(TimeOp::SetTzOffsetMs) => {
                if let Some(scalar) = msg.body.scalar_message() {
                    let tz_hi_ms = scalar.arg1;
                    let tz_lo_ms = scalar.arg2;
                    let tz_ms = ((tz_hi_ms as i64) << 32) | (tz_lo_ms as i64);
                    log::info!("TZ offset set to {}", tz_ms / 1000);
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
