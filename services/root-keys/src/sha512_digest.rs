use digest::{
    consts::{U64, U128},
    BlockInput,
    FixedOutputDirty,
    Reset,
    Update,
};

/// Wrap a pre-hash value in a Digest trait
#[derive(Clone)]
pub struct Sha512Prehash {
    /// track the length of the message processed so far
    hash: Option<[u8; 64]>
}
impl Sha512Prehash {
    // use this function instead of default for more control over configuration of the hardware engine
    pub fn new() -> Self {
        Sha512Prehash {
            hash: None
        }
    }
    pub fn set_prehash(&mut self, hash: [u8; 64]) {
        self.hash = Some(hash);
    }
}
impl Default for Sha512Prehash {
    fn default() -> Self {
        Sha512Prehash::new()
    }
}

impl BlockInput for Sha512Prehash {
    type BlockSize = U128;
}

impl Update for Sha512Prehash {
    fn update(&mut self, _input: impl AsRef<[u8]>) {
        panic!("Prehash implementation cannot process any new data");
    }
}

impl FixedOutputDirty for Sha512Prehash {
    type OutputSize = U64;

    fn finalize_into_dirty(&mut self, out: &mut digest::Output<Self>) {
        if let Some(hash) = self.hash {
            for (dest, &src) in out.chunks_exact_mut(1).zip(hash.iter()) {
                dest.copy_from_slice(&[src])
            }
        } else {
            panic!("Sha512Prehash object was not initialized with a pre-hash value before use!");
        }
    }
}

impl Reset for Sha512Prehash {
    fn reset(&mut self) {
    }
}