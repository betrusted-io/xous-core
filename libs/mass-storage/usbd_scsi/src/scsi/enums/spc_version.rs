use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum SpcVersion {
    //The device server does not claim conformance to any standard.
    None = 0x00,
    //The device server complies to ANSI INCITS 351-2001 (SPC-2).
    Spc2 = 0x04,
    //The device server complies to ANSI INCITS 408-2005 (SPC-3).
    Spc3 = 0x05,
    //The device server complies to SPC-4.
    Spc4 = 0x06,
}
impl Default for SpcVersion {
    fn default() -> Self {
        SpcVersion::Spc4
    }
}