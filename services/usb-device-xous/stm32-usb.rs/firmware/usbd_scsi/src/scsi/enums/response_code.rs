use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum ResponseCode {
    FixedSenseData = 0x70,
    DescriptorSenseData = 0x72,    
}
impl Default for ResponseCode {
    fn default() -> Self {
        ResponseCode::FixedSenseData
    }
}