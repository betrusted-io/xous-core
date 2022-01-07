#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::{FromPrimitive, ToPrimitive};
use xous_ipc::Buffer;
use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack};

use locales::t;
use gam::modal::*;

use core::sync::atomic::{AtomicU32, Ordering};
static CB_TO_MAIN_CONN: AtomicU32 = AtomicU32::new(0);

#[cfg(any(target_os = "none", target_os = "xous"))]
mod implementation {
    #![allow(dead_code)]
    use bitflags::*;
    use crate::CB_TO_MAIN_CONN;
    use core::sync::atomic::Ordering;
    use llio::{I2cStatus, I2cTransaction, Llio};
    use crate::api::{Opcode, DateTime, Weekday};
    use xous_ipc::Buffer;
    use num_traits::ToPrimitive;

    const BLOCKING_I2C_TIMEOUT_MS: u64 = 50;

    const ABRTCMC_I2C_ADR: u8 = 0x68;
    const ABRTCMC_CONTROL1: u8 = 0x00;
    bitflags! {
        pub struct Control1: u8 {
            const CORRECTION_INT  = 0b0000_0001;
            const ALARM_INT       = 0b0000_0010;
            const SECONDS_INT     = 0b0000_0100;
            const HR_MODE_12      = 0b0000_1000;
            const SOFT_RESET      = 0b0001_0000;
            const STOP            = 0b0010_0000;
        }
    }

    const ABRTCMC_CONTROL2: u8 = 0x01;
    bitflags! {
        pub struct Control2: u8 {
            const COUNTDOWN_B_INT = 0b0000_0001;
            const COUNTDOWN_A_INT = 0b0000_0010;
            const WATCHDOG_A_INT  = 0b0000_0100;
            const ALARM_HAPPENED  = 0b0000_1000;
            const SECONDS_HAPPENED= 0b0001_0000;
            const COUNTB_HAPPENED = 0b0010_0000;
            const COUNTA_HAPPENED = 0b0100_0000;
            const WATCHA_HAPPENED = 0b1000_0000;
        }
    }

    const ABRTCMC_CONTROL3: u8 = 0x02;
    bitflags! {
        pub struct Control3: u8 {
            const BATTLOW_INT     = 0b0000_0001;
            const BATTSWITCH_INT  = 0b0000_0010;
            const BATTLOW_STAT    = 0b0000_0100;
            const BATTSW_HAPPENED = 0b0000_1000;

            const BATT_STD_BL_EN  = 0b0000_0000;
            const BATT_DIR_BL_EN  = 0b0010_0000;
            const BATT_DIS_BL_EN  = 0b0100_0000;
            const BATT_STD_BL_DIS = 0b1000_0000;
            const BATT_DI_BL_DIS  = 0b1110_0000;
        }
    }

    const ABRTCMC_SECONDS: u8 = 0x3;
    bitflags! {
        pub struct Seconds: u8 {
            const SECONDS_BCD    = 0b0111_1111;
            const CORRUPTED      = 0b1000_0000;
        }
    }

    const ABRTCMC_MINUTES: u8 = 0x4;
    // no bitflags, minutes are BCD whole register

    const ABRTCMC_HOURS: u8 = 0x5;
    bitflags! {
        pub struct Hours: u8 {
            const HR12_HOURS_BCD = 0b0001_1111;
            const HR12_PM_FLAG   = 0b0010_0000;
            const HR24_HOURS_BCD = 0b0011_1111;
        }
    }

    const ABRTCMC_DAYS: u8 = 0x6;
    // no bitflags, days are BCD whole register

    const ABRTCMC_WEEKDAYS: u8 = 0x7;
    bitflags! {
        pub struct Weekdays: u8 {
            const SUNDAY   = 0b000;
            const MONDAY   = 0b001;
            const TUESDAY  = 0b010;
            const WEDNESDAY= 0b011;
            const THURSDAY = 0b100;
            const FRIDAY   = 0b101;
            const SATURDAY = 0b110;
        }
    }

