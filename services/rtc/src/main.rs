#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::FromPrimitive;
use xous_ipc::{String, Buffer};
use api::{Return, Opcode};
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack};

#[cfg(target_os = "none")]
mod implementation {
    use log::info;
    #[macro_use]
    use bitflags::*;

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

    fn to_weekday(bcd: u8) -> Weekdays {
        match bcd {
            0 => Weekdays::SUNDAY,
            1 => Weekdays::MONDAY,
            2 => Weekdays::TUESDAY,
            3 => Weekdays::WEDNESDAY,
            4 => Weekdays::THURSDAY,
            5 => Weekdays::FRIDAY,
            6 => Weekdays::SATURDAY,
            _ => Weekdays::SUNDAY,
        }
    }

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
            info!("RTC: hardware initialized");

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


        /// we only support 24 hour mode
        /// TODO: sanity check arguments
        /// TODO: write accesses should happen in a single block, to guarante atomicity of the operation
        pub fn rtc_set(&mut self, secs: u8, mins: u8, hours: u8, days: u8, months: u8, years: u8, d: Weekdays) -> bool {
            let mut txbuf: [u8; 2];

            // make sure battery switchover is enabled, otherwise we won't keep time when power goes off
            txbuf = [ABRTCMC_CONTROL3, (Control3::BATT_STD_BL_EN).bits()];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);

            txbuf = [ABRTCMC_SECONDS, to_bcd(secs)];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);
            self.updated_ticks = get_ticks(&self.p);
            self.seconds = secs;

            txbuf = [ABRTCMC_MINUTES, to_bcd(mins)];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);
            self.minutes = mins;

            txbuf = [ABRTCMC_HOURS, to_bcd(hours)];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);
            self.hours = hours;

            txbuf = [ABRTCMC_DAYS, to_bcd(days)];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);
            self.days = days;

            txbuf = [ABRTCMC_MONTHS, to_bcd(months)];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);
            self.months = months;

            txbuf = [ABRTCMC_YEARS, to_bcd(years)];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);
            self.years = years;

            txbuf = [ABRTCMC_WEEKDAYS, d.bits()];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);
            self.weekday = d;

            true  // sanity check args would return false on fail
        }

        pub fn rtc_update(&mut self) {
            let txbuf: [u8; 1];
            let mut rxbuf: [u8; 7] = [0; 7];

            // only update from RTC if more than 1 second has passed since the last update
            if get_ticks(&self.p) - self.updated_ticks > 1000 {
                // read as a single block to make the time readout atomic
                txbuf = [ABRTCMC_SECONDS];
                i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), Some(&mut rxbuf), I2C_TIMEOUT);

                self.seconds = to_binary(rxbuf[0] & 0x7f);
                self.minutes = to_binary(rxbuf[1] & 0x7f);
                self.hours = to_binary(rxbuf[2] & 0x3f);
                self.days = to_binary(rxbuf[3] & 0x3f);
                self.weekday = to_weekday(rxbuf[4] & 0x7f);
                self.months = to_binary(rxbuf[5] & 0x1f);
                self.years = to_binary(rxbuf[6]);

                self.updated_ticks = get_ticks(&self.p);
            }
        }

        /// testing-only routine -- wakeup self after designated number of seconds
        pub fn wakeup_alarm(&mut self, seconds: u8) {
            let mut txbuf: [u8; 2];

            // make sure battery switchover is enabled
            txbuf = [ABRTCMC_CONTROL3, (Control3::BATT_STD_BL_EN).bits()];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);

            // set clock units to 1 second, output pulse length to ~218ms
            txbuf = [ABRTCMC_TIMERB_CLK, (TimerClk::CLK_1_S | TimerClk::PULSE_218_MS).bits()];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);

            // program elapsed time
            txbuf = [ABRTCMC_TIMERB, seconds];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);

            // enable timerb countdown interrupt, also clears any prior interrupt flag
            txbuf = [ABRTCMC_CONTROL2, (Control2::COUNTDOWN_B_INT).bits()];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);

            // turn on the timer proper -- the system will restart in 5...4..3....
            txbuf = [ABRTCMC_CONFIG, (Config::TIMER_B_ENABLE | Config::CLKOUT_DISABLE).bits()];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);
        }

        pub fn clear_alarm(&mut self) {
            // turn off RTC wakeup timer, in case previously set
            let mut txbuf : [u8; 2] = [ABRTCMC_CONFIG, (Config::CLKOUT_DISABLE).bits()];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);

            // clear all interrupts and flags
            txbuf = [ABRTCMC_CONTROL2, 0];
            i2c_controller(&self.p, ABRTCMC_I2C_ADR, Some(&txbuf), None, I2C_TIMEOUT);
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

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Rtc;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("RTC: my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let rtc_sid = xns.register_name(api::SERVER_NAME_RTC).expect("RTC: can't register server");
    log::trace!("RTC: registered with NS -- {:?}", rtc_sid);

    #[cfg(target_os = "none")]
    let rtc = Rtc::new();

    #[cfg(not(target_os = "none"))]
    let mut rtc = Rtc::new();

    let mut cb_conns: [Option<CID>; 32] = [None; 32];
    log::trace!("RTC: ready to accept requests");
    loop {
        let msg = xous::receive_message(rtc_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SetDateTime) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let dt = buffer.as_flat::<DateTime, _>().unwrap();
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(rtc_sid).unwrap();
    xous::destroy_server(rtc_sid).unwrap();
    log::trace!("quitting");
}
