// Types from https://github.com/ithinuel/sh1107-rs/tree/main

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayState {
    Off,
    On,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Normal,
    Inverted,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AddressMode {
    Page,
    Column,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayMode {
    BlackOnWhite,
    WhiteOnBlack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    SetColumnAddress(u8),
    SetAddressMode(AddressMode),
    SetDisplayMode(DisplayMode),
    ForceEntireDisplay(bool),
    SetClkDividerOscFrequency {
        divider: u8,
        osc_freq_ratio: i8,
    },
    SetMultiplexRatio(u8),
    SetStartLine(u8),
    SetSegmentReMap(bool),
    SetCOMScanDirection(Direction),
    SetDisplayOffset(u8),
    SetContrastControl(u8),
    /// Set Charge & Discharge period
    SetChargePeriods {
        precharge: Option<u8>,
        discharge: u8,
    },
    SetVCOMHDeselectLevel(u8),
    SetDCDCSettings(u8),
    DisplayOnOff(DisplayState),
    SetPageAddress(u8),
    StartReadModifyWrite,
    EndReadModifyWrite,
    Nop,
}

impl Command {
    fn encode(self) -> impl Iterator<Item = u8> {
        use either::Either::*;
        match self {
            Self::SetColumnAddress(addr) => {
                assert!(addr < 128);
                Right([addr & 0xF, 0x10 | ((addr & 0x70) >> 4)])
            }
            Self::SetAddressMode(mode) => Left(0x20 | if let AddressMode::Page = mode { 0 } else { 1 }),
            Self::SetContrastControl(contrast) => Right([0x81, contrast]),
            Self::SetSegmentReMap(is_remapped) => Left(0xA0 | if is_remapped { 1 } else { 0 }),
            Self::SetMultiplexRatio(ratio) => {
                assert!((1..=128).contains(&ratio));
                Right([0xA8, ratio - 1])
            }
            Self::ForceEntireDisplay(state) => Left(0xA4 | if state { 1 } else { 0 }),
            Self::SetDisplayMode(mode) => {
                Left(0xA6 | if let DisplayMode::WhiteOnBlack = mode { 1 } else { 0 })
            }
            Self::SetDisplayOffset(offset) => Right([0xD3, offset & 0x7F]),
            Self::SetDCDCSettings(cfg) => Right([0xAD, 0x80 | (cfg & 0x0F)]),
            Self::DisplayOnOff(state) => Left(0xAE | if let DisplayState::On = state { 1 } else { 0 }),
            Self::SetPageAddress(addr) => {
                assert!(addr < 16);
                Left(0xB0 | (addr & 0x0F))
            }
            Self::SetCOMScanDirection(dir) => Left(0xC0 | if let Direction::Normal = dir { 0 } else { 0x08 }),
            Self::SetClkDividerOscFrequency { divider, osc_freq_ratio } => {
                assert!(osc_freq_ratio % 5 == 0, "osc_freq_ratio must be a multiple of 5.");
                assert!((-25..=50).contains(&osc_freq_ratio), "osc_freq_ratio must be within [-25; 50]");
                assert!((1..=16).contains(&divider), "divider must be in [1; 16]");

                let osc_freq_ratio = osc_freq_ratio / 5 + 5;
                Right([0xD5, ((osc_freq_ratio & 0xF) << 4) as u8 | (divider - 1)])
            }
            Self::SetChargePeriods { precharge, discharge } => {
                let precharge = if let Some(v) = precharge {
                    assert!((1..=15).contains(&v));
                    v
                } else {
                    0
                };
                assert!((1..=15).contains(&discharge));
                let arg = discharge << 4 | precharge;

                Right([0xD9, arg])
            }
            Self::SetVCOMHDeselectLevel(arg) => Right([0xDB, arg]),
            Self::SetStartLine(line) => {
                assert!(line < 128);

                Right([0xDC, line & 0x7F])
            }
            Self::StartReadModifyWrite => Left(0xE0),
            Self::EndReadModifyWrite => Left(0xEE),
            Self::Nop => Left(0xE3),
        }
        .map_left(|v| [v])
        .into_iter()
    }
}
