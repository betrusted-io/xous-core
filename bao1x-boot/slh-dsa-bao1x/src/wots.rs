use crate::{SkSeed, address, hashes::HashSuite, util::base_2b};
use core::fmt::Debug;
use hybrid_array::{Array, ArraySize};
use typenum::{Unsigned, generic_const_mappings::U};

// WOTS+ in general is parameterized on these values
// But the FIPS standard uses the same values for all parameter sets
// So we make these global consts for simplicity
const LOG_W: usize = 4;
const W: u32 = 16;
const CK_LEN: usize = 3; // Length of a checksum in chunks

#[derive(Clone, Debug)]
pub(crate) struct WotsSig<P: WotsParams>(Array<Array<u8, P::N>, P::WotsSigLen>);

impl<P: WotsParams> PartialEq for WotsSig<P> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<P: WotsParams> Eq for WotsSig<P> {}

impl<P: WotsParams> WotsSig<P> {
    pub(crate) const SIZE: usize = P::N::USIZE * P::WotsSigLen::USIZE;

    pub(crate) fn write_to(&self, buf: &mut [u8]) {
        debug_assert!(buf.len() == Self::SIZE, "WOTS+ serialize length mismatch");

        buf.chunks_exact_mut(P::N::USIZE)
            .zip(self.0.iter())
            .for_each(|(buf, sig)| buf.copy_from_slice(sig.as_slice()));
    }

    #[cfg(feature = "alloc")]
    #[cfg(test)]
    pub(crate) fn to_vec(&self) -> alloc::vec::Vec<u8> {
        let mut vec = alloc::vec![0u8; Self::SIZE];
        self.write_to(&mut vec);
        vec
    }
}

impl<P: WotsParams> TryFrom<&[u8]> for WotsSig<P> {
    type Error = ();

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != Self::SIZE {
            return Err(());
        }
        let mut sig = Array::<Array<u8, P::N>, P::WotsSigLen>::default();
        for i in 0..P::WotsSigLen::USIZE {
            sig[i].copy_from_slice(&value[i * P::N::USIZE..(i + 1) * P::N::USIZE]);
        }
        Ok(WotsSig(sig))
    }
}

pub(crate) trait WotsParams: HashSuite {
    type WotsMsgLen: ArraySize; // Number of chunks in a WOTS message. Must equal 2 * Self::N
    type WotsSigLen: ArraySize + Debug + Eq; // Number of chunks in a WOTS signature. Must equal WotsSigLen + CK_LEN;

    /// Algorithm 4
    fn wots_chain(
        &self,
        x: &Array<u8, Self::N>,
        i: u32,
        s: u32,
        adrs: &address::WotsHash,
    ) -> Array<u8, Self::N> {
        debug_assert!(i + s < 1 << LOG_W, "Invalid wots_chain index");

        let mut tmp = x.clone(); //TODO: no clone
        let mut adrs = adrs.clone(); // TODO: no clone
        for j in i..(i + s) {
            adrs.hash_adrs.set(j);
            tmp = self.f(&adrs, &tmp); // TODO: overwrite existing buffer
        }
        tmp
    }

    /// Algorithm 5
    fn wots_pk_gen(
        &self,
        sk_seed: &SkSeed<Self::N>,
        adrs: &address::WotsHash,
    ) -> Array<u8, Self::N> {
        let mut adrs = adrs.clone();
        let mut sk_adrs = adrs.prf_adrs();

        let tmp = Array::<Array<u8, Self::N>, Self::WotsSigLen>::from_fn(|i: usize| {
            let i: u32 = i.try_into().expect("i is less than 2^32");
            sk_adrs.chain_adrs.set(i);
            adrs.chain_adrs.set(i);
            let sk = self.prf_sk(sk_seed, &sk_adrs);
            self.wots_chain(&sk, 0, (1 << LOG_W) - 1, &adrs)
        });
        let pk_adrs = adrs.pk_adrs();
        self.t(&pk_adrs, &tmp)
    }

