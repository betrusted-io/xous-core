use arbitrary_int::{Number, u4, u6};

use super::DaliOpError;

/* Overview:
 *
 * Two-layer structure allows user-facing functions of form (address, command) or (command, data) that work
 * with all possible input combinations while preventing misformed frames. Alternative would be nested
 * enums.
 *
 * User-facing structures:
 *   Address byte:
 *     Address enum
 *     SpecialCommand101
 *     SpecialCommand110
 *   Data byte:
 *     Brightness
 *     Command
 *     Part*** (different parts exist for different devices)
 *     SpecialCommandData
 *
 * Constants to decode response frames:
 *   mod bitflags
 *
 * User-facing traits:
 *   impl StdCommand
 *   impl SpecialCommand
 *
 * Traits for ForwardFrame constructor:
 *   impl AddressByte
 *   impl DataByte
 *
 */

// Single-field struct to distinguish different kinds of u8s used here
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Brightness(pub u8);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Address {
    Broadcast,
    ShortAddress(u6),
    GroupAddress(u4),
}

#[rustfmt::skip]
#[derive(Clone, Copy, Debug)]
pub enum Command {
// Arc power control commands
    Off                           =   0, // # of command == bit representation
    Up                            =   1, // 0b0000_0001
    Down                          =   2, // 0b0000_0010
    StepUp                        =   3, // etc.
    StepDown                      =   4,
    RecallMaxLevel                =   5,
    RecallMinLevel                =   6,
    StepDownAndOff                =   7,
    OnAndStepUp                   =   8,
    EnableDapcSequence            =   9,
    GoToLastActiveLevel           =  10, // * (part of Dali-2)
    // Reserved                   11-15
    GoToScene0                    =  16, // 0x10
    GoToScene1                    =  17,
    GoToScene2                    =  18,
    GoToScene3                    =  19,
    GoToScene4                    =  20,
    GoToScene5                    =  21,
    GoToScene6                    =  22,
    GoToScene7                    =  23,
    GoToScene8                    =  24,
    GoToScene9                    =  25,
    GoToScene10                   =  26,
    GoToScene11                   =  27,
    GoToScene12                   =  28,
    GoToScene13                   =  29,
    GoToScene14                   =  30,
    GoToScene15                   =  31,
// Configuration commands
// Recommended to send all of these twice
    Reset                         =  32,
    StoreActualLevelInDtr0        =  33,
    SavePersistentVariables       =  34, // *
    SetOperatingMode              =  35, // * sets DTR0 as operating mode
    ResetMemoryBank               =  36, // *
    IdentifyDevice                =  37, // *
    // Reserved                   38-41
    StoreDtrAsMaxLevel            =  42, // set max level from DTR0
    StoreDtrAsMinLevel            =  43,
    StoreDtrAsSystemFailureLevel  =  44,
    SotreDtrAsPowerOnLevel        =  45,
    SotreDtrAsFadeTime            =  46,
    SotreDtrAsFadeRate            =  47,
    SetExtendedFadeTime           =  48, // * (part of Dali-2)
    // Reserved                   49-63
    StoreDtrAsScene0              =  64, // 0x40
    StoreDtrAsScene1              =  65,
    StoreDtrAsScene2              =  66,
    StoreDtrAsScene3              =  67,
    StoreDtrAsScene4              =  68,
    StoreDtrAsScene5              =  69,
    StoreDtrAsScene6              =  70,
    StoreDtrAsScene7              =  71,
    StoreDtrAsScene8              =  72,
    StoreDtrAsScene9              =  73,
    StoreDtrAsScene10             =  74,
    StoreDtrAsScene11             =  75,
    StoreDtrAsScene12             =  76,
    StoreDtrAsScene13             =  77,
    StoreDtrAsScene14             =  78,
    StoreDtrAsScene15             =  79,
    RemoveFromScene0              =  80, // 0x50
    RemoveFromScene1              =  81,
    RemoveFromScene2              =  82,
    RemoveFromScene3              =  83,
    RemoveFromScene4              =  84,
    RemoveFromScene5              =  85,
    RemoveFromScene6              =  86,
    RemoveFromScene7              =  87,
    RemoveFromScene8              =  88,
    RemoveFromScene9              =  89,
    RemoveFromScene10             =  90,
    RemoveFromScene11             =  91,
    RemoveFromScene12             =  92,
    RemoveFromScene13             =  93,
    RemoveFromScene14             =  94,
    RemoveFromScene15             =  95,
    AddToGroup0                   =  96, // 0x60
    AddToGroup1                   =  97,
    AddToGroup2                   =  98,
    AddToGroup3                   =  99,
    AddToGroup4                   = 100,
    AddToGroup5                   = 101,
    AddToGroup6                   = 102,
    AddToGroup7                   = 103,
    AddToGroup8                   = 104,
    AddToGroup9                   = 105,
    AddToGroup10                  = 106,
    AddToGroup11                  = 107,
    AddToGroup12                  = 108,
    AddToGroup13                  = 109,
    AddToGroup14                  = 110,
    AddToGroup15                  = 111,
    RemoveFromGroup0              = 112, // 0x70
    RemoveFromGroup1              = 113,
    RemoveFromGroup2              = 114,
    RemoveFromGroup3              = 115,
    RemoveFromGroup4              = 116,
    RemoveFromGroup5              = 117,
    RemoveFromGroup6              = 118,
    RemoveFromGroup7              = 119,
    RemoveFromGroup8              = 120,
    RemoveFromGroup9              = 121,
    RemoveFromGroup10             = 122,
    RemoveFromGroup11             = 123,
    RemoveFromGroup12             = 124,
    RemoveFromGroup13             = 125,
    RemoveFromGroup14             = 126,
    RemoveFromGroup15             = 127,
    StoreDtrAsShortAddress        = 128,
    EnableWriteMemory             = 129,
    // Reserved                 130-143
// Query Commands
    QueryStatus                   = 144, // 0x90
    QueryControlGearPresent       = 145,
    QueryLampFailure              = 146,
    QueryLampPowerOn              = 147,
    QueryLimitError               = 148,
    QueryResetState               = 149,
    QueryMissingShortAddress      = 150,
    QueryVersionNumber            = 151,
    QueryContentDtr               = 152,
    QueryDeviceType               = 153,
    QueryPhysicalMinLevel         = 154,
    QueryPowerFailure             = 155,
    QueryContentDtr1              = 156,
    QueryContentDtr2              = 157,
    QueryOperatingMode            = 158, // *
    QueryLightSourceType          = 159, // *
    QueryActualLevel              = 160,
    QueryMaxLevel                 = 161,
    QueryMinLevel                 = 162,
    QueryPowerOnLevel             = 163,
    QuerySystemFailureLevel       = 164,
    QueryFadeTimeFadeRate         = 165,
    QueryManufacturerSpecificMode = 166, // * (part of Dali-2)
    QueryNextDeviceType           = 167, // *
    QueryExtendedFadeTime         = 168, // *
    QueryControlGearFailure       = 169, // *
    // Reserved                 170-175
    QueryLevelScene0              = 176, // 0xb0
    QueryLevelScene1              = 177,
    QueryLevelScene2              = 178,
    QueryLevelScene3              = 179,
    QueryLevelScene4              = 180,
    QueryLevelScene5              = 181,
    QueryLevelScene6              = 182,
    QueryLevelScene7              = 183,
    QueryLevelScene8              = 184,
    QueryLevelScene9              = 185,
    QueryLevelScene10             = 186,
    QueryLevelScene11             = 187,
    QueryLevelScene12             = 188,
    QueryLevelScene13             = 189,
    QueryLevelScene14             = 190,
    QueryLevelScene15             = 191,
    QueryGroups0_7                = 192, // Does --addressee-- belong to any of these groups?
    QueryGroups8_15               = 193, // 0/no 1/yes for each bit
    QueryRandomAdressH            = 194, // What are the high 8 bits of random address?
    QueryRandomAdressM            = 195,
    QueryRandomAdressL            = 196,
    ReadMemoryLocation            = 197,
    // Reserved                 198-223
// Extended Commands            224-255
}

