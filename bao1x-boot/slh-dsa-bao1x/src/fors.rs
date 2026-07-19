use crate::{SkSeed, address, hypertree::HypertreeParams, util::base_2b};
use core::fmt::Debug;
use hybrid_array::{Array, ArraySize};
use typenum::Unsigned;

#[derive(Clone, Debug)]
pub(crate) struct ForsMTSig<P: ForsParams> {
    sk: Array<u8, P::N>,
    auth: Array<Array<u8, P::N>, P::A>,
}

impl<P: ForsParams> PartialEq for ForsMTSig<P> {
    fn eq(&self, other: &Self) -> bool {
        self.sk == other.sk && self.auth == other.auth
    }
}

impl<P: ForsParams> Eq for ForsMTSig<P> {}

impl<P: ForsParams> ForsMTSig<P> {
    const SIZE: usize = P::N::USIZE + P::A::USIZE * P::N::USIZE;

    fn write_to(&self, slice: &mut [u8]) {
        debug_assert!(
            slice.len() == Self::SIZE,
            "Writing FORS MT sig to slice of incorrect length"
        );

        slice
            .chunks_exact_mut(P::N::USIZE)
            .enumerate()
            .for_each(|(i, c)| {
                if i == 0 {
                    c.copy_from_slice(&self.sk);
                } else {
                    c.copy_from_slice(&self.auth[i - 1]);
                }
            });
    }
}

impl<P: ForsParams> Default for ForsMTSig<P> {
    fn default() -> Self {
        Self {
            sk: Array::default(),
            auth: Array::default(),
        }
    }
}

impl<P: ForsParams> TryFrom<&[u8]> for ForsMTSig<P> {
    // TODO - real error type
    type Error = ();
    fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
        if slice.len() != ForsMTSig::<P>::SIZE {
            return Err(());
        }
        #[allow(deprecated)]
        let sk = Array::clone_from_slice(&slice[..P::N::USIZE]);
        let mut auth: Array<Array<u8, P::N>, P::A> = Array::default();
        for i in 0..P::A::USIZE {
            auth[i].copy_from_slice(
                &slice[P::N::USIZE + i * P::N::USIZE..P::N::USIZE + (i + 1) * P::N::USIZE],
            );
        }
        Ok(Self { sk, auth })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ForsSignature<P: ForsParams>(Array<ForsMTSig<P>, P::K>);

impl<P: ForsParams> PartialEq for ForsSignature<P> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<P: ForsParams> Eq for ForsSignature<P> {}

impl<P: ForsParams> TryFrom<&[u8]> for ForsSignature<P> {
    // TODO - real error type
    type Error = ();
    fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
        if slice.len() != Self::SIZE {
            return Err(());
        }
        Ok(Self(
            slice
                .chunks(ForsMTSig::<P>::SIZE)
                .map(|c| c.try_into().unwrap())
                .collect(),
        ))
    }
}

impl<P: ForsParams> Default for ForsSignature<P> {
    fn default() -> Self {
        Self(Array::default())
    }
}

impl<P: ForsParams> ForsSignature<P> {
    pub(crate) const SIZE: usize = P::K::USIZE * (P::A::USIZE + 1) * P::N::USIZE;

    pub(crate) fn write_to(&self, slice: &mut [u8]) {
        debug_assert!(
            slice.len() == Self::SIZE,
            "Writing FORS sig to slice of incorrect length"
        );

        slice
            .chunks_exact_mut(ForsMTSig::<P>::SIZE)
            .enumerate()
            .for_each(|(i, c)| self.0[i].write_to(c));
    }

    #[cfg(feature = "alloc")]
    pub(crate) fn to_vec(&self) -> alloc::vec::Vec<u8> {
        let mut v = alloc::vec![0u8; Self::SIZE];
        self.write_to(&mut v);
        v
    }
}

pub(crate) trait ForsParams: HypertreeParams {
    type K: ArraySize + Eq + Debug;
    type A: ArraySize + Eq + Debug;
    type MD: ArraySize; // ceil(K*A/8)

