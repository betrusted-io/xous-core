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
