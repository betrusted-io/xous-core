/// This defines a set of functions to get and receive MACs (message
/// authentication codes, also referred to as the tag in AES-GCM-SIV.
pub struct SwapMac {
    pub macs: &'static mut [[u8; 16]],
}
impl SwapMac {
    pub fn new(base: usize, bounds: usize) -> Self {
        SwapMac {
            // safety: this is only safe because the loader guarantees memory-mapped SMT is initialized and
            // aligned and properly mapped into the swapper's memory space.
            macs: unsafe { core::slice::from_raw_parts_mut(base as *mut [u8; 16], bounds as usize / 16) },
        }
    }

    pub fn lookup_mac(&self, swap_page_offset: usize) -> [u8; 16] { todo!() }

    pub fn store_mac(&mut self, swap_page_offset: usize, mac: &[u8; 16]) { todo!() }
}
