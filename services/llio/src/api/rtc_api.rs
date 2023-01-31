#![allow(dead_code)]
use bitflags::*;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub enum Weekday {
    Sunday,
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
}
impl Default for Weekday {
    fn default() -> Self { Weekday::Sunday }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum TimeUnits {
    Seconds,
    Minutes,
    Hours,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, Default)]
pub struct DateTime {
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub days: u8,
    pub months: u8,
    pub years: u8,
    pub weekday: Weekday,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, Default)]
pub struct RtcSessionOffset {
    pub rtc_seconds: u64,
    pub ticktimer_ms: u64,
}

pub const BLOCKING_I2C_TIMEOUT_MS: u64 = 50;

pub const ABRTCMC_I2C_ADR: u8 = 0x68;
pub const ABRTCMC_CONTROL1: u8 = 0x00;
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

pub const ABRTCMC_CONTROL2: u8 = 0x01;
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

pub const ABRTCMC_CONTROL3: u8 = 0x02;
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
        const BATT_DIR_BL_DIS = 0b1010_0000;
        const BATT_DI_BL_DIS  = 0b1110_0000;
    }
}

pub const ABRTCMC_SECONDS: u8 = 0x3;
bitflags! {
    pub struct Seconds: u8 {
        const SECONDS_BCD    = 0b0111_1111;
        const CORRUPTED      = 0b1000_0000;
    }
}

pub const ABRTCMC_MINUTES: u8 = 0x4;
// no bitflags, minutes are BCD whole register

pub const ABRTCMC_HOURS: u8 = 0x5;
bitflags! {
    pub struct Hours: u8 {
        const HR12_HOURS_BCD = 0b0001_1111;
        const HR12_PM_FLAG   = 0b0010_0000;
        const HR24_HOURS_BCD = 0b0011_1111;
    }
}

pub const ABRTCMC_DAYS: u8 = 0x6;
// no bitflags, days are BCD whole register

pub const ABRTCMC_WEEKDAYS: u8 = 0x7;
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

pub const ABRTCMC_MONTHS: u8 = 0x8;
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

pub const ABRTCMC_YEARS: u8 = 0x9;
// no bitflags, years are 00-99 in BCD format

pub const ABRTCMC_MINUTE_ALARM: u8 = 0xA;
pub const ABRTCMC_HOUR_ALARM: u8 = 0xB;
pub const ABRTCMC_DAY_ALARM: u8 = 0xC;
pub const ABRTCMC_WEEKDAY_ALARM: u8 = 0xD;
bitflags! {
    pub struct Alarm: u8 {
        const ENABLE    = 0b1000_0000;
        // all others code minute/hour/day/weekday in BCD LSBs
        const HR12_PM_FLAG    = 0b0010_0000; // only used in hours alarm, 12-hour mode
    }
}

pub const ABRTCMC_CONFIG: u8 = 0xF;
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

pub const ABRTCMC_TIMERA_CLK: u8 = 0x10;
pub const ABRTCMC_TIMERB_CLK: u8 = 0x12;
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

pub const ABRTCMC_TIMERA: u8 = 0x11;
// no bitflags, register is timer period in seconds, and the period is N / (source clock frequency)
pub const ABRTCMC_TIMERB: u8 = 0x13;
// no bitflags, register is timer period in seconds, and the period is N / (source clock frequency)

