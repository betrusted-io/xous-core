use rand::{CryptoRng, Rng, RngCore};

#[derive(Debug)]
pub struct Trng {}
impl Trng {
    pub fn new(_xns: &xous_names::XousNames) -> Result<Self, xous::Error> { Ok(Trng {}) }

    pub fn get_u32(&self) -> Result<u32, xous::Error> { Ok(rand::thread_rng().gen()) }

    pub fn get_u64(&self) -> Result<u64, xous::Error> { Ok(rand::thread_rng().gen()) }

    pub fn fill_buf(&self, data: &mut [u32]) -> Result<(), xous::Error> {
        for d in data.iter_mut() {
            *d = rand::thread_rng().gen();
        }
        Ok(())
    }

    /// This is copied out of the 0.5 API for rand_core
    pub fn fill_bytes_via_next(&mut self, dest: &mut [u8]) {
        use core::mem::transmute;
        let mut left = dest;
        while left.len() >= 8 {
            let (l, r) = { left }.split_at_mut(8);
            left = r;
            let chunk: [u8; 8] = unsafe { transmute(rand::thread_rng().next_u64().to_le()) };
            l.copy_from_slice(&chunk);
        }
        let n = left.len();
        if n > 4 {
            let chunk: [u8; 8] = unsafe { transmute(rand::thread_rng().next_u64().to_le()) };
            left.copy_from_slice(&chunk[..n]);
        } else if n > 0 {
            let chunk: [u8; 4] = unsafe { transmute(rand::thread_rng().next_u32().to_le()) };
            left.copy_from_slice(&chunk[..n]);
        }
    }
}

impl RngCore for Trng {
    fn next_u32(&mut self) -> u32 { rand::thread_rng().gen() }

    fn next_u64(&mut self) -> u64 { rand::thread_rng().gen() }

    fn fill_bytes(&mut self, dest: &mut [u8]) { self.fill_bytes_via_next(dest); }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        Ok(self.fill_bytes(dest))
    }
}

impl CryptoRng for Trng {}

pub mod api {
    #[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, PartialEq, Eq, Copy, Clone)]
    pub enum TrngTestMode {
        // No test mode configured. Whitened data is sampled.
        None,
        // Raw TRNG output.
        Raw,
    }
}