    const ABRTCMC_MONTHS: u8 = 0x8;
    bitflags! {
        pub struct Months: u8 { // BCD "months"
            const JANUARY      = 0b0_0001;
            const FEBRUARY     = 0b0_0010;
            const MARCH        = 0b0_0011;
            const APRIL        = 0b0_0100;
            const MAY          = 0b0_0101;
            const JUNE         = 0b0_0110;
            const JULY         = 0b0_0111;
            const AUGUST       = 0b0_1000;
            const SEPTEMBER    = 0b0_1001;
            const OCTOBER      = 0b1_0000;
            const NOVEMBER     = 0b1_0001;
            const DECEMBER     = 0b1_0010;
        }
    }

    const ABRTCMC_YEARS: u8 = 0x9;
    // no bitflags, years are 00-99 in BCD format

    const ABRTCMC_MINUTE_ALARM: u8 = 0xA;
    const ABRTCMC_HOUR_ALARM: u8 = 0xB;
    const ABRTCMC_DAY_ALARM: u8 = 0xC;
    const ABRTCMC_WEEKDAY_ALARM: u8 = 0xD;
    bitflags! {
        pub struct Alarm: u8 {
            const ENABLE    = 0b1000_0000;
            // all others code minute/hour/day/weekday in BCD LSBs
            const HR12_PM_FLAG    = 0b0010_0000; // only used in hours alarm, 12-hour mode
        }
    }

    const ABRTCMC_CONFIG: u8 = 0xF;
    bitflags! {
        pub struct Config: u8 {
            const TIMER_B_ENABLE   = 0b0000_0001;
            const TIMER_A_WATCHDOG = 0b0000_0100;
            const TIMER_A_COUNTDWN = 0b0000_0010;
            const TIMER_A_DISABLE  = 0b0000_0000;
            const TIMER_A_DISABLE2 = 0b0000_0110;

            const CLKOUT_32768_HZ  = 0b0000_0000;
            const CLKOUT_16384_HZ  = 0b0000_1000;
            const CLKOUT_8192_HZ   = 0b0001_0000;
            const CLKOUT_4096_HZ   = 0b0001_1000;
            const CLKOUT_1024_HZ   = 0b0010_0000;
            const CLKOUT_32_HZ     = 0b0010_1000;
            const CLKOUT_1_HZ      = 0b0011_0000;
            const CLKOUT_DISABLE   = 0b0011_1000;

            const TIMERB_INT_PULSED = 0b0100_0000;
            const TIMERA_SECONDS_INT_PULSED = 0b1000_0000;
        }
    }

    const ABRTCMC_TIMERA_CLK: u8 = 0x10;
    const ABRTCMC_TIMERB_CLK: u8 = 0x12;
    bitflags! {
        pub struct TimerClk: u8 {
            const CLK_3600_S  = 0b0000_0100;
            const CLK_60_S    = 0b0000_0011;
            const CLK_1_S     = 0b0000_0010;
            const CLK_64_HZ   = 0b0000_0001;  // 15.625ms
            const CLK_4096_HZ = 0b0000_0000;  // 0.2441ms

            const PULSE_46_MS  = 0b0000_0000;
            const PULSE_62_MS  = 0b0001_0000;
            const PULSE_78_MS  = 0b0010_0000;
            const PULSE_93_MS  = 0b0011_0000;
            const PULSE_125_MS = 0b0100_0000;
            const PULSE_156_MS = 0b0101_0000;
            const PULSE_187_MS = 0b0110_0000;
            const PULSE_218_MS = 0b0111_0000;
        }
    }

    const ABRTCMC_TIMERA: u8 = 0x11;
    // no bitflags, register is timer period in seconds, and the period is N / (source clock frequency)
    const ABRTCMC_TIMERB: u8 = 0x13;
    // no bitflags, register is timer period in seconds, and the period is N / (source clock frequency)

    /// convert binary to BCD
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

    fn to_binary(bcd: u8) -> u8 {
        (bcd & 0xf) + ((bcd >> 4) * 10)
    }