    fn fors_sk_gen(
        &self,
        sk_seed: &SkSeed<Self::N>,
        adrs: &address::ForsTree,
        idx: u32,
    ) -> Array<u8, Self::N> {
        let mut adrs = adrs.prf_adrs();
        adrs.tree_index.set(idx);
        self.prf_sk(sk_seed, &adrs)
    }

    fn fors_node(
        &self,
        sk_seed: &SkSeed<Self::N>,
        i: u32,
        z: u32,
        adrs: &address::ForsTree,
    ) -> Array<u8, Self::N> {
        debug_assert!(z <= Self::A::U32);
        debug_assert!(i < (Self::K::U32 << (Self::A::U32 - z)));
        let mut adrs = adrs.clone(); // TODO: do we really need clone or should we take mut ref?
        if z == 0 {
            let sk = self.fors_sk_gen(sk_seed, &adrs, i);
            adrs.tree_height.set(0);
            adrs.tree_index.set(i);
            self.f(&adrs, &sk)
        } else {
            let lnode = self.fors_node(sk_seed, 2 * i, z - 1, &adrs);
            let rnode = self.fors_node(sk_seed, 2 * i + 1, z - 1, &adrs);
            adrs.tree_height.set(z);
            adrs.tree_index.set(i);
            self.h(&adrs, &lnode, &rnode)
        }
    }

    fn fors_sign(
        &self,
        md: &Array<u8, Self::MD>,
        sk_seed: &SkSeed<Self::N>,
        adrs: &address::ForsTree,
    ) -> ForsSignature<Self> {
        let mut sig = ForsSignature::<Self>::default();
        let indices = base_2b::<Self::K, Self::A>(md);
        for i in 0..Self::K::U32 {
            sig.0[i as usize].sk = self.fors_sk_gen(
                sk_seed,
                adrs,
                (i << Self::A::U32) + u32::from(indices[i as usize]),
            );
            for j in 0..Self::A::U32 {
                let s = (indices[i as usize] >> j) ^ 1;
                sig.0[i as usize].auth[j as usize] =
                    self.fors_node(sk_seed, (i << (Self::A::U32 - j)) + u32::from(s), j, adrs);
            }
        }
        sig
    }

    fn fors_pk_from_sig(
        &self,
        sig: &ForsSignature<Self>,
        md: &Array<u8, Self::MD>,
        adrs: &address::ForsTree,
    ) -> Array<u8, Self::N> {
        let mut adrs = adrs.clone();
        let indices = base_2b::<Self::K, Self::A>(md);
        let mut roots = Array::<Array<u8, Self::N>, Self::K>::default();
        for i in 0..Self::K::U32 {
            let sk = &sig.0[i as usize].sk;
            adrs.tree_height.set(0);
            adrs.tree_index
                .set((i << Self::A::U32) + u32::from(indices[i as usize]));
            let mut node = self.f(&adrs, sk);
            for j in 0..Self::A::U32 {
                adrs.tree_height.set(j + 1);
                adrs.tree_index.set(adrs.tree_index.get() >> 1);
                if (indices[i as usize] >> j) & 1 == 0 {
                    node = self.h(&adrs, &node, &sig.0[i as usize].auth[j as usize]);
                } else {
                    node = self.h(&adrs, &sig.0[i as usize].auth[j as usize], &node);
                }
            }
            roots[i as usize] = node;
        }
        self.t(&adrs.fors_roots(), &roots)
    }
}

#[cfg(test)]
mod tests {
    use self::address::ForsTree;
    use super::*;
    use crate::{PkSeed, Shake128f, util::macros::test_parameter_sets};
    use rand::RngExt;

