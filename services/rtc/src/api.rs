use xous::{Message, ScalarMessage};

pub enum Weekday {
    Sunday,
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
}

pub enum TimeUnits {
    Seconds,
    Minutes,
    Hours,
}

pub struct DateTime {
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub days: u8,
    pub months: u8,
    pub years: u8,
    pub weekday: Weekday,
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum Opcode {
    /// Get datetime. This will be accurate to +/-2s, as the RTC is updated from hardware
    /// once a second.
    GetDateTime,

    /// sets the datetime
    SetDateTime(DateTime),

    /// sets a wake-up alarm. This forces the SoC into power-on state, if it happens to be off.
    /// primarily used to trigger cold reboots, but could have other reasons
    SetWakeupAlarm(u8, TimeUnits),

    /// clear any wakeup alarms that have been set
    ClearWakeupAlarm,
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
        match message {
            Message::BlockingScalar(m) => match m.id {
                0 => Ok(Opcode::GetTrng(m.arg1)),
                _ => Err("TRNG api: unknown BlockingScalar ID"),
            },
            _ => Err("TRNG api: unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::GetTrng(count) => Message::BlockingScalar(ScalarMessage {
                id: 0,
                arg1: count.into(),
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            // _ => panic!("TRNG api: Opcode type not handled by Into(), refer to helper method"),
        }
    }
}