    fn to_weekday(bcd: u8) -> Weekday {
        match bcd {
            0 => Weekday::Sunday,
            1 => Weekday::Monday,
            2 => Weekday::Tuesday,
            3 => Weekday::Wednesday,
            4 => Weekday::Thursday,
            5 => Weekday::Friday,
            6 => Weekday::Saturday,
            _ => Weekday::Sunday,
        }
    }

    fn i2c_callback(trans: I2cTransaction) {
        if trans.status == I2cStatus::ResponseReadOk {
            let cb_to_main_conn = CB_TO_MAIN_CONN.load(Ordering::Relaxed);
            if cb_to_main_conn != 0 {
                // we expect this to be 0, as we don't set it
                assert!(trans.callback_id == 0, "callback ID was incorrect!");
                if let Some(rxbuf) = trans.rxbuf {
                    let dt = DateTime {
                        seconds: to_binary(rxbuf[0] & 0x7f),
                        minutes: to_binary(rxbuf[1] & 0x7f),
                        hours: to_binary(rxbuf[2] & 0x3f),
                        days: to_binary(rxbuf[3] & 0x3f),
                        weekday: to_weekday(rxbuf[4] & 0x7f),
                        months: to_binary(rxbuf[5] & 0x1f),
                        years: to_binary(rxbuf[6]),
                    };
                    let buf = Buffer::into_buf(dt).unwrap();
                    buf.send(cb_to_main_conn, Opcode::ResponseDateTime.to_u32().unwrap()).unwrap();
                } else {
                    log::error!("i2c_callback: no rx data to unpack!")
                }
            } else {
                log::error!("i2c_callback happened, but no connection to the main server!");
            }
        }
    }

    pub struct Rtc {
        llio: Llio,
        rtc_alarm_enabled: bool,
        wakeup_alarm_enabled: bool,
        ticktimer: ticktimer_server::Ticktimer,
    }

    impl Rtc {
        pub fn new(xns: &xous_names::XousNames) -> Rtc {
            log::trace!("hardware initialized");
            let llio = Llio::new(xns).expect("can't connect to LLIO");
            Rtc {
                llio,
                rtc_alarm_enabled: false,
                wakeup_alarm_enabled: false,
                ticktimer: ticktimer_server::Ticktimer::new().expect("can't connect to ticktimer"),
            }
        }

        /// we only support 24 hour mode
        pub fn rtc_set(&mut self, secs: u8, mins: u8, hours: u8, days: u8, months: u8, years: u8, day: Weekday)
           -> Result<bool, xous::Error> {
            let mut txbuf: [u8; 8] = [0; 8];

            if secs > 59 { return Ok(false); }
            if mins > 59 { return Ok(false); }
            if hours > 23 { return Ok(false); }
            if days > 31 { return Ok(false); }
            if months > 12 { return Ok(false); }
            if years > 99 { return Ok(false); }

            // convert enum to bitfields
            let d = match day {
                Weekday::Monday => Weekdays::MONDAY,
                Weekday::Tuesday => Weekdays::TUESDAY,
                Weekday::Wednesday => Weekdays::WEDNESDAY,
                Weekday::Thursday => Weekdays::THURSDAY,
                Weekday::Friday => Weekdays::FRIDAY,
                Weekday::Saturday => Weekdays::SATURDAY,
                Weekday::Sunday => Weekdays::SUNDAY,
            };

            // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
            txbuf[0] = (Control3::BATT_STD_BL_EN).bits();
            txbuf[1] = to_bcd(secs);
            txbuf[2] = to_bcd(mins);
            txbuf[3] = to_bcd(hours);
            txbuf[4] = to_bcd(days);
            txbuf[5] = to_bcd(d.bits);
            txbuf[6] = to_bcd(months);
            txbuf[7] = to_bcd(years);

            match self.llio.i2c_write(ABRTCMC_I2C_ADR, ABRTCMC_CONTROL3, &txbuf, None) {
                Ok(status) => {
                    match status {
                        I2cStatus::ResponseWriteOk => Ok(true),
                        I2cStatus::ResponseBusy => Err(xous::Error::ServerQueueFull),
                        _ => {log::error!("try_send_i2c unhandled response: {:?}", status); Err(xous::Error::InternalError)},
                    }
                }
                _ => {log::error!("try_send_i2c unhandled error"); Err(xous::Error::InternalError)}
            }
        }

