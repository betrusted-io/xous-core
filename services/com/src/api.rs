use xous::{Message, ScalarMessage};

// NOTE: the use of ComState "verbs" as commands is not meant as a 1:1 mapping of commands
// It's just a convenient abuse of already-defined constants. However, it's intended that
// the COM server on the SoC side abstracts much of the EC bus complexity away.
use com_rs::*;

#[derive(Debug)]
pub enum Opcode {
    /// Battery stats
    BattStats,

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
            Opcode::ImuAccelRead => Message::BlockingScalar(ScalarMessage {
                id: ComState::GYRO_READ.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            _ => todo!("message type not yet implemented")
        }
    }
}
