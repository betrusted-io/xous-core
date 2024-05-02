/// This is an implementation for SMTs that are accessible only through a SPI
/// register interface. The base and bounds must be translated to SPI accesses
/// in a hardware-specific manner.
pub struct SwapMac {
    pub base: usize,
    pub bounds: usize,
}
impl SwapMac {
    pub fn new(base: usize, bounds: usize) -> Self { SwapMac { base, bounds } }

    pub fn lookup_mac(&self, swap_page_offset: usize) -> [u8; 16] { todo!() }

    pub fn store_mac(&mut self, swap_page_offset: usize, mac: &[u8; 16]) { todo!() }
}