        pub fn rtc_get(&mut self) -> Result<(), xous::Error> {
            let mut rxbuf = [0; 7];
            match self.llio.i2c_read(ABRTCMC_I2C_ADR, ABRTCMC_SECONDS, &mut rxbuf, Some(i2c_callback)) {
                Ok(status) => {
                    match status {
                        I2cStatus::ResponseInProgress => Ok(()),
                        I2cStatus::ResponseBusy => Err(xous::Error::ServerQueueFull),
                        _ => Err(xous::Error::InternalError),
                    }
                }
                _ => Err(xous::Error::InternalError)
            }
        }
        pub fn rtc_get_ack(&mut self) {
            self.llio.i2c_async_done();
        }

        // the awkward array syntax is a legacy of a port from a previous implementation
        // would be fine to clean up method signature as e.g.
        // blocking_i2c_write2(adr: u8, data: u8) -> bool
        // but need to make sure we don't bork any of the constants later on in this code :P
        fn blocking_i2c_write2(&mut self, adr: u8, data: u8) -> bool {
            match self.llio.i2c_write(ABRTCMC_I2C_ADR, adr, &[data], None) {
                Ok(status) => {
                    match status {
                        I2cStatus::ResponseWriteOk => true,
                        I2cStatus::ResponseBusy => false,
                        _ => {log::error!("try_send_i2c unhandled response: {:?}", status); return false;},
                    }
                }
                _ => {log::error!("try_send_i2c unhandled error"); return false;}
            }
        }

        /// wakeup self after designated number of seconds
        pub fn wakeup_alarm(&mut self, seconds: u8) {
            self.wakeup_alarm_enabled = true;

            log::trace!("wakeup: switchover");
            // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
            self.blocking_i2c_write2(ABRTCMC_CONTROL3, (Control3::BATT_STD_BL_EN).bits());

            log::trace!("wakeup: timerb_clk");
            // set clock units to 1 second, output pulse length to ~218ms
            self.blocking_i2c_write2(ABRTCMC_TIMERB_CLK, (TimerClk::CLK_1_S | TimerClk::PULSE_218_MS).bits());

            log::trace!("wakeup: timerb");
            // program elapsed time
            self.blocking_i2c_write2(ABRTCMC_TIMERB, seconds);

            log::trace!("wakeup: b_int");
            // enable timerb countdown interrupt, also clears any prior interrupt flag
            let mut control2 = (Control2::COUNTDOWN_B_INT).bits();
            if self.rtc_alarm_enabled {
                control2 |= Control2::COUNTDOWN_A_INT.bits();
            }
            self.blocking_i2c_write2(ABRTCMC_CONTROL2, control2);

            log::trace!("wakeup: config");
            // turn on the timer proper -- the system will wakeup in 5..4..3....
            let mut config = (Config::CLKOUT_DISABLE | Config::TIMER_B_ENABLE).bits();
            if self.rtc_alarm_enabled {
                config |= (Config::TIMER_A_COUNTDWN | Config::TIMERA_SECONDS_INT_PULSED).bits();
            }
            self.blocking_i2c_write2(ABRTCMC_CONFIG, config);
        }

        pub fn clear_wakeup_alarm(&mut self) {
            self.wakeup_alarm_enabled = false;

            // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
            self.blocking_i2c_write2(ABRTCMC_CONTROL3, (Control3::BATT_STD_BL_EN).bits());

            let mut config = Config::CLKOUT_DISABLE.bits();
            if self.rtc_alarm_enabled {
                config |= (Config::TIMER_A_COUNTDWN | Config::TIMERA_SECONDS_INT_PULSED).bits();
            }
            // turn off RTC wakeup timer, in case previously set
            self.blocking_i2c_write2(ABRTCMC_CONFIG, config);

            // clear my interrupts and flags
            let mut control2 = 0;
            if self.rtc_alarm_enabled {
                control2 |= Control2::COUNTDOWN_A_INT.bits();
            }
            self.blocking_i2c_write2(ABRTCMC_CONTROL2, control2);
        }