#[rustfmt::skip]
#[derive(Clone, Copy, Debug)]
pub enum Part207LedDriver {
    ReferenceSystemPower          = 224,
    EnableCurrentProtector        = 225,
    DisableCurrentProtector       = 226,
    SelectDimmingCurve            = 227, // DTR0: 0 -> log, 1 -> linear
    StoreDtrAsFastFadeTime        = 228,
    // Reserved                 229-236
    QueryGearType                 = 237,
    QueryDimmingCurve             = 238,
    QueryPossibleOperatingModes   = 239,
    QueryFeatures                 = 240,
    QueryFailureStatus            = 241,
    QueryShortCircuit             = 242,
    QueryOpenCircuit              = 243,
    QueryLoadDecrease             = 244,
    QueryLoadIncrease             = 245,
    QueryCurrentProtectorActive   = 246,
    QueryThermalShutdown          = 247,
    QueryThermalOverload          = 248,
    QueryReferenceRunning         = 249,
    QueryReferenceMeasurementFail = 250,
    QueryCurrentProtectorEnabled  = 251,
    QueryOperatingMode            = 252,
    QueryFastFadeTime             = 253,
    QueryMinFastFadeTime          = 254,
    QueryExtendedVersionNumber    = 255,
}

#[rustfmt::skip]
#[derive(Clone, Copy, Debug)]
pub enum SpecialCommand101 {
// Recommended to send Initialize and Randomize twice.
    Terminate             =  0, // command #256 - Release the initialize state
    DataTransferRegister0 =  1, // #257 - Store data byte in DTR0
    Initialize            =  2, // #258 - Set devices in Initialize state (enable special commands)
    Randomize             =  3, // #259 - Generate random address
    Compare               =  4, // #260 - Is the random address <= to the search address?
    Withdraw              =  5, // #261 - Exclude devices from the compare process if search address ==
                                //        random address
    // Reserved              6
    Ping                  =  7, // #263 - Ignored in the slave (?) * (part of Dali-2)
    SearchAddressH        =  8, // #264 - Specify high byte of search address
    SearchAddressM        =  9, // #265
    SearchAddressL        = 10, // #266
    ProgramShortAddress   = 11, // #267 - Selected device shall store received address as short address
                                //        (data byte: 0AAA_AAA1)
                                //        Selected: random == search address or phys. sel. mode
    VerifyShortAddress    = 12, // #268 - Is short address AAA_AAA?
    QueryShortAddress     = 13, // #269 - What's the short address of the device being selected?
    PhysicalSelection     = 14, // #270 - not Dali-2 - set device to physical selection mode, exclude it from
                                //        the compare process
    // Reserved             15
}

