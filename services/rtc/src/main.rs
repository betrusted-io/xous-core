#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

use num_traits::FromPrimitive;
use xous_ipc::Buffer;
use api::{Return, Opcode, DateTime};
use xous::{CID, msg_scalar_unpack};

#[cfg(target_os = "none")]
mod implementation {
    #![allow(dead_code)]
    use bitflags::*;
    use crate::CB_TO_MAIN_CONN;
    use llio::{I2cStatus, I2cTransaction, Llio};
    use crate::api::{Opcode, DateTime, Weekday};
    use xous_ipc::Buffer;
    use num_traits::ToPrimitive;

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
            if let Some(cb_to_main_conn) = unsafe{CB_TO_MAIN_CONN} {
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
    }

    impl Rtc {
        pub fn new(xns: &xous_names::XousNames) -> Rtc {
            log::trace!("hardware initialized");
            let mut llio = Llio::new(xns).expect("can't connect to LLIO");
            llio.hook_i2c_callback(i2c_callback).expect("can't hook I2C callback");
            Rtc {
                llio,
                rtc_alarm_enabled: false,
                wakeup_alarm_enabled: false,
            }
        }

        /// we only support 24 hour mode
        pub fn rtc_set(&mut self, secs: u8, mins: u8, hours: u8, days: u8, months: u8, years: u8, day: Weekday)
           -> Result<bool, xous::Error> {
            let mut transaction = I2cTransaction::new();
            let mut txbuf: [u8; 258] = [0; 258];

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
            txbuf[0] = ABRTCMC_CONTROL3;
            txbuf[1] = (Control3::BATT_STD_BL_EN).bits();
            txbuf[2] = to_bcd(secs);
            txbuf[3] = to_bcd(mins);
            txbuf[4] = to_bcd(hours);
            txbuf[5] = to_bcd(days);
            txbuf[6] = to_bcd(d.bits);
            txbuf[7] = to_bcd(months);
            txbuf[8] = to_bcd(years);

            transaction.bus_addr = ABRTCMC_I2C_ADR;
            transaction.txbuf = Some(txbuf);
            transaction.txlen = 9;
            transaction.status = I2cStatus::RequestIncoming;
            match self.llio.send_i2c_request(transaction) {
                Ok(status) => {
                    match status {
                        I2cStatus::ResponseInProgress => Ok(true),
                        I2cStatus::ResponseBusy => Err(xous::Error::ServerQueueFull),
                        _ => Err(xous::Error::InternalError),
                    }
                }
                _ => Err(xous::Error::InternalError)
            }
        }