        pub fn rtc_alarm(&mut self, seconds: u8) {
            self.rtc_alarm_enabled = true;
            // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
            self.blocking_i2c_write2(ABRTCMC_CONTROL3, (Control3::BATT_STD_BL_EN).bits());

            // set clock units to 1 second, output pulse length to ~218ms
            self.blocking_i2c_write2(ABRTCMC_TIMERA_CLK, (TimerClk::CLK_1_S | TimerClk::PULSE_218_MS).bits());

            // program elapsed time
            self.blocking_i2c_write2(ABRTCMC_TIMERA, seconds);

            // enable timerb countdown interrupt, also clears any prior interrupt flag
            let mut control2 = (Control2::COUNTDOWN_A_INT).bits();
            if self.wakeup_alarm_enabled {
                control2 |= Control2::COUNTDOWN_B_INT.bits();
            }
            self.blocking_i2c_write2(ABRTCMC_CONTROL2, control2);

            // turn on the timer proper -- interrupt in 5..4..3....
            let mut config = (Config::CLKOUT_DISABLE | Config::TIMER_A_COUNTDWN | Config::TIMERA_SECONDS_INT_PULSED).bits();
            if self.wakeup_alarm_enabled {
                config |= (Config::TIMER_B_ENABLE).bits();
            }
            self.blocking_i2c_write2(ABRTCMC_CONFIG, config);
        }