    // Algorithm 6
    fn wots_sign(
        &self,
        m: &Array<u8, Self::N>,
        sk_seed: &SkSeed<Self::N>,
        adrs: &address::WotsHash,
    ) -> WotsSig<Self> {
        let msg = base_2b::<Self::WotsMsgLen, U<LOG_W>>(m.as_slice());
        let csum = msg.iter().map(|&x| (1 << LOG_W) - 1 - x).sum::<u16>() << 4; // Algorithm 6 Line 9

        let csum_bytes = csum.to_be_bytes();
        let csum_chunks = base_2b::<U<CK_LEN>, U<LOG_W>>(&csum_bytes);
        let mut msg_csum = msg.iter().chain(csum_chunks.iter());

        let mut adrs = adrs.clone();
        let mut sk_adrs = adrs.prf_adrs();

        let sig = Array::<Array<u8, Self::N>, Self::WotsSigLen>::from_fn(|i: usize| {
            let i: u32 = i.try_into().expect("i is less than 2^32");
            sk_adrs.chain_adrs.set(i);
            adrs.chain_adrs.set(i);

            let sk = self.prf_sk(sk_seed, &sk_adrs);
            self.wots_chain(&sk, 0, u32::from(*msg_csum.next().unwrap()), &adrs)
        });

        WotsSig(sig)
    }

    fn wots_pk_from_sig(
        &self,
        sig: &WotsSig<Self>,
        m: &Array<u8, Self::N>,
        adrs: &address::WotsHash,
    ) -> Array<u8, Self::N> {
        let msg = base_2b::<Self::WotsMsgLen, U<LOG_W>>(m.as_slice());
        let csum = msg.iter().map(|&x| (1 << LOG_W) - 1 - x).sum::<u16>() << 4; // TODO: remove magic 4
        let csum_bytes = csum.to_be_bytes();
        let csum_chunks = base_2b::<U<CK_LEN>, U<LOG_W>>(&csum_bytes);
        let mut msg_csum = msg.iter().chain(csum_chunks.iter());

        let mut adrs = adrs.clone();
        let tmp = Array::<Array<u8, Self::N>, Self::WotsSigLen>::from_fn(|i: usize| {
            adrs.chain_adrs
                .set(i.try_into().expect("i is less than 2^32"));
            let msg_i = u32::from(*msg_csum.next().unwrap());
            self.wots_chain(&sig.0[i], msg_i, W - 1 - msg_i, &adrs)
        });
        self.t(&adrs.pk_adrs(), &tmp)
    }
}
#[cfg(test)]
mod tests {
    use super::WotsParams;
    use crate::{PkSeed, SkSeed, util::macros::test_parameter_sets};
    use crate::{address::WotsHash, hashes::Shake128f};
    use hex_literal::hex;
    use hybrid_array::Array;
    use rand::Rng;

    fn test_sign_verify<Wots: WotsParams>() {
        // Generate random sk_seed, pk_seed, message, address
        let mut rng = rand::rng();

        let sk_seed = SkSeed::new(&mut rng);
        let pk_seed = PkSeed::new(&mut rng);

        let mut msg = Array::<u8, _>::default();
        rng.fill_bytes(msg.as_mut_slice());

        let adrs = &WotsHash::default();
        let wots = Wots::new_from_pk_seed(&pk_seed);
        let pk = wots.wots_pk_gen(&sk_seed, adrs);
        let sig = wots.wots_sign(&msg, &sk_seed, adrs);
        let pk_recovered = wots.wots_pk_from_sig(&sig, &msg, adrs);

        assert_eq!(pk, pk_recovered);
    }

    test_parameter_sets!(test_sign_verify);