    #[test]
    #[cfg(feature = "alloc")]
    #[allow(clippy::too_many_lines)] // KAT is long
    fn fors_sign_kat() {
        use hex_literal::hex;

        let sk_seed = SkSeed(Array([1; 16]));
        let pk_seed = PkSeed(Array([2; 16]));
        let fors = Shake128f::new_from_pk_seed(&pk_seed);
        let adrs = ForsTree::new(3, 5);
        let md = Array([3; 25]);
        let sig = fors.fors_sign(&md, &sk_seed, &&adrs);

        let expected = hex!(
            "2cac88fad4eeae791048fe07aa3544a9
        ab0db7949e4abe2d767811bce716bc00
        8b512f3dc7992fe8d5fe70c0f822f65b
        c3c8f23ec667ac82a899d62267431e59
        57d9471da7421fc353f3a4e3117e5b82
        6dbe311ae7149847237fcb470e4ca87a
        d9a1aac408b8b5e3083abdabbd7b4383
        5cab0d526d48edb9394d2de1e336f032
        c927304c7d98006b391a246c026b4db4
        a7f9a4b9e23098d7979fa9e12596e91e
        d2e211744d165a6fa345a46f75466d7f
        9cf4246210a029514bacff2c5a7e388e
        d9367fb58b5e0822c3d626763ab28448
        7c0ce3e00ef878da1eb86e79a644adb9
        a9594b1965a681ab3808b7449ccc3fad
        92c18b2dcf30f6039ba6bc905c0120c0
        007ee543f98332209796725f60c5215a
        9e22bbc28bc53a9ed7e80cd2b1a749ca
        15b17e02f21a655154fd0e3766728432
        08e41bfcb86d13453a9acf9fa4fab1f9
        f0bbca7e8061a902626d4cf67daa1efd
        c250a680f1f7c73edd342e306fd5b6c5
        83c863db88fc2ad9aa7cab71df150c80
        8ee52368c247c00cdb6cd172148f621b
        05fb4c57760821ec6e0ff89b34a48d50
        70e9e31aace4ea4b3baab68d6fd87426
        29f156fd3f24cd7af90bd16bb7925ff6
        ae35d668a5fa16a6dccf53bb6a1fd84f
        43c6e5553c24889437ce33d1eaadcc7d
        d1a5725d47e1373d7b630afc3e6a4881
        f5884498333213de34b874f9e7156abe
        24df0d49ac1b47061682b1206adf3a90
        b7b187467375aa88e31b1998d2ca9ffb
        4f5b403cfa4986f058385d355d057049
        c1e39cba529da7ac0564d1042e7e19b9
        1e45b9d93dcd7b47fe320e86065340aa
        02e982b098c7d4de76d35f90b49bc769
        f2fed7692d65bedd7e7faed34ac0d2b6
        6e6cad8acb9c21403b6f188759e1624d
        7f3acc1475dc67b10120a9cdb61e5066
        ae48c47623acb8a22a1c448ae0a7526e
        8640c4c3c5fc39b102dcd5bf96e14a0c
        f92209e13e7de627d1dbc35efeec0adb
        0bededc8ce1e04726336114f8193fc22
        ed7c3fa25c57d2739e61c013a701e1f2
        6b84e638d4a162a952da631b83ec82bd
        5c117842a1f3c90e4bd098c201ad01fa
        4826bb3f8f677f5a28bfb1341ae73830
        c2bde99049a98dd2e72203fe129ebb74
        df772dd23af9b65509ad64c9fe570c37
        a2dec3937d65a8742d15eb43232ade15
        770abf59f1d58dfb9e162288cb5704e5
        75917584d91861d4d05c72802d8b0d20
        d60de536a559ec19863cf3df994f7f57
        7e780d48fdc539505cf9cbe7d3366a98
        b19a3b4ff7c8ac873ba0fc7d8b62eb97
        dedc2b8fde1cd48769393c814858e26c
        5154a7a68f8fe04e89d51f2e9ea558ad
        893a93cf89c25e10560ecb52a1824a69
        50d9681113386ca46256d2f315196e49
        1d7fddc302f2a6b13d209d01f1a931d7
        3db8c7e526a3adb66374d421d2856bac
        4ae2273f5dbbfb41730e094037116e8c
        2c2142a2e99000a8651223be1d809f86
        4dba1df1a351bd141e3823623b2ef412
        67c78b83d8348909e655ac0a6d7de7bc
        6869592c0c8098fdb0582d8d3f7b4b8d
        0cff1364c27ed916cc605ba0756c7de9
        547aeb18856b1a7ad47ab4216e652240
        64b781cdf331ec8b90069b8ca8957119
        42ed015d94563e15cd15269691b2f5da
        daa4de5fb65b446f56cdb15a56097a7e
        c21d2b8a383a5d57c9212d97ff49bda9
        e9fd7e4ac97a63a5d766b23951c46ae1
        17ff035a8a01b6b13d552929030a39c9
        3a6ce3514e849b9846d7cb50477d2f50
        c75defe9cf2e581aa6472c2d5091b174
        ea125546262026f88b809e883df0ca55
        5542ccbf45463888eb69c4ae776d223f
        bc4aa9a3226a6902e08879e8f26cc5cc
        3c10e957b8b9df1d0f63ab2302d3848f
        a279887737582214fbaf2ca8c6d4db9a
        2dccbbf77436fc1c92094cc95f829c7d
        01537cf3e050db557f3af9f066629256
        6f866cf423ed7b3d319451f1c3a149fc
        dab3d0af11df4cea9f03091a50724d7e
        0828d959f75ee7a8dbd77173e0a8d960
        1d68359917b8322f1404d2df90bbbdff
        30cf95015991d269d8750342f87a9d12
        8f71c1e2f59c4195c88568c33b2d9e31
        01c3d2eb8333dba37032ad1cecf93e1d
        9549eadcc51402b6facc0e9dc49eb319
        dd219e310d8685aad7eafa9eeb3c6a9f
        f8b0d92dd69ee21ae5ccba033aed106d
        353b3cf6f3ebc571bdd52d3564fae36f
        b72beb2c1e5560c10fc63bae4fd899e2
        477fc22f24f840bf707836ffc7555330
        a9d598529222e0a5236cd98a3fd7fe71
        397cfd5aadf18308e26952723ecf68fa
        fef58ab686e103099877e3dcaf54017c
        3845037737dddc5cee97be961fc143c4
        6c174debbe5dd50b54b78a6c3ed296e7
        ba807221df6f4edbcede1e112a8532ad
        4152a0451ee1ad5119f0b64febc9f708
        18fa91d3c20487846f6a4e0b6fdb58cd
        8c898b4674f89f58d340147da10af553
        4cc0a257f11ba2c56d5df3caf98dfe60
        1bfbe5db517a4962ad1b5aef4e35fc09
        504564da0bcadff2a978c6bc5771db63
        afce16ca77afea52025207222936de1a
        4301b9c22cb7a82faae7e6de51c2a964
        ef6c5cd90f387438ada33d83d1298df5
        9fc6655b89a44eeaa2193415d9a74288
        e1b938f7f8b3e3ff7b6cbd4dcaa4f94b
        dff08cc1d146be2b1b98288daee9465b
        9a559e0f45b3688a3e15d608625a605a
        b61c1ed68830af15af0a420a892b1a7d
        931398d38c693a682b831dcb2bd597cc
        6c688f6f8e1fef3af478e787b3fdcc97
        3bfaec54bf35f95885f9dcbd4c2dec0e
        77685949e2e719f9efdcf87f68d53cc4
        08a18cae49765c7e069db3589f8a40ef
        d8079a2ce3d3f642ae810798ef005f16
        4bc49a460f489fb6636de626cc9e15d9
        bac5b681a5778fcc47992067370685e2
        fe13fad1524b074ecc0c22b538f6a4dc
        cc04e74bfeb8555e2ca70668e795891f
        2f29e90ee399860bbd304e4adc4b0fb9
        06903cf76c14eae445f1264e9d02c9fd
        f8f136891a2edd673fd618fa9087cda3
        ee848ec664db24040ee9984b87b32f17
        426f874a1df9caf48e56189e7c77c5dc
        c7d67ae43ad8981090d194e7296ebe7f
        a4d2079e9459b3c94a9aea25417ea56c
        6b534a33f522e8a84dd72d36775098b0
        197bc35de831d4dd2e1b285ab3dc48f7
        0e093b8e8371d163b4caa433b02300fa
        5c2e13151db1c007639f7fab7daddc7d
        61cecc434d98da5cd935bff6d6ff0249
        de2499f600247f45380421b8bca738f7
        706b3ac2eb72c0f063ec2fa49b4cd3ee
        a81d78c05097af13e0627624f05b2a8e
        e35c1aee48d793d71376e520035a9adf
        3b4da3e5589fa9feb181e0760e42bebc
        cc732d75278c8e3db0204ac4286dee76
        831debc5c747f739ce9ba8033c88395e
        c5c545f84e56b859af1e8ec8bed15ef9
        5376dfd94080277f9a46c57be0d8dc95
        d8c081215984612108ce867d660219d8
        2af26fc92ea0612984d54e2e9919ae21
        f9e707447b568fe377805ae910971300
        66735a71ffb3d2ee302a8238af655f78
        312a3114429d9229b70c0ca8a6b3610e
        fdc3e255becaea51e210b3a953164461
        c2989ad008df28ee01894c6d004f3aff
        8330a0705e6dc310c114b507637df65b
        a9536eee7333a0aec3d066210876f18c
        68337e1c01557778babb9c42a3c22750
        062eb3d85f0b483d0df6fd1257ee50c5
        1410c61bb736de6c5b299af5e1adea19
        4a90b0df2141d4f09825ab4fb9b0677a
        81e0082b272d1d285de6f75d71be139e
        4c2600dea5382f25d6d31d2948fa2fb7
        b249f2c9d7d878b7d4fb83248ed9bf1d
        115f60790e04878b7aa6a06fc4777f61
        73167d6670f528654155181c837e1f3c
        e7a8878da886b2272edfecb480b01140
        0d21e113516fb7e5ed586777c8b869b0
        daf770a8ee057a4ea9ec0c3e6173b569
        d4a9584d94154dd2dd398871e96fe740
        86565ad3de7f4082ac854cca03db02e1
        743a60610d0e9b280b4cf07324609d8a
        60696152e8a43e8d54dde2e4fa8520a2
        dfb1ad52e52b02d3a59f7efe0002fadb
        13cc4459a83adc9c95c7c629cffc4969
        0e31e939838b10b099e6805e5f836afe
        87ddc555a60a170e420c4689602688e1
        bfdaf2acc8b8b7b5a2baa7fea26cee53
        a8234624f3bfa46b42de7e88b55be6b2
        70673e514506dae3a3315721e12fcb6c
        9351840dc9190cc10a33b12a2c8458e2
        c7d604940278c225f4be63fb0aaa2b04
        4cca2caa877f616ebc6c7c3d0925d8fc
        fa85ff9d7bb34cb583f67c2491404e36
        0b8d44ba7bd5781212f14508b201ddec
        32f782a66f99524dc671dd9d6504b515
        7e05a9868de15601bac2adf0dd11a098
        641d2eb7e5e4bebcd0c0ac5bcd677ea2
        68ab2e99bd60342400b5c212f627972c
        be7131f09d96688254758bb15e0866ae
        f3cf49d71e271bb5013e9678a2fe6b56
        dffc42f24fc8d3ffe2181c9396f84984
        be88f3fe6cb54876daa601e44294a340
        c52b64cd7e85b7a928639521f69ed9f6
        2b2697d1a102f1f2dbd8ed8ac5f41c66
        206291fc7ded397c707edd225023323c
        c0065ea2d842feb8422e2c4cc91c7e0d
        eb69e80cf3347a75585413fe33f6ec7b
        e06ad09a68cc238b48cd58b8d4c90028
        1b8cfa79d2afabeada78d80ff1c67844
        ad4277bf118864649a3ec6b28ae522ff
        d7533f98a5502b523d369a8a6a02de6f
        0840f44620f206a48b6ebd0120e8e7f3
        11e3b61f09a57f499c6931d475021bb9
        637ce37c0e657720e2fc2c04595a3607
        25575c7f9e90eb41dee5254476d1fba1
        1f3c19b4ade31e888e919d3f30030f3a
        19605154fa89d14f5fe7d1c787aac421
        f5c3ddd3b69f0a335c5605ab674f7053
        05b0a5bbebf4202cc5023b47b21eef68
        15e0f6b08d2b2da46323938729bdd55f
        5341ff14d36df69803229c6046b2503a
        62d4e47da0195d0028cbfe0a67bf5230
        b4b8a8d2c1c31203ceac2873a860c235
        5e5f859a2be634f323dc30efe4ce3ffa
        7a61e0a32ca8d3a4cc611bee7c0f08ae
        0e7c17c2a9c8e42d5dc22e315b39800c
        435391233c937ed90125ffc463573afb
        4c15b452d653e03de6b13497a3e6e275
        7e7f218b4b8c0430d9a27c26997b092f
        20985c4af70d1cf2fd0d42d86732d807
        11fb25ce2e0a2a1ae179073350832cf4
        495ec887a77739598aabd4f3caf2a2c7
        0b70631e0fe67e3ee4aa7843c41c2f25
        3bc696d6b859844e28f9a416314a1d74
        0cb5e757000ff1ca5422dafc98a3cd85
        6ee679d11f9d4fe0ff7f1e6e9217f53e
        74bacc9f8098d1c7e54e9ed32f69f06e
        2b5a345df79c4d3864078ec89befb558
        5cdc3d79e51bd5fe70896089ea2af20e
        99b6ca2ce814190f602542a1bfa738ea"
        );

        assert_eq!(sig.to_vec(), expected);
    }

