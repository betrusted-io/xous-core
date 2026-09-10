use alloc::vec::Vec;
use core::convert::TryFrom;
use digest::Digest;
use sha2_bao1x::Sha256;

/// Iteratively performs ShiftXOR function as described in the SUANT paper.
pub struct ShiftXor<const N: usize> {
    seed: [u8; N],
    key_block: [u8; N],
    pending: Vec<u8>,
    counter: u32,
}

impl<const N: usize> ShiftXor<N> {
    /// Derived size of shift parameter. Unlike in the SUANT paper, we round up to the next byte
    /// boundary when pulling bytes from the extraction function to avoid shifting bits within
    /// bytes.
    const SHIFT_BITS: usize = 7;
    const SHIFT_BYTES: usize = (Self::SHIFT_BITS + 7) / 8;

    pub fn new(seed: &[u8], key_block: &[u8]) -> Self {
        ShiftXor {
            seed: <[u8; N]>::try_from(seed).expect("Invalid seed length!"),
            key_block: <[u8; N]>::try_from(key_block).expect("Invalid key length!"),
            pending: Vec::with_capacity(32), // capacity: size of hash output
            counter: 0,
        }
    }

    fn get_shift(&mut self) -> usize {
        if self.pending.len() >= Self::SHIFT_BYTES {
            // Decode shift from the prefix pending bytes (little-endian).
            let mut shift: u32 = 0;
            for &b in self.pending[..Self::SHIFT_BYTES].iter().rev() {
                shift <<= 8;
                shift |= b as u32;
            }
            // Note: VecDeque would avoid copies here but pending is always pretty small, so it's
            // not a huge deal.
            let tail = self.pending.split_off(Self::SHIFT_BYTES);
            self.pending = tail;
            shift as usize % (1 << Self::SHIFT_BITS)
        } else {
            // Load more bytes and then try again.
            let mut h = Sha256::new();
            h.update(self.seed);
            h.update(self.counter.to_le_bytes());
            self.counter += 1;
            self.pending.extend_from_slice(&h.finalize());
            self.get_shift()
        }
    }

    fn absorb_chunk(&mut self, ciphertext: &[u8; N]) {
        // XOR the key block with a cyclic shift of the ciphertext.
        let shift = self.get_shift();
        for i in 0..ciphertext.len() {
            let ct_lower_idx = ((shift / 8) + i) % ciphertext.len();
            let ct_upper_idx = ((shift / 8) + i + 1) % ciphertext.len();
            let ct_lower = ciphertext[ct_lower_idx] >> (shift % 8);
            let ct_upper = ciphertext[ct_upper_idx] & ((1 << (shift % 8)) - 1);
            let ct = if shift % 8 == 0 { ct_lower } else { ct_lower | (ct_upper << (8 - (shift % 8))) };
            self.key_block[i] ^= ct;
        }
    }

    pub fn absorb(&mut self, ciphertext: &[u8]) {
        let (chunks, remainder) = ciphertext.as_chunks::<N>();
        if remainder.len() != 0 {
            panic!("Invalid ciphertext length: {:?}", ciphertext.len());
        }
        for chunk in chunks {
            self.absorb_chunk(chunk);
        }
    }

    pub fn key(&self) -> &[u8] {
        &self.key_block
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        let seed: [u8; 16] = [0xff; 16];
        let key_block: [u8; 16] =
            [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        let shifter = ShiftXor::<16>::new(&seed, &key_block);
        assert_eq!(shifter.key(), key_block);
    }

    #[test]
    fn test_basic() {
        let seed: [u8; 16] =
            [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        let key_block: [u8; 16] =
            [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        let mut shifter = ShiftXor::<16>::new(&seed, &key_block);
        shifter.absorb(&seed);
        shifter.absorb(&seed);
        assert_eq!(
            shifter.key(),
            [0xff, 0x89, 0x22, 0xb8, 0x55, 0xab, 0x00, 0xda, 0xbb, 0xcd, 0x66, 0xfc, 0x11, 0xef, 0x44, 0x9e]
        );
    }

    #[test]
    fn test_symmetric() {
        let seed: [u8; 16] =
            [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f];
        let key_block: [u8; 16] =
            [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        let mut shifter = ShiftXor::<16>::new(&seed, &key_block);
        shifter.absorb(&seed);
        shifter.absorb(&seed);
        let mut reshift = ShiftXor::<16>::new(&seed, shifter.key());
        reshift.absorb(&seed);
        reshift.absorb(&seed);
        assert_eq!(reshift.key(), key_block);
    }
}
