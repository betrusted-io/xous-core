//! Hash functions used in the SLH-DSA signature scheme
//!
//! Each parameter set defines several functions derived from the core hash function (SHA2 or SHAKE)
//! A `HashSuite` contains all of these functions, defined in FIPS-205 section 10
mod sha2;
mod shake;

use core::fmt::Debug;

use hybrid_array::{Array, ArraySize};

pub use sha2::*;
pub use shake::*;

use crate::{PkSeed, SkPrf, SkSeed, address::Address};

/// A trait specifying the hash functions described in FIPS-205 section 10
pub(crate) trait HashSuite: Sized + Clone + Debug {
    type N: ArraySize + Debug + Clone + PartialEq + Eq;
    type M: ArraySize + Debug + Clone + PartialEq + Eq;

    /// Instantiates the hash suite.
    fn new_from_pk_seed(pk_seed: &PkSeed<Self::N>) -> Self;

    /// Pseudorandom function that generates the randomizer for the randomized hashing of the message to be signed.
    fn prf_msg(
        sk_prf: &SkPrf<Self::N>,
        opt_rand: &Array<u8, Self::N>,
        msg: &[&[impl AsRef<[u8]>]],
    ) -> Array<u8, Self::N>;

    /// Hashes a message using a given randomizer
    fn h_msg(
        rand: &Array<u8, Self::N>,
        pk_seed: &PkSeed<Self::N>,
        pk_root: &Array<u8, Self::N>,
        msg: &[&[impl AsRef<[u8]>]],
    ) -> Array<u8, Self::M>;

    /// PRF that is used to generate the secret values in WOTS+ and FORS private keys.
    fn prf_sk(&self, sk_seed: &SkSeed<Self::N>, adrs: &impl Address) -> Array<u8, Self::N>;

    /// A hash function that maps an L*N-byte string to an N-byte string. Used for the chain function in WOTS+.
    /// Message length must be a multiple of `N`. Panics otherwise.
    fn t<L: ArraySize>(
        &self,
        adrs: &impl Address,
        m: &Array<Array<u8, Self::N>, L>,
    ) -> Array<u8, Self::N>;

    /// Specialization of `t` for 2*chunk messages. Used to compute Merkle tree nodes.
    /// May be reimplemented for better performance.
    fn h(
        &self,
        adrs: &impl Address,
        m1: &Array<u8, Self::N>,
        m2: &Array<u8, Self::N>,
    ) -> Array<u8, Self::N>;

    /// Hash function that takes an N-byte input to an N-byte output
    /// Used for the WOTS+ chain function
    fn f(&self, adrs: &impl Address, m: &Array<u8, Self::N>) -> Array<u8, Self::N>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;
    fn prf_msg<H: HashSuite>(expected: &[u8]) {
        let sk_prf = SkPrf(Array::<u8, H::N>::from_fn(|_| 0));
        let opt_rand = Array::<u8, H::N>::from_fn(|_| 1);
        let msg = [2u8; 32];

        let result = H::prf_msg(&sk_prf, &opt_rand, &[&[msg]]);

        assert_eq!(result.as_slice(), expected);
    }

    fn h_msg<H: HashSuite>(expected: &[u8]) {
        let rand = Array::<u8, H::N>::from_fn(|_| 0);
        let pk_seed = PkSeed(Array::<u8, H::N>::from_fn(|_| 1));
        let pk_root = Array::<u8, H::N>::from_fn(|_| 2);
        let msg = [3u8; 32];

        let result = H::h_msg(&rand, &pk_seed, &pk_root, &[&[msg]]);

        assert_eq!(result.as_slice(), expected);
    }

    #[test]
    fn prf_msg_shake128f() {
        prf_msg::<Shake128f>(&hex!("bc5c062307df0a41aeeae19ad655f7b2"));
    }

    #[test]
    fn prf_msg_sha2_128_f() {
        prf_msg::<Sha2_128f>(&hex!("6a4b5cf23911d4f3a6591d7003445316"));
    }

    // Exercises the mgf1_sha256 function
    #[test]
    fn h_msg_sha2_128_f() {
        h_msg::<Sha2_128f>(&hex!(
            "56658221f675d907a309255e8faef639d11e6a1118fa05d3bbd26179a7e0a54a7f5b"
        ));
    }

    // Exercises the mgf1_sha512 function
    #[test]
    fn h_msg_sha2_256_f() {
        h_msg::<Sha2_256f>(&hex!(
            "8c86dfb66392d1b647df0deab90be68fb6f988513e84d3ef75fa68591122bb5d74f6413672db5164e56492b7ca2c2e0335"
        ));
    }
}