/// This function takes the raw &[u8] as returned by the RTC I2C low level read function
/// and converts it to a number of seconds. All hardware RTC readings are based off of the
/// BCD equivalent of Jan 1 2000, 00:00:00, but keep in mind this is just an internal representation.
/// We turn this into a u64 number of seconds because what we really want out of the hardware RTC
/// is _just_ a count of seconds from some arbitrary but fixed start point, that we anchor through other
/// algorithms to UTC.
pub fn rtc_to_seconds(settings: &[u8]) -> Option<u64> {
    const CTL3: usize = 0;
    const SECS: usize = 1;
    const MINS: usize = 2;
    const HOURS: usize = 3;
    const DAYS: usize = 4;
    // note 5 is skipped - this is weekdays, and is unused
    const MONTHS: usize = 6;
    const YEARS: usize = 7;
    if ((settings[CTL3] & 0xE0) != crate::RTC_PWR_MODE) // power switchover setting should be initialized
    || (settings[SECS] & 0x80 != 0) { // clock integrity should be guaranteed
        log::error!("RTC is in an uninitialized state!, {:?}", settings);
        return None;
    }
    // this is a secondary check -- I have seen RTC return non-sensical time results before
    // so this is an extra check above and beyond what's in the datasheet
    if (to_binary(settings[SECS]) > 59)
    || (to_binary(settings[MINS]) > 59)
    || (to_binary(settings[HOURS]) > 23) // 24 hour mode is default and assumed
    || (to_binary(settings[DAYS]) > 31) || (to_binary(settings[DAYS]) == 0)
    || (to_binary(settings[MONTHS]) > 12) || (to_binary(settings[MONTHS]) == 0)
    || (to_binary(settings[YEARS]) > 99) {
        log::error!("RTC has invalid digits!");
        return None;
    }
    let mut total_secs: u64 = 0;
    total_secs += to_binary(settings[SECS]) as u64;
    total_secs += to_binary(settings[MINS]) as u64 * 60;
    total_secs += to_binary(settings[HOURS]) as u64 * 3600;
    const SECS_PER_DAY: u64 = 86400;
    // DAYS is checked to be 1-31, so, it's safe to subtract 1 here
    total_secs += (to_binary(settings[DAYS]) as u64 - 1) * SECS_PER_DAY;
    // this will iterate from 0 through 11; december never has an offset added, because its contribution is directly measured in DAYS
    for month in 0..to_binary(settings[MONTHS]) {
        match month {
            0 => total_secs += 0u64,
            1 => total_secs += 31u64 * SECS_PER_DAY,
            2 => {
                // per spec sheet: 1) If the year counter contains a value which is exactly divisible by 4 (including the year 00),
                // the AB-RTCMC-32.768kHz-B5ZE-S3 compensates for leap years by adding a 29th day to February.
                if (to_binary(settings[YEARS]) % 4) == 0 {
                    total_secs += 29u64 * SECS_PER_DAY;
                } else {
                    total_secs += 28u64 * SECS_PER_DAY;
                };
            },
            3 => total_secs += 31u64 * SECS_PER_DAY,
            4 => total_secs += 30u64 * SECS_PER_DAY,
            5 => total_secs += 31u64 * SECS_PER_DAY,
            6 => total_secs += 30u64 * SECS_PER_DAY,
            7 => total_secs += 31u64 * SECS_PER_DAY,
            8 => total_secs += 31u64 * SECS_PER_DAY,
            9 => total_secs += 30u64 * SECS_PER_DAY,
            10 => total_secs += 31u64 * SECS_PER_DAY,
            11 => total_secs += 30u64 * SECS_PER_DAY,
            // December shoud never be encountered in this loop since it's right-exclusive
            _ => panic!("RTC code has an internal error, months encountered an 'impossible' value"),
        }
    }
    // figure out what the last round multiple of leap years was before the current time
    let last_leap = (to_binary(settings[YEARS]) - to_binary(settings[YEARS]) % 4) as u64;
    // now add the contributions of all these prior years
    total_secs += (last_leap / 4) * (365 * 3 + 366) * SECS_PER_DAY;
    // now add the contributions of any years since the last round multiple of leap years
    if to_binary(settings[YEARS]) % 4 != 0 {
        // account for the leap year
        total_secs += 366 * SECS_PER_DAY;
        // now account for successive years
        total_secs += 365 * (((to_binary(settings[YEARS]) % 4) - 1) as u64) * SECS_PER_DAY;
    }
    Some(total_secs)
}

fn to_binary(bcd: u8) -> u8 {
    (bcd & 0xf) + ((bcd >> 4) * 10)
}