        pub fn rtc_get(&mut self) -> Result<(), xous::Error> {
            let mut transaction = I2cTransaction::new();
            let mut txbuf = [0; 258];
            let rxbuf = [0; 258];
            txbuf[0] = ABRTCMC_SECONDS;
            transaction.bus_addr = ABRTCMC_I2C_ADR;
            transaction.txlen = 1;
            transaction.rxlen = 7;
            transaction.txbuf = Some(txbuf);
            transaction.rxbuf = Some(rxbuf);
            transaction.status = I2cStatus::RequestIncoming;
            match self.llio.send_i2c_request(transaction) {
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

        fn blocking_i2c_write2(&self, tx: [u8; 2]) -> bool {
            let mut transaction = I2cTransaction::new();
            let mut txbuf: [u8; 258] = [0; 258];
            txbuf[0] = tx[0];
            txbuf[1] = tx[1];
            transaction.bus_addr = ABRTCMC_I2C_ADR;
            transaction.txbuf = Some(txbuf);
            transaction.txlen = 2;
            transaction.status = I2cStatus::RequestIncoming;

            while self.llio.poll_i2c_busy().unwrap() {
                xous::yield_slice();
            }
            let mut sent = false;
            while !sent {
                match self.llio.send_i2c_request(transaction) {
                    Ok(status) => {
                        match status {
                            I2cStatus::ResponseInProgress => sent = true,
                            I2cStatus::ResponseBusy => sent = false,
                            _ => {log::error!("try_send_i2c unhandled response"); return false;},
                        }
                    }
                    _ => {log::error!("try_send_i2c unhandled error"); return false;}
                }
                xous::yield_slice();
            }
            true
        }

        /// wakeup self after designated number of seconds
        pub fn wakeup_alarm(&mut self, seconds: u8) {
            self.wakeup_alarm_enabled = true;

            // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
            self.blocking_i2c_write2([ABRTCMC_CONTROL3, (Control3::BATT_STD_BL_EN).bits()]);

            // set clock units to 1 second, output pulse length to ~218ms
            self.blocking_i2c_write2([ABRTCMC_TIMERB_CLK, (TimerClk::CLK_1_S | TimerClk::PULSE_218_MS).bits()]);

            // program elapsed time
            self.blocking_i2c_write2([ABRTCMC_TIMERB, seconds]);

            // enable timerb countdown interrupt, also clears any prior interrupt flag
            let mut control2 = (Control2::COUNTDOWN_B_INT).bits();
            if self.rtc_alarm_enabled {
                control2 |= Control2::COUNTDOWN_A_INT.bits();
            }
            self.blocking_i2c_write2([ABRTCMC_CONTROL2, control2]);

            // turn on the timer proper -- the system will wakeup in 5..4..3....
            let mut config = (Config::CLKOUT_DISABLE | Config::TIMER_B_ENABLE | Config::TIMERB_INT_PULSED).bits();
            if self.rtc_alarm_enabled {
                config |= (Config::TIMER_A_COUNTDWN | Config::TIMERA_SECONDS_INT_PULSED).bits();
            }
            self.blocking_i2c_write2([ABRTCMC_CONFIG, config]);
        }

        pub fn clear_wakeup_alarm(&mut self) {
            self.wakeup_alarm_enabled = false;

            let mut config = Config::CLKOUT_DISABLE.bits();
            if self.rtc_alarm_enabled {
                config |= (Config::TIMER_A_COUNTDWN | Config::TIMERA_SECONDS_INT_PULSED).bits();
            }
            // turn off RTC wakeup timer, in case previously set
            self.blocking_i2c_write2([ABRTCMC_CONFIG, config]);

            // clear my interrupts and flags
            let mut control2 = 0;
            if self.rtc_alarm_enabled {
                control2 |= Control2::COUNTDOWN_A_INT.bits();
            }
            self.blocking_i2c_write2([ABRTCMC_CONTROL2, control2]);
        }



        pub fn rtc_alarm(&mut self, seconds: u8) {
            self.rtc_alarm_enabled = true;
            // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
            self.blocking_i2c_write2([ABRTCMC_CONTROL3, (Control3::BATT_STD_BL_EN).bits()]);

            // set clock units to 1 second, output pulse length to ~218ms
            self.blocking_i2c_write2([ABRTCMC_TIMERA_CLK, (TimerClk::CLK_1_S | TimerClk::PULSE_218_MS).bits()]);

            // program elapsed time
            self.blocking_i2c_write2([ABRTCMC_TIMERA, seconds]);

            // enable timerb countdown interrupt, also clears any prior interrupt flag
            let mut control2 = (Control2::COUNTDOWN_A_INT).bits();
            if self.wakeup_alarm_enabled {
                control2 |= Control2::COUNTDOWN_B_INT.bits();
            }
            self.blocking_i2c_write2([ABRTCMC_CONTROL2, control2]);

            // turn on the timer proper -- interrupt in 5..4..3....
            let mut config = (Config::CLKOUT_DISABLE | Config::TIMER_A_COUNTDWN | Config::TIMERA_SECONDS_INT_PULSED).bits();
            if self.wakeup_alarm_enabled {
                config |= (Config::TIMER_B_ENABLE | Config::TIMERB_INT_PULSED).bits();
            }
            self.blocking_i2c_write2([ABRTCMC_CONFIG, config]);
        }

        pub fn clear_rtc_alarm(&mut self) {
            self.rtc_alarm_enabled = false;
            // turn off RTC wakeup timer, in case previously set
            let mut config = Config::CLKOUT_DISABLE.bits();
            if self.wakeup_alarm_enabled {
                config |= (Config::TIMER_B_ENABLE | Config::TIMERB_INT_PULSED).bits();
            }
            self.blocking_i2c_write2([ABRTCMC_CONFIG, config]);

            // clear my interrupts and flags
            let mut control2 = 0;
            if self.wakeup_alarm_enabled {
                control2 |= Control2::COUNTDOWN_B_INT.bits();
            }
            self.blocking_i2c_write2([ABRTCMC_CONTROL2, control2]);
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use log::info;

    pub struct Rtc {
        pub seconds: u8,
        pub minutes: u8,
        pub hours: u8,
        pub days: u8,
        pub months: u8,
        pub years: u8,
        pub weekday: Weekdays,
        updated_ticks: u64,
    }

    impl Rtc {
        pub fn new() -> Rtc {
            Rtc {
                seconds: 0,
                minutes: 0,
                hours: 0,
                days: 0,
                months: 0,
                years: 0,
                weekday: Weekdays::SUNDAY,
                updated_ticks: 0,
            }
        }

    }
}

static mut CB_TO_MAIN_CONN: Option<CID> = None;
#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Rtc;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let rtc_sid = xns.register_name(api::SERVER_NAME_RTC).expect("can't register server");
    log::trace!("registered with NS -- {:?}", rtc_sid);
    unsafe{CB_TO_MAIN_CONN = Some(xous::connect(rtc_sid).unwrap())};

    #[cfg(target_os = "none")]
    let mut rtc = Rtc::new(&xns);

    #[cfg(not(target_os = "none"))]
    let mut rtc = Rtc::new();

    let ticktimer = ticktimer_server::Ticktimer::new().expect("can't connect to ticktimer");
    let mut dt_cb_conns: [Option<CID>; 32] = [None; 32];
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
            },
            Some(Opcode::RegisterDateTimeCallback) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = Some(xous::connect(sid).unwrap());
                let mut found = false;
                for entry in dt_cb_conns.iter_mut() {
                    if *entry == None {
                        *entry = cid;
                        found = true;
                        break;
                    }
                }
                if !found {
                    log::error!("RegisterDateTimeCallback listener ran out of space registering callback");
                }
            }),
            Some(Opcode::ResponseDateTime) => {
                let incoming_buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let dt = incoming_buffer.to_original::<DateTime, _>().unwrap();
                let ret = Return::ReturnDateTime(dt);
                let outgoing_buf = Buffer::into_buf(ret).or(Err(xous::Error::InternalError)).unwrap();
                for maybe_conn in dt_cb_conns.iter_mut() {
                    if let Some(conn) = maybe_conn {
                        match outgoing_buf.lend(*conn, 0) { // the ID field is ignored on the callback server
                            Err(xous::Error::ServerNotFound) => {
                                *maybe_conn = None
                            },
                            Ok(_) => {},
                            _ => panic!("unhandled error or result in callback processing")
                        }
                    }
                }
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
            }
            Some(Opcode::SetWakeupAlarm) => msg_scalar_unpack!(msg, delay, _, _, _, {
                rtc.wakeup_alarm(delay as u8); // this will block until finished, no callbacks used
            }),
            Some(Opcode::ClearWakeupAlarm) => msg_scalar_unpack!(msg, _, _, _, _, {
                rtc.clear_wakeup_alarm(); // blocks until transaction is finished
            }),
             Some(Opcode::SetRtcAlarm) => msg_scalar_unpack!(msg, delay, _, _, _, {
                rtc.rtc_alarm(delay as u8); // this will block until finished, no callbacks used
            }),
            Some(Opcode::ClearRtcAlarm) => msg_scalar_unpack!(msg, _, _, _, _, {
                rtc.clear_rtc_alarm(); // blocks until transaction is finished
            }),
            None => {
                log::error!("unknown opcode received, exiting");
                break;
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    unsafe{
        if let Some(cb)= CB_TO_MAIN_CONN {
            xous::disconnect(cb).unwrap();
        }
    }
    for entry in dt_cb_conns.iter_mut() {
        if let Some(conn) = entry {
            let dropmsg = Return::Drop;
            let buf = Buffer::into_buf(dropmsg).unwrap();
            buf.lend(*conn, 0).unwrap(); // the ID is ignored for this server
            unsafe{xous::disconnect(*conn).unwrap();}
        }
        *entry = None;
    }
    xns.unregister_server(rtc_sid).unwrap();
    xous::destroy_server(rtc_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}