#[rustfmt::skip]
#[derive(Clone, Copy, Debug)]
pub enum SpecialCommand110 {
    EnableDeviceType           = 0, // #272 - maybe selects function of Extended***Set-commands?
    DataTransferRegister1      = 1, // #273 - Store data in DTR1
    DataTransferRegister2      = 2, // #274 - Store data in DTR2
    WriteMemoryLocation        = 3, // #275 - Write data into specified mem location
    WriteMemoryLocationNoReply = 4, // #276 - DTR0: address, DTR1: memory bank
    // Reserved     5-15
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpecCmdData {
    None,
    Address(u6),
    Data(u8),
    InitializeBroadcast,
    InitializeShortAddress(u6),
    InitializeDeviceWithoutShortAddress,
}

#[derive(Clone, Copy, Debug)]
pub struct SpCmdData(u8);

// Public because if this is private, the compiler emits a warning
#[derive(Clone, Copy, Debug)]
pub enum GroupBit {
    ShortAddress = 0,
    GroupAddress = 1,
}

#[derive(Clone, Copy, Debug)]
pub enum CommandBit {
    DirectArcPowerCommand = 0,
    OtherCommand = 1,
}

#[derive(Clone, Copy, Debug)]
pub struct ForwardFrame {
    address_byte: u8,
    data_byte: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct BackwardFrame(pub u8);

// Trait for ForwardFrame constructor
pub trait AddrByte {
    fn to_bits(&self) -> u8;
    fn set_group_bit(&self) -> GroupBit { GroupBit::GroupAddress }
}

impl AddrByte for Address {
    fn to_bits(&self) -> u8 {
        match self {
            Address::Broadcast => 0b0111_1110,
            Address::ShortAddress(addr) => addr.as_u8() << 1,
            Address::GroupAddress(grp) => grp.as_u8() << 1,
        }
    }