        pub fn clear_rtc_alarm(&mut self) {
            self.rtc_alarm_enabled = false;
            // turn off RTC wakeup timer, in case previously set
            let mut config = Config::CLKOUT_DISABLE.bits();
            if self.wakeup_alarm_enabled {
                config |= (Config::TIMER_B_ENABLE | Config::TIMERB_INT_PULSED).bits();
            }
            self.blocking_i2c_write2(ABRTCMC_CONFIG, config);

            // clear my interrupts and flags
            let mut control2 = 0;
            if self.wakeup_alarm_enabled {
                control2 |= Control2::COUNTDOWN_B_INT.bits();
            }
            self.blocking_i2c_write2(ABRTCMC_CONTROL2, control2);
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod implementation {
    use crate::api::Weekday;
    use chrono::prelude::*;
    use crate::CB_TO_MAIN_CONN;
    use core::sync::atomic::Ordering;
    use num_traits::ToPrimitive;

    fn rtc_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
        let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
        log::trace!("rtc callback server started");
        loop {
            let msg = xous::receive_message(sid).unwrap();
            log::trace!("rtc callback got msg: {:?}", msg);
            // we only have one purpose, and that's to send this message.
            let cb_to_main_conn = CB_TO_MAIN_CONN.load(Ordering::Relaxed);
            if cb_to_main_conn != 0 {
                log::trace!("rtc_get sending time to main server");
                let now = Local::now();
                let wday: Weekday = match now.weekday() {
                    chrono::Weekday::Mon => Weekday::Monday,
                    chrono::Weekday::Tue => Weekday::Tuesday,
                    chrono::Weekday::Wed => Weekday::Wednesday,
                    chrono::Weekday::Thu => Weekday::Thursday,
                    chrono::Weekday::Fri => Weekday::Friday,
                    chrono::Weekday::Sat => Weekday::Saturday,
                    chrono::Weekday::Sun => Weekday::Sunday,
                };
                let dt = crate::api::DateTime {
                    seconds: now.second() as u8,
                    minutes: now.minute() as u8,
                    hours: now.hour() as u8,
                    months: now.month() as u8,
                    days: now.day() as u8,
                    years: (now.year() - 2000) as u8,
                    weekday: wday,
                };
                let buf = xous_ipc::Buffer::into_buf(dt).unwrap();
                buf.send(cb_to_main_conn, crate::api::Opcode::ResponseDateTime.to_u32().unwrap()).unwrap();
            }
        }
    }

    pub struct Rtc {
        cb_conn: xous::CID,
    }

    impl Rtc {
        pub fn new(_xns: &xous_names::XousNames) -> Rtc {
            let sid = xous::create_server().unwrap();
            let sid_tuple = sid.to_u32();
            let cid = xous::connect(sid).unwrap();
            xous::create_thread_4(rtc_thread, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            Rtc {
                cb_conn: cid,
            }
        }
        pub fn rtc_set(&mut self, _secs: u8, _mins: u8, _hours: u8, _days: u8, _months: u8, _years: u8, _day: Weekday)
           -> Result<bool, xous::Error> {
               Ok(true)
        }
        pub fn rtc_get(&mut self) -> Result<(), xous::Error> {
            xous::send_message(self.cb_conn, xous::Message::new_scalar(0, 0, 0, 0, 0)).unwrap();
            Ok(())
        }
        pub fn wakeup_alarm(&mut self, _seconds: u8) { }
        pub fn clear_wakeup_alarm(&mut self) { }
        pub fn rtc_alarm(&mut self, _seconds: u8) { }
        pub fn clear_rtc_alarm(&mut self) { }
        pub fn rtc_get_ack(&mut self) {
        }
    }
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum ValidatorOp {
    UxMonth,
    UxDay,
    UxYear,
    UxHour,
    UxMinute,
    UxSeconds,
}

fn rtc_ux_validator(input: TextEntryPayload, opcode: u32) -> Option<ValidatorErr> {
    let text_str = input.as_str();
    let input_int = match text_str.parse::<u32>() {
        Ok(input_int) => input_int,
        _ => return Some(ValidatorErr::from_str(t!("rtc.integer_err", xous::LANG))),
    };
    log::trace!("validating input {}, parsed as {} for opcode {}", text_str, input_int, opcode);
    match FromPrimitive::from_u32(opcode) {
        Some(ValidatorOp::UxMonth) => {
            if input_int < 1 || input_int > 12 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxDay) => {
            if input_int < 1 || input_int > 31 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxYear) => {
            if input_int > 99 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxHour) => {
            if input_int > 23 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxMinute) => {
            if input_int > 59 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        Some(ValidatorOp::UxSeconds) => {
            if input_int > 59 {
                return Some(ValidatorErr::from_str(t!("rtc.range_err", xous::LANG)))
            }
        }
        _ => {
            log::error!("internal error: invalid opcode was sent to validator: {:?}", opcode);
            panic!("internal error: invalid opcode was sent to validator");
        }
    }
    None
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Rtc;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // expected connections:
    // - GAM
    // - shellchat/rtc
    // - shellchat/sleep x2
    // - factory test
    // - UX thread (self, created without xns, so does not count)
    // - rootkeys (for coordinating reboot)
    let rtc_sid = xns.register_name(api::SERVER_NAME_RTC, Some(5)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", rtc_sid);
    CB_TO_MAIN_CONN.store(xous::connect(rtc_sid).unwrap(), Ordering::Relaxed);

    #[cfg(any(target_os = "none", target_os = "xous"))]
    let mut rtc = Rtc::new(&xns);

    #[cfg(not(any(target_os = "none", target_os = "xous")))]
    let mut rtc = Rtc::new(&xns);

    let day_of_week_list = [
        t!("rtc.monday", xous::LANG),
        t!("rtc.tuesday", xous::LANG),
        t!("rtc.wednesday", xous::LANG),
        t!("rtc.thursday", xous::LANG),
        t!("rtc.friday", xous::LANG),
        t!("rtc.saturday", xous::LANG),
        t!("rtc.sunday", xous::LANG),
    ];

    let ticktimer = ticktimer_server::Ticktimer::new().expect("can't connect to ticktimer");
    let mut dt_cb_conns: [bool; xous::MAX_CID] = [false; xous::MAX_CID];
    let modals = modals::Modals::new(&xns).unwrap();
    log::trace!("ready to accept requests");
    loop {
        let msg = xous::receive_message(rtc_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SetDateTime) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let dt = buffer.to_original::<DateTime, _>().unwrap();
                let mut sent = false;
                while !sent {
                    match rtc.rtc_set(
                        dt.seconds,
                        dt.minutes,
                        dt.hours,
                        dt.days,
                        dt.months,
                        dt.years,
                        dt.weekday,
                    ) {
                        Ok(true) => {sent = true;},
                        Ok(false) => {sent = true; log::error!("badly formatted arguments setting RTC date and time");}
                        Err(xous::Error::ServerQueueFull) => {
                            sent = false;
                            log::trace!("I2C interface was busy setting date and time, retrying");
                            ticktimer.sleep_ms(1).unwrap();  // wait a quanta of time and then retry
                        },
                        _ => {
                            sent = true;
                            log::error!("error setting RTC date time");
                        }
                    }
                }
                log::trace!("rtc_set of {:?} successful", dt);
            },
            Some(Opcode::RegisterDateTimeCallback) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = xous::connect(sid).unwrap();
                if (cid as usize) < dt_cb_conns.len() {
                    dt_cb_conns[cid as usize] = true;
                } else {
                    // this should "never" happen because we only have up to 32 connections possible per server
                    log::error!("RegisterDateTimeCallback received a CID out of range");
                }
            }),
            Some(Opcode::UnregisterDateTimeCallback) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = xous::connect(sid).unwrap();
                if (cid as usize) < dt_cb_conns.len() {
                    dt_cb_conns[cid as usize] = false;
                } else {
                    log::error!("UnregisterDateTimeCallback CID out of allowable range");
                }
                unsafe{xous::disconnect(cid).unwrap()};
            }),
            Some(Opcode::ResponseDateTime) => {
                rtc.rtc_get_ack(); // let the async callback interface know we returned
                let incoming_buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let dt = incoming_buffer.to_original::<DateTime, _>().unwrap();
                log::trace!("ResponseDateTime received: {:?}", dt);
                for cid in 1..dt_cb_conns.len() { // 0 is not a valid connection
                    if dt_cb_conns[cid as usize] {
                        let outgoing_buf = Buffer::into_buf(dt).or(Err(xous::Error::InternalError)).unwrap();
                        log::trace!("ResponeDateTime sending to {}", cid);
                        match outgoing_buf.lend(cid as u32, Return::ReturnDateTime.to_u32().unwrap()) {
                            Err(xous::Error::ServerNotFound) => {
                                log::trace!("ServerNotFound, dropping connection");
                                dt_cb_conns[cid] = false;
                            },
                            Ok(_) => {
                                log::trace!("RespondeDateTime sent successfully");
                            },
                            _ => panic!("unhandled error or result in callback processing")
                        }
                    }
                }
                log::trace!("ResponeDateTime done");
            },
            Some(Opcode::RequestDateTime) => {
                let mut sent = false;
                while !sent {
                    match rtc.rtc_get() {
                        Ok(_) => sent = true,
                        Err(xous::Error::ServerQueueFull) => {
                            sent = false;
                            log::trace!("I2C interface was busy getting date and time, retrying");
                            ticktimer.sleep_ms(1).unwrap();
                        },
                        _ => {
                            sent = true;
                            log::error!("error requesting RTC date and time");
                        }
                    }
                }
                log::trace!("RequestDateTime completed");
            }
            Some(Opcode::SetWakeupAlarm) => msg_blocking_scalar_unpack!(msg, delay, _, _, _, {
                rtc.wakeup_alarm(delay as u8); // this will block until finished, no callbacks used
                xous::return_scalar(msg.sender, 0).expect("couldn't return to caller");
            }),
            Some(Opcode::ClearWakeupAlarm) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                rtc.clear_wakeup_alarm(); // blocks until transaction is finished
                xous::return_scalar(msg.sender, 0).expect("couldn't return to caller");
            }),
             Some(Opcode::SetRtcAlarm) => msg_blocking_scalar_unpack!(msg, delay, _, _, _, {
                rtc.rtc_alarm(delay as u8); // this will block until finished, no callbacks used
                xous::return_scalar(msg.sender, 0).expect("couldn't return to caller");
            }),
            Some(Opcode::ClearRtcAlarm) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                rtc.clear_rtc_alarm(); // blocks until transaction is finished
                xous::return_scalar(msg.sender, 0).expect("couldn't return to caller");
            }),
            Some(Opcode::UxSetTime) => msg_scalar_unpack!(msg, _, _, _, _, {
                let secs: u8;
                let mins: u8;
                let hours: u8;
                let months: u8;
                let days: u8;
                let years: u8;
                let weekday: Weekday;

                months = modals.get_text(
                    t!("rtc.month", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxMonth.to_u32().unwrap())
                ).expect("couldn't get month").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got months {}", months);

                days = modals.get_text(
                    t!("rtc.day", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxDay.to_u32().unwrap())
                ).expect("couldn't get month").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got days {}", days);

                years = modals.get_text(
                    t!("rtc.year", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxYear.to_u32().unwrap())
                ).expect("couldn't get month").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got years {}", years);

                for dow in day_of_week_list.iter() {
                    modals.add_list_item(dow).expect("couldn't build day of week list");
                }
                let payload = modals.get_radiobutton(t!("rtc.day_of_week", xous::LANG)).expect("couldn't get day of week");
                weekday =
                    if payload.as_str() == t!("rtc.monday", xous::LANG) {
                        Weekday::Monday
                    } else if payload.as_str() == t!("rtc.tuesday", xous::LANG) {
                        Weekday::Tuesday
                    } else if payload.as_str() == t!("rtc.wednesday", xous::LANG) {
                        Weekday::Wednesday
                    } else if payload.as_str() == t!("rtc.thursday", xous::LANG) {
                        Weekday::Thursday
                    } else if payload.as_str() == t!("rtc.friday", xous::LANG) {
                        Weekday::Friday
                    } else if payload.as_str() == t!("rtc.saturday", xous::LANG) {
                        Weekday::Saturday
                    } else {
                        Weekday::Sunday
                    };
                log::debug!("got weekday {:?}", weekday);

                hours = modals.get_text(
                    t!("rtc.hour", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxHour.to_u32().unwrap())
                ).expect("couldn't get hour").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got hours {}", hours);

                mins = modals.get_text(
                    t!("rtc.minute", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxMinute.to_u32().unwrap())
                ).expect("couldn't get minutes").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got minutes {}", mins);

                secs = modals.get_text(
                    t!("rtc.seconds", xous::LANG),
                    Some(rtc_ux_validator), Some(ValidatorOp::UxSeconds.to_u32().unwrap())
                ).expect("couldn't get seconds").as_str()
                .parse::<u8>().expect("pre-validated input failed to re-parse!");
                log::debug!("got seconds {}", secs);

                log::info!("Setting time: {}/{}/{} {}:{}:{} {:?}", months, days, years, hours, mins, secs, weekday);
                rtc.rtc_set(secs, mins, hours, days, months, years, weekday).expect("couldn't set the current time");
            }),
            Some(Opcode::Quit) => {
                log::error!("Quitting RTC server");
                break;
            },
            None => {
                log::error!("unknown opcode {:?}", msg.body.id());
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    unsafe{
        let cb_to_main_conn = CB_TO_MAIN_CONN.load(Ordering::Relaxed);
        if cb_to_main_conn != 0 {
            xous::disconnect(cb_to_main_conn).unwrap();
        }
    }
    for cid in 1..dt_cb_conns.len() {
        if dt_cb_conns[cid as usize] {
            xous::send_message(cid as u32,
                xous::Message::new_scalar(Return::Drop.to_usize().unwrap(), 0, 0, 0, 0)
            ).unwrap();
            unsafe{xous::disconnect(cid as u32).unwrap();}
            dt_cb_conns[cid as usize] = false;
        }
    }
    xns.unregister_server(rtc_sid).unwrap();
    xous::destroy_server(rtc_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
