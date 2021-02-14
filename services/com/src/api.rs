use xous::{Message, ScalarMessage, MemoryMessage};

// NOTE: the use of ComState "verbs" as commands is not meant as a 1:1 mapping of commands
// It's just a convenient abuse of already-defined constants. However, it's intended that
// the COM server on the SoC side abstracts much of the EC bus complexity away.
use com_rs_ref as com_rs;
use com_rs::*;

// subtype constants for registering service listeners from the COM
// pub const SUBTYPE_REGISTER_BATTSTATS_LISTENER: u16 = 0;
pub const REGISTER_BATTSTATS_LISTENER: u32 = 0x0022_0000;

#[derive(Debug, Default, Copy, Clone, rkyv::Archive)]
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
        [
            (self.voltage as usize & 0xffff) | ((self.soc as usize) << 16) & 0xFF_0000,
            (self.remaining_capacity as usize & 0xffff)
                | ((self.current as usize) << 16) & 0xffff_0000,
        ]
    }
}
#[allow(dead_code)]
#[derive(Debug, rkyv::Archive)]
pub enum Opcode {
    /// Battery stats
    BattStats,

    /// Battery stats, non-blocking
    BattStatsNb,

    /// Battery stats return
    BattStatsEvent(BattStats),

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
    //FlashProgram(&'a [u8]),

    /// Update the SSID list
    SsidScan,

    /// Return the latest SSID list
    SsidFetch,

    /// Fetch the git ID of the EC
    EcGitRev,

    /// Fetch the firmware rev of the WF200
    Wf200Rev,

    /// Send a line of PDS data
    Wf200PdsLine(xous::String<512>),

    /// Send Rx stats to fcc-agent
    RxStatsAgent,

    /// request for a listener to BattStats events
    RegisterBattStatsListener(xous_names::api::XousServerName),
}

impl core::convert::TryFrom<& Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: & Message) -> Result<Self, Self::Error> {
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
                } else if m.id == xous::names::GID_COM_BATTSTATS_EVENT {
                    let raw_stats: [usize; 2] = [m.arg1, m.arg2];
                    Ok(Opcode::BattStatsEvent(raw_stats.into()))
                } else if m.id as u16 == ComState::WFX_RXSTAT_GET.verb {
                    Ok(Opcode::RxStatsAgent)
                } else {
                    Err("unrecognized command")
                }
            }
            Message::BlockingScalar(m) => {
                if m.id as u16 == ComState::STAT.verb {
                    Ok(Opcode::BattStats)
                } else if m.id as u16 == ComState::GYRO_READ.verb {
                    Ok(Opcode::ImuAccelRead)
                } else if m.id as u16 == ComState::WFX_FW_REV_GET.verb {
                    Ok(Opcode::Wf200Rev)
                } else if m.id as u16 == ComState::EC_GIT_REV.verb {
                    Ok(Opcode::EcGitRev)
                } else {
                    Err("unrecognized opcode")
                }
            },
            /*
            Message::Borrow(m) => {
                if m.id as u16 == ComState::WFX_PDS_LINE_SET.verb {
                    let s = unsafe {
                        core::slice::from_raw_parts(
                            m.buf.as_ptr(),
                            m.valid.map(|x| x.get()).unwrap_or_else(|| m.buf.len()),
                        )
                    };
                    Ok(Opcode::Wf200PdsLine(core::str::from_utf8(s).unwrap()))
                } else if m.id as u32 == REGISTER_BATTSTATS_LISTENER {
                    Ok(Opcode::RegisterBattStatsListener({
                        unsafe { *( (m.buf.as_mut_ptr()) as *mut xous_names::api::XousServerName) }
                    }))
                } else {
                    Err("COM: unknown borrow ID")
                }
            }*/
            _ => Err("unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::BoostOn => Message::Scalar(ScalarMessage {
                id: ComState::CHG_BOOST_ON.verb as _,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::BoostOff => Message::Scalar(ScalarMessage {
                id: ComState::CHG_BOOST_OFF.verb as _,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::ShipMode => Message::Scalar(ScalarMessage {
                id: ComState::POWER_SHIPMODE.verb as _,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::PowerOffSoc => Message::Scalar(ScalarMessage {
                id: ComState::POWER_OFF.verb as _,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::BattStats => Message::BlockingScalar(ScalarMessage {
                id: ComState::STAT.verb as _,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::BattStatsNb => Message::Scalar(ScalarMessage {
                id: ComState::STAT.verb as _,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::ImuAccelRead => Message::BlockingScalar(ScalarMessage {
                id: ComState::GYRO_READ.verb as _,
                arg1: 0,
                arg2: 0,
                arg3: 0,
                arg4: 0,
            }),
            Opcode::BattStatsEvent(stats) => {
                let raw_stats: [usize; 2] = stats.into();
                Message::Scalar(ScalarMessage {
                    id: xous::names::GID_COM_BATTSTATS_EVENT as usize,
                    arg1: raw_stats[0],
                    arg2: raw_stats[1],
                    arg3: 0,
                    arg4: 0,
                })
            },
            Opcode::Wf200Rev => Message::BlockingScalar(ScalarMessage {
                id: ComState::WFX_FW_REV_GET.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            Opcode::EcGitRev => Message::BlockingScalar(ScalarMessage {
                id: ComState::EC_GIT_REV.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            // Wf200PdsLine() uses the direct string "lend" API
            Opcode::RxStatsAgent => Message::Scalar(ScalarMessage {
                id: ComState::WFX_RXSTAT_GET.verb as _, arg1: 0, arg2: 0, arg3: 0, arg4: 0 }),
            _ => panic!("opcode not handled -- maybe you meant to use one of the direct APIs")
        }
    }
}