    fn test_sign_verify_fail<Wots: WotsParams>() {
        // Generate random sk_seed, pk_seed, message
        let mut rng = rand::rng();

        let sk_seed = SkSeed::new(&mut rng);
        let pk_seed = PkSeed::new(&mut rng);

        let mut msg = Array::<u8, _>::default();
        rng.fill_bytes(msg.as_mut_slice());

        let adrs = &WotsHash::default();
        let wots = Wots::new_from_pk_seed(&pk_seed);

        // Generate public key
        let pk = wots.wots_pk_gen(&sk_seed, adrs);

        // Sign the message
        let sig = wots.wots_sign(&msg, &sk_seed, adrs);

        // Tweak the message
        msg[0] ^= 0xff; // Invert the first byte of the message

        // Attempt to recover the public key from the tweaked message and signature
        let pk_recovered = wots.wots_pk_from_sig(&sig, &msg, adrs);

        // Check that the recovered public key does not match the original public key
        assert_ne!(
            pk, pk_recovered,
            "Signature verification should fail with a modified message"
        );
    }

    test_parameter_sets!(test_sign_verify_fail);

    #[test]
    fn test_pk_gen_shake128f_kat() {
        let sk_seed = SkSeed(Array([1; 16]));
        let pk_seed = PkSeed(Array([2; 16]));
        let adrs = WotsHash::default();
        let wots = Shake128f::new_from_pk_seed(&pk_seed);

        // Generated by https://github.com/mjosaarinen/slh-dsa-py
        let expected = Array(hex!("98b63dd1574484876b1f8a1120421eac"));
        let result = wots.wots_pk_gen(&sk_seed, &adrs);
        assert_eq!(result, expected);
    }

    #[test]
    #[cfg(feature = "alloc")]
    fn test_sign_shake128f_kat() {
        let sk_seed = SkSeed(Array([1; 16]));
        let pk_seed = PkSeed(Array([2; 16]));
        let adrs = &WotsHash::default();
        let msg = Array([3; 16]);
        let wots = Shake128f::new_from_pk_seed(&pk_seed);

        let expected = &hex!(
            "f7bcb9575590faae2e6a8ae33149082d2ec777cff4051f43177ef44bcbd2c18d
            a94146c50037c914461dd6ed720192b059bd2be6ed8d8cf26e4e9d68fbf9ded1
            6c334bed21677c6a3679f17a8425de40431b4317326c5d825d931b4a54a1b81f
            e7ad259086ea665109a7eca79f03e3619d99af5d0419fece8300973f29467f28
            d2b18639eeaa826488f6c785d492703463e80f8b088e64de9ca3b373cead611f
            d356bf6c22f70f98f229174a9ac815342f0439eb289a78f49f47aa8c3f272a15
            f5f0f5020b5d71981254daa9e1f01a90248935c1c67ad1cf71d9224184820cf9
            ece9b737ec986c86ba0a9431ff8485c274140bebc9d856316d49128eb075f81a
            c00d32b9f949940f2dd684a2e615e16b47093eb49e3bc9d77e69c7944d7063c6
            f8b4b5aa46fe759999fa2892ce4c7881b80f38d684427a0b77f3ad43377833d2
            d94c600b340ea408a0ad7c32c409bdb4ebaade3b1dda4ac8584acba979c845a9
            b0ddfc69ea22ffb415745b779b45d7af00ca9fde87e5d59385d7b5cedec6e30f
            3346f573f59a00af993a2ec314ed951e3a8c00f69364a82fa34d14933fe3cdb7
            bd5e5d511297695bad5cda22daea8d39f61d4ed34412acd1f5399a54953ae04b
            09828f90877ad7f01605631ace0a4e7c773cc887e2d0fa0bd3d6db811794df3a
            a8721c308482ccb511c9133311653ce8f9c2336e2980c2ab554c41bad436c0c7
            1c394d3f7eafcea2806c153113d6291a912c0e73e44197763b9ead341c298585
            bc6e16d8458fc1917ff4ac57de461ee1"
        );

        let result = wots.wots_sign(&msg, &sk_seed, adrs);
        assert_eq!(result.to_vec(), expected.as_slice());
    }
}
