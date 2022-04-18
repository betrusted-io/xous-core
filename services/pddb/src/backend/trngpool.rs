use core::mem::size_of;
use aes_gcm_siv::Nonce;
use rand_core::RngCore;
use std::convert::TryInto;
use core::cell::Cell;

/// Crate-shared resource for TRNGs.
pub(crate) struct TrngPool {
    trng: Cell::<trng::Trng>,
    /// The PDDB eats a lot of entropy. Keep a local pool of entropy, so we're not wasting a lot of
    /// overhead passing messages to the TRNG hardware server.
    e_cache: Cell::<Vec::<u8>>,
}
impl TrngPool {
    pub fn new() -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        let mut trng = trng::Trng::new(&xns).unwrap();
        let mut cache: [u8; 8192] = [0; 8192];
        trng.fill_bytes(&mut cache);
        TrngPool {
            trng: Cell::new(trng),
            e_cache: Cell::new(cache.to_vec())
        }
    }
    pub(crate) fn ensure_entropy(&self, amount: usize) {
        if self.e_cache.get().len() < amount {
            let mut cache: [u8; 8192] = [0; 8192];
            self.trng.get().fill_bytes(&mut cache);
            self.e_cache.get().extend_from_slice(&cache);
        }
    }
    pub(crate) fn get_u8(&self) -> u8 {
        self.ensure_entropy(1);
        self.e_cache.get().pop().unwrap()
    }
    pub(crate) fn get_u32(&self) -> u32 {
        self.ensure_entropy(4);
        let ret = u32::from_le_bytes(self.e_cache.get()[self.e_cache.get().len() - 4..].try_into().unwrap());
        self.e_cache.get().truncate(self.e_cache.get().len() - 4);
        ret
    }
    pub(crate) fn get_u64(&self) -> u64 {
        self.ensure_entropy(8);
        let ret = u64::from_le_bytes(self.e_cache.get()[self.e_cache.get().len() - 8..].try_into().unwrap());
        self.e_cache.get().truncate(self.e_cache.get().len() - 8);
        ret
    }
    pub(crate) fn get_slice(&self, bucket: &mut [u8]) {
        self.ensure_entropy(bucket.len());
        for (src, dst) in self.e_cache.get().drain(
            (self.e_cache.get().len() - bucket.len())..
        ).zip(bucket.iter_mut()) {
            *dst = src;
        }
    }
    /// generates a 96-bit nonce using the TRNG
    pub(crate) fn get_nonce(&self) -> [u8; size_of::<Nonce>()] {
        let mut nonce: [u8; size_of::<Nonce>()] = [0; size_of::<Nonce>()];
        self.get_slice(&mut nonce);
        nonce
    }
}