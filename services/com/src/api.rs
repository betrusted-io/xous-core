use xous::{Message, ScalarMessage};

// NOTE: the use of ComState "verbs" as commands is not meant as a 1:1 mapping of commands
// It's just a convenient abuse of already-defined constants. However, it's intended that
// the COM server on the SoC side abstracts much of the EC bus complexity away.
use com_rs::*;
#[derive(Debug, Default)]
pub struct BattStats {
    /// instantaneous voltage in mV
    pub voltage: u16,
    /// state of charge in %, as inferred by impedance tracking
    pub soc: u8,
    /// instantaneous current draw in mA
    pub current: i16,
    /// remaining capacity in mA, as measured by coulomb counting
    pub remaining_capacity: u16,
}

impl From<[usize; 2]> for BattStats {
    fn from(a: [usize; 2]) -> BattStats {
        BattStats {
            voltage: (a[0] & 0xFFFF) as u16,
            soc: ((a[0] >> 16) & 0xFF) as u8,
            current: ((a[1] >> 16) & 0xFFFF) as i16,
            remaining_capacity: (a[1] & 0xFFFF) as u16,
        }
    }
}

impl Into<[usize; 2]> for BattStats {
    fn into(self) -> [usize; 2] {
        [ (self.voltage as usize & 0xffff) | ((self.soc as usize) << 16) & 0xFF_0000,
          (self.remaining_capacity as usize & 0xffff) | ((self.current as usize) << 16) & 0xffff_0000 ]
    }
}

#[derive(Debug)]
pub enum Opcode {
    /// Battery stats
    BattStats,

    /// Battery stats, non-blocking
    BattStatsNb,

    /// Battery stats return
    BattStatsReturn(BattStats),

    /// Query Full charge capacity of the battery
    BattFullCapacity,

    /// Turn Boost Mode On
    BoostOn,

    /// Turn Boost Mode Off
    BoostOff,

    /// Read the current accelerations off the IMU
    ImuAccelRead,

    /// Power off the SoC
    PowerOffSoc,

    /// Ship mode (battery disconnect)
    ShipMode,

    /// Is the battery charging?
    IsCharging,

    /// Set the backlight brightness
    SetBackLight,

    /// Request charging
    RequestCharging,

    /// Erase a region of EC FLASH
    FlashErase,

    /// Program a page of FLASH
    FlashProgram,

    /// Update the SSID list
    SsidScan,

    /// Return the latest SSID list
    SsidFetch,
}

impl<'a> core::convert::TryFrom<&'a Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: &'a Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => {
                if m.id as u16 == ComState::CHG_BOOST_ON.verb {
                    Ok(Opcode::BoostOn)
                } else if m.id as u16 == ComState::CHG_BOOST_OFF.verb {
                    Ok(Opcode::BoostOff)
                } else if m.id as u16 == ComState::POWER_SHIPMODE.verb {
                    Ok(Opcode::ShipMode)
                } else if m.id as u16 == ComState::POWER_OFF.verb {
                    Ok(Opcode::PowerOffSoc)
                } else if m.id as u16 == ComState::STAT.verb {
                    Ok(Opcode::BattStatsNb)
                } else if m.id as u16 == ComState::STAT_RETURN.verb {
                    let raw_stats: [usize; 2] = [m.arg1, m.arg2];
                    Ok(Opcode::BattStatsReturn(raw_stats.into()))
                } else {
                    Err("unrecognized command")
                }
            },
            Message::BlockingScalar(m) => {
                if m.id as u16 == ComState::STAT.verb {
                    Ok(Opcode::BattStats)
                } else if m.id as u16 == ComState::GYRO_READ.verb {
                    Ok(Opcode::ImuAccelRead)
                } else {
                    Err("unrecognized opcode")
                }
            },
            _ => Err("unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::BoostOn => Message::Scalar(ScalarMessage {
                id: ComState::CHG_BOOST_ON.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            Opcode::BoostOff => Message::Scalar(ScalarMessage {
                id: ComState::CHG_BOOST_OFF.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            Opcode::ShipMode => Message::Scalar(ScalarMessage {
                id: ComState::POWER_SHIPMODE.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            Opcode::PowerOffSoc => Message::Scalar(ScalarMessage {
                id: ComState::POWER_OFF.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            Opcode::BattStats => Message::BlockingScalar(ScalarMessage {
                id: ComState::STAT.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            Opcode::BattStatsNb => Message::Scalar(ScalarMessage {
                id: ComState::STAT.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            Opcode::ImuAccelRead => Message::BlockingScalar(ScalarMessage {
                id: ComState::GYRO_READ.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            Opcode::BattStatsReturn(stats) => {
                let raw_stats: [usize; 2] = stats.into();
                Message::Scalar(ScalarMessage {
                    id: ComState::STAT_RETURN.verb as usize,
                    arg1: raw_stats[1],
                    arg2: raw_stats[0],
                    arg3: 0,
                    arg4: 0,
                })
            }
            _ => todo!("message type not yet implemented")
        }
    }
}
