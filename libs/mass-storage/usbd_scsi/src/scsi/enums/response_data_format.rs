use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum ResponseDataFormat {
    /// A RESPONSE DATA FORMAT field set to 2h indicates that the standard INQUIRY data
    Standard = 0x2,
}

impl Default for ResponseDataFormat {
    fn default() -> Self {
        ResponseDataFormat::Standard
    }
}