    fn test_sign_verify<Fors: ForsParams>() {
        // Generate random sk_seed, pk_seed, message, index, address
        let mut rng = rand::rng();

        let sk_seed = SkSeed::new(&mut rng);
        let pk_seed = PkSeed::new(&mut rng);
        let fors = Fors::new_from_pk_seed(&pk_seed);

        let mut msg = Array::<u8, Fors::MD>::default();
        rng.fill(msg.as_mut_slice());

        let idx_tree = rng.random_range(
            0..=(1u64
                .checked_shl(Fors::H::U32 - Fors::HPrime::U32)
                .unwrap_or(0)
                .wrapping_sub(1)),
        );
        let idx_leaf = rng.random_range(0..(1 << (Fors::HPrime::USIZE)));

        let mut adrs = ForsTree::new(idx_tree, idx_leaf);
        let mut pks = Array::<Array<u8, Fors::N>, Fors::K>::default();
        for i in 0..Fors::K::U32 {
            adrs.tree_index.set(i);
            pks[i as usize] = fors.fors_node(&sk_seed, i, Fors::A::U32, &adrs);
        }
        let pk = fors.t(&adrs.fors_roots(), &pks);

        let sig = fors.fors_sign(&msg, &sk_seed, &adrs);
        let pk_recovered = fors.fors_pk_from_sig(&sig, &msg, &adrs);
        assert_eq!(pk, pk_recovered);
    }

