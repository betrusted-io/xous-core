use packing::{
    Packed,
    PackedSize,
};
use crate::scsi::enums::MediumType;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ModeParameterHeader6 {
    #[pkd(7, 0, 0, 0)]
    pub mode_data_length: u8,

    #[pkd(7, 0, 1, 1)]
    pub medium_type: MediumType,

    #[pkd(7, 0, 2, 2)]
    pub device_specific_parameter: SbcDeviceSpecificParameter,

    #[pkd(7, 0, 3, 3)]
    pub block_descriptor_length: u8,
}
impl Default for ModeParameterHeader6 {
    fn default() -> Self {
        Self {
            mode_data_length: Self::BYTES as u8 - 1,
            medium_type: Default::default(),
            device_specific_parameter: Default::default(),
            block_descriptor_length: 0,
        }
    }
}
impl ModeParameterHeader6 {
    /// Increase the relevant length fields to indicate the provided page follows this header
    /// can be called multiple times but be aware of the max length allocated by CBW
    pub fn increase_length_for_page(&mut self, page_code: PageCode) {
        self.mode_data_length += match page_code {
            PageCode::CachingModePage => CachingModePage::BYTES as u8,
        };
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ModeParameterHeader10 {
    #[pkd(7, 0, 0, 1)]
    pub mode_data_length: u16,

    #[pkd(7, 0, 2, 2)]
    pub medium_type: MediumType,

    #[pkd(7, 0, 3, 3)]
    pub device_specific_parameter: SbcDeviceSpecificParameter,

    #[pkd(0, 0, 4, 4)]
    pub long_lba: bool,

    #[pkd(7, 0, 6, 7)]
    pub block_descriptor_length: u16,
}
impl Default for ModeParameterHeader10 {
    fn default() -> Self {
        Self {
            mode_data_length: Self::BYTES as u16 - 2,
            medium_type: Default::default(),
            device_specific_parameter: Default::default(),
            long_lba: Default::default(),
            block_descriptor_length: 0,
        }
    }
}
impl ModeParameterHeader10 {
    /// Increase the relevant length fields to indicate the provided page follows this header
    /// can be called multiple times but be aware of the max length allocated by CBW
    #[allow(dead_code)]
    pub fn increase_length_for_page(&mut self, page_code: PageCode) {
        self.mode_data_length += match page_code {
            PageCode::CachingModePage => CachingModePage::BYTES as u16,
        };
    }
}


#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed, Default)]
#[packed(big_endian, lsb0)]
pub struct SbcDeviceSpecificParameter {
    #[pkd(7, 7, 0, 0)]
    pub write_protect: bool,

    #[pkd(4, 4, 0, 0)]
    pub disable_page_out_and_force_unit_access_available: bool,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum PageCode {
    CachingModePage = 0x08,
}

/// This is only a partial implementation, there are a whole load of extra
/// fields defined in SBC-3 6.4.5
/// Default config is no read or write cache
#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct CachingModePage {
    #[pkd(5, 0, 0, 0)]
    pub page_code: PageCode,

    #[pkd(7, 0, 1, 1)]
    pub page_length: u8,

    #[pkd(2, 2, 2, 2)]
    pub write_cache_enabled: bool,

    #[pkd(0, 0, 2, 2)]
    pub read_cache_disable: bool,
}
impl Default for CachingModePage {
    fn default() -> Self {
        Self {
            page_code: PageCode::CachingModePage,
            page_length: Self::BYTES as u8,
            write_cache_enabled: false,
            read_cache_disable: true,
        }
    }
}