    fn set_group_bit(&self) -> GroupBit {
        match self {
            Address::ShortAddress(_) => GroupBit::ShortAddress,
            _ => GroupBit::GroupAddress,
        }
    }
}

impl AddrByte for SpecialCommand101 {
    fn to_bits(&self) -> u8 {
        let cmd = *self as u8;
        0b0010_0000 | cmd << 1
    }
}

impl AddrByte for SpecialCommand110 {
    fn to_bits(&self) -> u8 {
        let cmd = *self as u8;
        0b0100_0000 | cmd << 1
    }
}

// Trait for ForwardFrame constructor
pub trait DataByte {
    fn to_bits(&self) -> u8;
    fn set_command_bit(&self) -> CommandBit { CommandBit::OtherCommand }
}

impl DataByte for Brightness {
    fn to_bits(&self) -> u8 { self.0 }

    fn set_command_bit(&self) -> CommandBit { CommandBit::DirectArcPowerCommand }
}
impl DataByte for SpCmdData {
    fn to_bits(&self) -> u8 { self.0 }
}
impl DataByte for Command {
    fn to_bits(&self) -> u8 { *self as u8 }
}
impl DataByte for Part207LedDriver {
    fn to_bits(&self) -> u8 { *self as u8 }
}

impl ForwardFrame {
    pub fn new(address: impl AddrByte, data: impl DataByte) -> Self {
        let group_bit = address.set_group_bit();
        let address_bits = address.to_bits();
        let command_bit = data.set_command_bit();
        let address_byte = (group_bit as u8) << 7 | address_bits | command_bit as u8;
        Self { address_byte, data_byte: data.to_bits() }
    }