    test_parameter_sets!(test_sign_verify);

    fn test_sign_verify_failure<Fors: ForsParams>() {
        // Generate random sk_seed, pk_seed, message, index, address
        let mut rng = rand::rng();

        let sk_seed = SkSeed::new(&mut rng);
        let pk_seed = PkSeed::new(&mut rng);
        let fors = Fors::new_from_pk_seed(&pk_seed);

        let mut msg = Array::<u8, Fors::MD>::default();
        rng.fill(msg.as_mut_slice());

        let idx_tree = rng.random_range(
            0..=(1u64
                .checked_shl(Fors::H::U32 - Fors::HPrime::U32)
                .unwrap_or(0)
                .wrapping_sub(1)),
        );
        let idx_leaf = rng.random_range(0..(1 << (Fors::HPrime::USIZE)));

        let mut adrs = ForsTree::new(idx_tree, idx_leaf);
        let mut pks = Array::<Array<u8, Fors::N>, Fors::K>::default();
        for i in 0..Fors::K::U32 {
            adrs.tree_index.set(i);
            pks[i as usize] = fors.fors_node(&sk_seed, i, Fors::A::U32, &adrs);
        }
        let pk = fors.t(&adrs.fors_roots(), &pks);

        let sig = fors.fors_sign(&msg, &sk_seed, &adrs);

        // Modify the message
        msg[0] ^= 0xff; // Invert the first byte of the message

        let pk_recovered = fors.fors_pk_from_sig(&sig, &msg, &adrs);
        assert_ne!(
            pk, pk_recovered,
            "Signature verification should fail with a modified message"
        );
    }

    test_parameter_sets!(test_sign_verify_failure);
}