    pub fn to_bits(&self) -> u16 { (self.address_byte as u16) << 8 | self.data_byte as u16 }
}

// User-facing trait to send special commands
pub trait SpecialCommand {
    fn match_data_byte(&self, data: SpecCmdData) -> Result<SpCmdData, DaliOpError>;
}

impl SpecialCommand for SpecialCommand101 {
    fn match_data_byte(&self, data: SpecCmdData) -> Result<SpCmdData, DaliOpError> {
        match self {
            SpecialCommand101::Terminate
            | SpecialCommand101::Randomize
            | SpecialCommand101::Compare
            | SpecialCommand101::Withdraw
            | SpecialCommand101::QueryShortAddress
            | SpecialCommand101::PhysicalSelection
            | SpecialCommand101::Ping => match data {
                SpecCmdData::None => Ok(SpCmdData(0)),
                _ => Err(DaliOpError::WrongDataByteForCommand),
            },
            SpecialCommand101::ProgramShortAddress | SpecialCommand101::VerifyShortAddress => match data {
                SpecCmdData::Address(addr) => {
                    let byte = addr.as_u8() << 1 | 0b0000_0001;
                    Ok(SpCmdData(byte))
                }
                _ => Err(DaliOpError::WrongDataByteForCommand),
            },
            SpecialCommand101::Initialize => match data {
                SpecCmdData::InitializeBroadcast => Ok(SpCmdData(0b0000_0000)),
                SpecCmdData::InitializeShortAddress(addr) => {
                    let byte = addr.as_u8() << 1 | 0b0000_0001;
                    Ok(SpCmdData(byte))
                }
                SpecCmdData::InitializeDeviceWithoutShortAddress => Ok(SpCmdData(0b1111_1111)),
                _ => Err(DaliOpError::WrongDataByteForCommand),
            },
            SpecialCommand101::DataTransferRegister0
            | SpecialCommand101::SearchAddressH
            | SpecialCommand101::SearchAddressM
            | SpecialCommand101::SearchAddressL => match data {
                SpecCmdData::Data(data) => Ok(SpCmdData(data)),
                _ => Err(DaliOpError::WrongDataByteForCommand),
            },
        }
    }
}

impl SpecialCommand for SpecialCommand110 {
    fn match_data_byte(&self, data: SpecCmdData) -> Result<SpCmdData, DaliOpError> {
        match data {
            SpecCmdData::Data(data) => Ok(SpCmdData(data)),
            _ => Err(DaliOpError::WrongDataByteForCommand),
        }
    }
}

// User-facing trait to send regular commands
pub trait StdCommand {}

impl StdCommand for Command {}
impl StdCommand for Part207LedDriver {}

// Information to decode backward frames
#[rustfmt::skip]
pub mod bitflags {
    pub mod std {
        pub mod status_info {
            pub const STATUS_OF_CONTROL_GEAR: u8      = 0x01;
            pub const LAMP_FAILURE: u8                = 0x02;
            pub const LAMP_ARC_POWER_ON: u8           = 0x04;
            pub const QUERY_LIMIT_ERROR: u8           = 0x08;
            pub const FADE_RUNNING: u8                = 0x10;
            pub const QUERY_RESET_STATE: u8           = 0x20;
            pub const QUERY_MISSING_SHORT_ADDRESS: u8 = 0x40;
            pub const QUERY_POWER_FAILURE: u8         = 0x80;
        }
        pub mod lamp_failure {
            pub const SHORT_CIRCUIT: u8            = 0x01;
            pub const OPEN_CIRCUIT: u8             = 0x02;
            pub const LOAD_DECREASE: u8            = 0x04;
            pub const LOAD_INCREASE: u8            = 0x08;
            pub const CURRENT_PROTECTOR_ACTIVE: u8 = 0x10;
        }
    }
    pub mod part_207_led_driver {
        pub mod gear_type {
            pub const LED_POWER_SUPPLY_INTEGRATED: u8 = 0x01;
            pub const LED_MODULE_INTEGRATED: u8       = 0x02;
            pub const AC_SUPPLY_POSSIBLE: u8          = 0x04;
            pub const DC_SUPPLY_POSSIBLE: u8          = 0x08;
            // Bits 4-7 unused
        }
        pub mod possible_operating_mode {
            pub const PWM_MODE_IS_POSSIBLE: u8          = 0x01;
            pub const AM_MODE_IS_POSSIBLE: u8           = 0x02;
            pub const OUTPUT_IS_CURRENT_CONTROLLED: u8  = 0x04;
            pub const HIGH_CURRENT_PULSE_MODE: u8       = 0x08;
            // Bits 4-7 unused
        }
        pub mod features {
            pub const SHORT_CIRCUIT_DETECTION_CAN_BE_QUERIED: u8              = 0x01;
            pub const OPEN_CIRCUIT_DETECTION_CAN_BE_QUERIED: u8               = 0x02;
            pub const DETECTION_OF_LOAD_DECREASE_CAN_BE_QUERIED: u8           = 0x04;
            pub const DETECTION_OF_LOAD_INCREASE_CAN_BE_QUERIED: u8           = 0x08;
            pub const CURRENT_PROTECTOR_IS_IMPLEMENTED_AND_CAN_BE_QUERIED: u8 = 0x10;
            pub const THERMAL_SHUTDOWN: u8                                    = 0x20;
            pub const LIGHT_LEVEL_REDUCTION_DUE_TO_OVERTEMPERATURE: u8        = 0x40;
            pub const PHYSICAL_SELECTION_IS_SUPPORTED: u8                     = 0x80;
        }
        pub mod failure_modes {
            pub const SHORT_CIRCUIT_DETECTION_CAN_BE_QUERIED: u8               = 0x01;
            pub const OPEN_CIRCUIT_DETECTION_CAN_BE_QUERIED: u8                = 0x02;
            pub const DETECTION_OF_LOAD_DECREASE_CAN_BE_QUERIED: u8            = 0x04;
            pub const DETECTION_OF_LOAD_INCREASE_CAN_BE_QUERIED: u8            = 0x08;
            pub const CURRENT_PROTECTOR_IS_IMPLEMENTED_AND_CAN_BE_QUERIED: u8  = 0x10;
            pub const THERMAL_SHUTDOWN: u8                                     = 0x20;
            // Bit 6-7 unused
        }
        pub mod operating_mode {
            pub const PWM_MODE_ACTIVE: u8                       = 0x01;
            pub const AM_MODE_ACTIVE: u8                        = 0x02;
            pub const OUTPUT_IS_CURRENT_CONTROLLED: u8          = 0x04;
            pub const HIGH_CURRENT_PULSE_MODE_ACTIVE: u8        = 0x08;
            pub const NON_LOGARITHMIC_DIMMING_CURVE_ACTIVE: u8  = 0x10;
            // Bits 5-7 unused
        }
    }
}
