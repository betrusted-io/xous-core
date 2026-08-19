//! Independent Known-Answer Tests for SLH-DSA-SHA2-128-24 (NIST SP 800-230 ipd).
//!
//! The expected public key and signature below were produced by the
//! `sphincs/sphincsplus` C reference implementation (branch `bas/fips205`, which
//! uses big-endian FORS indexing matching FIPS-205 and this crate), NOT by this
//! crate — so a passing test cross-validates the Rust implementation against an
//! independent oracle. See `c-test-harness/`.
//!
//! Deterministic inputs (matching the C driver `slh_kat.c`):
//!   sk_seed  = 16 bytes 0x00,0x01,...,0x0f
//!   sk_prf   = 16 bytes 0x40,0x41,...,0x4f
//!   pk_seed  = 16 bytes 0x80,0x81,...,0x8f
//!   opt_rand = 16 bytes 0xC0,0xC1,...,0xcf   (FIPS-205 addrnd / R input)
//!   message  = "NIST SP 800-230 SLH-DSA vector\n"  (31 bytes), empty context
//!
//! Place this file in the crate's `tests/` directory and run `cargo test`.
//! The tree-cache variant additionally requires
//! `cargo test --release --features tree-cache`, and the hardened-verify
//! variant `--features hardened-verify`.

use slh_dsa::*;

const MSG: &[u8] = b"NIST SP 800-230 SLH-DSA vector\n";

const SHA2_128_24_PK: &str = "808182838485868788898a8b8c8d8e8fa04ad33acb292af0da32f74d3285b014";

const SHA2_128_24_SIG: &str = "eabbc67b08a221583636efddfe80483c448c6e2fc068ac375b192a982c5b70d70d1850e1e67e7121999a9c396eac3977328a1f5cbcd0e6c1eeabf925adf3b00909b7599b458621012f9b555d9053b3edcb7bf0a4a9962ba3dc1a65e8023b8d1b98d14701d9c1778b28e4fdc8c3a8f178b422e42f0170f79467f8256dcbc0ddbd96865bae84872c993401af59b5dbf6f141d6bb23d93667380e9b05342ea97d412967ad4f24b9b1f80fb7d0f62de142d4a242f264f92b6b26f5c4352caa0036c19a9819b9b82cf287ba7eb43c2dbe5bbb45cd9f68c1e511b9f55f2b8a7b56a999ada47f0ec04ee3d9d34f03a3a77fe0c4ed33a20fcffa8de28ad6aa0eee2fa84b943b82a5c5dc37a34f7f69ca16c9bca0592efe3c7912f22502c3dbe5e29ef8ca18417c49dcf2a1dc5bca70c79f48e227dcef605d3b09ada25032041d7934f16842408df3e11d84396780f9324e27d5998f34ecb060f42f626ad4bfdacdfc06f67dfe946ec3cfc52ad6e68e127422ee4a690ba04faa169d031a35e1917fb0bf571b50b1ee5891b8ad5e9215410fd390282bfa99caa7ba11e885a1569edadf7d0ffde1c229504ecc786b5e195ad7183854700cdfc9be46eeec60a3f5db7f50228e75fb64d5c04e7c3fc64ef5606801f4cbf2a8f3a17cad9803cf8795bee9426146503760bf6b6572bc13658b7c66a88e9157e11241cb4556daeaa63221448427bcfd85da7c6096fbb59bdce5ff8be3de3a8de45c8868b0850d2b9f57a064857b807246939aa91955fbd023db52d3c461f7cc9798ef066c472d0cc018eb593263dba47a39656a2d8707981cd2a5ae0dac65e9fa5d288af50fc325c098c47939c4eef6ad7f94392cff5fc2b65d930b7280c7e9b2ae593ebe2c236a7461e51b3a7d1342972534535ae7d9d378c29a80f7869bc6a9aeb10da68abdb5a77d65a8ceaccd3c5b3122ace895b37a2fb51c47589f238e44d0050b698104ca50eb8a9cbeb92d586846869101b2fcea231cea0133eb6754229e3f083fe6554e4bd08c710b0b84beeec24355f4a45a20c36df8efe1c73607101ff553f0802493fc038018b16d598802a6f4091c4d39fe94291e24a0b539c0e4cbe5e7b16a2cb084ba502314b6c87df0bb59662cc808a023475507a74ae5a6a258e4b90d4168b02747347fc7d3f26e83ff49707c87cba57256409a07ae17a652fa529c77adcf8002ac8eaafcd79414ecffaca9b47c864ce029d0cb5c1b4aa589a27b41bb81e256a5c10d7732d3ff8b49e350ebdba86c17b73a79d143ad8fa7329223467fafbb409417ffba779d36f16cd683af9cfff2329f3e2c59f5cf7694df91286dfce6ab22be1d04ad8042c8483f28266902e84e9347c616d46e2e1ec3723ecad23652de23127aa47d10cc11150408fd3d619cd336c6013e1b898b14bc3feb66b4dc6f7b6d3ea0b6f30101c3660be72780a843a2a3324bbcc5b033a60f5413dd272e0f48062cafbc6e82eb6d06d120d86886a862b70d71f6f64d6e0566160588f86c78bd2989ca737dee12cb010aac2999d5c6e611429adbb44193aa5db2ecca9c72990d5f64f0f15b83529499addc02a4c29d1e8ea726ecad2714cc52599a030cd97a244f95357d0659357393eb54af6821c72a49e268241f4a52c172c7352b4ae786ab29f7ee3e15f90fe71eda43af66f95c2fc2f9d253285042d104814281e769cef547aecb93e252c64ad23627dff0824bc24edf46adad43377fd7f4172bde3453451b946b2c6bf192fa7f17825a168d500bcf2876002900aac4d9b0399222d731849b3b3a6687e3966cc3f4f3121e9feec94f7dcf782d553271788840857f4f1989ad55b4d42ac70af7e67845550ed0c56621efa20ebbc66938fc0d4a6592e6ce676d51acdff77f1e6f566e92a3d1a3546cb31ef076441e784d86a5302b9aaaf0ee91ca320289d41fa97b1be3e8da1a932fe029cd2e8cc6351806c4b3dcf42bfc9852e0ca156f3591a5dbb4451f81ff8fc3e4a0ad85d52120c561d99b87851a46b71500e0b9f1ad048452fff195b32c600b4fe053a359b2ec086aae9d8245add1dfa3918b2c83334911eb4d8960c96ac7c4c3cbf62802939b666b2cc56c777829395259c382006bb03c842a7200f9c7c13758c08a235bd29c67a307481336c1e334a52946c06dc28299b900eec05248d61c817f56ae55217405fc1e564874a55e1f025894ec05fe010a52d2e1fd86186923377736b4a5293c767072c754d99eca6ddca8deaabae566cdf379d8ec7f0563703d451bcd2814353f1f13a57c986d45d58295f90a4da01896bf2eae872a90aa364c574a6973a28a97305fedcc08d1d308d3651e64a280dd99979b54f83e9656e1fc31c5b86d1eac3b93c5ac2fa574369d587d9ade0df7686dcc9235ad4f81ef96a7e7957eb7476c1b23b18bc39714a734dc64b0140441bd61d0a7f86e959e4508ee8901afca72656e093f636f97031a28b31442ac23aad56a27201985ed0a57fd5f014a83a8912e618f27c12cec92d9a8626038bfd8a69325e4c63560c3e9ba304da81e0fcf5eaec249d9c524beb8af2d03725b1d483cb721b38faa5c4594406ab2ac39d1cac618f5ef995c6f2dbceac781a8b5fd25e4f8cef72be82873686921670d48645f69e6e473bdf77d8d4ac3d3fef5a214c563e1cea309fd899801ce3ada57466237ac77cea3d1411aee2133ec17f1c8d7ee8cc5865e2b0e3535691f891c353e68be1676b216c47f5f38c0e96f81563a9e85745ed048fb38a3387a2b3a5b74ed5cf7813fa6696ede3f3d481e5468cc80898758e2deb0960c9c428f34e394651082a297a79bf106a6818df7b3b7fa39b2e937e0ec6457cae0bd8bed3ef2e7104cb2b6ad3f7490f9be245e2d7584a42f12e134f8dc5b09bc08e9e36c166ca9f450abdb9a65408aee137f11b08bd7863735f658eaad3d2048bbe1cbdb55c6b823ee6a664f6ee4e0efd19f8587809b1905e4a3a8e0c4624673194cc121d983d76853c1b8844c47761efcb615350f40729c6ecfa8da513fae4324f938765c9470370248d3be3c2cd6e469df894256be0b06b742cd177d70d5828d3e3fc6c52a1ac43955f061fdcad872f77efcde65fc7d781f03180d77e64f891f38ef156ec279ce13f0418793e94b4e664363acac0e30342910459ac7eb5c80fabc37ba0fabf23f8dd7273d8fd24128a03fe1cab64a8fe6093361176693ca913ff9bb692fbbf451e9ede9013bcf609bc05c0b3fe1b17faf01e5131be4878ecf5696c5bc983997143a59a65f96ed9c11889331d3eef0381bd7343de8f42888775512abebc660a8379b43ce3b99898fa0e2dc92c0ae2aca027c2a3195d53aa382c395403998f2cfb8fb9b24a85871682286e875215c9f5e4ff4dc6e2f52280bd0230d7d420023cd70bd5ca3df7dd51758a474cf230dcf171ba02f10cb69c53a2238ebad14f475f829f41f82e76ea06e1b4232314947f3201bc7254a029e5ee9204f3202666b32c6995cc8d30df0492eb37614d52f2f7f580349d1375d19f4913fbe1be2c00a749d0cfaa7d878d223a6ca7723e2ff1f8d42bad5e12cf97dd08f3e182a546f97ed5e1eb90703cb987de044c6a6c51560df50bfeda8a9d3e2945a74edd218d746573d30ce3195e582c4432169b6cedca859e2dc049fa8cfc9c456cd4b5c2394c8c14deb4443cf1304954a268dc31b2863332c626e7cf0cf74497f7fff2911644e44dc696eacf3d75fa68de2757b1674817a28ace00a4ac53815dfd95c23cd87043c7545daeca10cb7842402d1720b62455b85403c5e17d009570a4dfaaa8898e496878c1d8ad821c50ed0bc327359ed3b2570447f6e3dde4881d860920532ba63b502824b576dacdf31abf8b6cdd078147e61210def9fc16e406fccca3fffe97734da5f6a0a5661fbc1b238665b951cc8aac7b04e26eafac82317838dd480b6af761bb1302c46bce7b37b94a1b2e640db4923bd6415597d08e1108d4e388f5894f58ca5a880baa0d36a9c17247c71a129fd89665f69abfc084b7f04721f0ef32e7b560b5d0506697ea24d8be0d272f530ebabb0987fee4a71177d9a804a07472209ac3d53600cdb96c7ce2b1b865da02014c7c14c7ed3b6810b5b925ac42ba7cf14843900f9bcbf04d5c807d9861fd3a569390dc08ebfa47f4b3cbb024de9219f7a0f6b6bc804acab8accd4a123fd54c30b2e940065d6b442abd8b4d43eec0433bb51db44bec775ccdd2186fad8e041c35dbf4d47bf907380b36e785c5ff3b65cea3246c5557370301f05013ef02fcf6261aae6a4a691e8e61cc0efa7e9820bbd27f7a30c583a32d7dba13f955607c0d64a0661f8fa746754da7f0e1983e033b47044e3b81e900b1af3d0af413ffffd4f5e2a1dbc6f7e62f6dc67aa26bd4df44224305875fc561d473ba5d8bafd978a52ba8db257cb15458253bdf56c6942dcae8869ed3c923f774f43380433be45c39d1381da466da4feee5a68c229208cf548bbd35cf848f4565388e0b2795e60519d96d9e680d5ebe2f436bc6260c010ec730d41bd296eca2d4146856b9b99050cdd3fbeeccf82bdc12d52a75fff600085a331e346e2beecced292feaf278a58a1dc1082214601d699e75fda40213ecefe6865ec8fb69e7f13863b6fa2305d118830eb58a53b20e229e01b1e1e6a6f0f35b62824dbc63705f498da306fbbdaa51c459ddd59e7d0f8b5f26c02d8c56ceb6888d4385fa2923124af946c745185ae2bf32fb22191b6885ca0b0908268f52971ef97372ee49e1e82e0d0c78a8767b193b6345e8ee6ac30b1cd476ddeb8c35d1d17cce1f3ee2a9f7dc37f3762e14978aae040af53922d0d8f284c09af7c303f0a7258a591d58f0341894bf0308516bc1e02de85a7c89e0868f4a0b5f9c6ad5a7b57137251a8288a955771fd720521b90eb9a5469b87e6269d44e1aaf9446a3f81818febd45b29790684ee07d7dd047bed958f216229a4088ed291135e1c9ec3b4e342feb552cac8bc24ba55c1c18f6e0bbb76f84a94c7b09abd62b8d509300011c59c3425a173c81491cce0a03d01497a35ecc540092b62b37557eac0d35c6826c7c2c07e6568f621f31906e7df2372a1cb47204fc2c7c5aabea685d856897f5b2943309546bbf35f974b81a8cfd48934a88df6e835b031164d024c8784fa3e4061f7e9e6c52618c4d44d38774a17de1721d996c2390c6734c8167188edc8167c0887ac17a0579cd00c1c92bfab3775a243723a88572858424bb42f334070ed33ef7db365a8fc91649d615dd07661cc9d8350ab612f7693077480a25029ce7cba2fd0777d141a1670a3d59be2b05515d99b34411ee0ac4832329d352ab391bf4a770956a7df10a94888b17d384324795b0b5e12d47e935bccf0a0a707b58ae560c726ce7ff3841500b3f053d17ff6a6bcf859c27a7ee039f7c920c47bcea78393b073f5799a1bbb07dd31";

fn ramp(base: u8, n: usize) -> Vec<u8> { (0..n).map(|i| base.wrapping_add(i as u8)).collect() }

fn kat_key<P: ParameterSet>(n: usize, pk_hex: &str) -> SigningKey<P> {
    let sk = SigningKey::<P>::slh_keygen_internal(&ramp(0x00, n), &ramp(0x40, n), &ramp(0x80, n));

    // Serialized signing key is sk_seed || sk_prf || pk_seed || pk_root;
    // the public key is the trailing 2*n bytes (pk_seed || pk_root).
    let skb = sk.to_bytes();
    let pk = &skb[2 * n..4 * n];
    assert_eq!(pk, hex::decode(pk_hex).unwrap().as_slice(), "public key mismatch");
    sk
}

fn check_sig<P: ParameterSet>(sig: &Signature<P>, sig_hex: &str) {
    let sig_bytes = sig.to_bytes();
    let expected = hex::decode(sig_hex).unwrap();
    assert_eq!(sig_bytes.len(), expected.len(), "signature length mismatch");
    assert_eq!(sig_bytes.as_slice(), expected.as_slice(), "signature mismatch");
}

/// keygen from the deterministic seeds, then check the public key and the full
/// signature against the independently generated vector.
#[test]
fn kat_sha2_128_24() {
    let sk = kat_key::<Sha2_128_24>(16, SHA2_128_24_PK);
    let sig = sk.slh_sign_internal(&[MSG], Some(&ramp(0xC0, 16)));
    check_sig(&sig, SHA2_128_24_SIG);
}

/// Same vector, but signed through the XMSS tree cache: build the cache,
/// round-trip it through its serialization, validate it against the verifying
/// key, and require the cached signature to match the C reference vector
/// byte-for-byte. This pins the cached signing path to the independent oracle,
/// not merely to the crate's own uncached path.
#[cfg(feature = "tree-cache")]
#[test]
fn kat_sha2_128_24_tree_cache() {
    use slh_dsa::signature::Keypair; // verifying_key()

    let sk = kat_key::<Sha2_128_24>(16, SHA2_128_24_PK);
    let vk = sk.verifying_key();

    // Build at the default floor (h' - 12 = 10 for this set), then exercise
    // the serialization boundary the way an application would.
    let cache = sk.build_tree_cache();
    assert_eq!(cache.floor(), 10);
    let cache = XmssTreeCache::<Sha2_128_24>::from_bytes(&cache.to_bytes())
        .expect("tree cache must round-trip through its serialization");
    assert!(cache.validate(&vk), "tree cache must validate against the KAT key");

    let sig = sk.slh_sign_internal_with_cache(&[MSG], Some(&ramp(0xC0, 16)), &cache);
    check_sig(&sig, SHA2_128_24_SIG);
}

/// Verify the C-reference vector through the fault-hardened path: parse the
/// public key and signature from the vector bytes (as the boot verifier
/// would), compute the masked roots, and run the recommended per-byte
/// unmask-and-compare loop. The hardened API mirrors the internal path, so
/// the KAT vector applies directly. Also checks a tampered message mismatches.
#[cfg(feature = "hardened-verify")]
#[test]
fn kat_sha2_128_24_hardened_verify() {
    let vk_bytes = hex::decode(SHA2_128_24_PK).unwrap();
    let vk = VerifyingKey::<Sha2_128_24>::try_from(vk_bytes.as_slice()).unwrap();
    let sig_bytes = hex::decode(SHA2_128_24_SIG).unwrap();
    let sig = Signature::<Sha2_128_24>::try_from(sig_bytes.as_slice()).unwrap();

    let unmask_compare = |out: &HardenedVerifyOutput<Sha2_128_24>, mask: u32| -> bool {
        let (mr, er) = (out.masked_root(), out.expected_root());
        if mr.len() != er.len() {
            return false;
        }
        let mut matched = 0usize;
        for i in 0..mr.len() {
            let unmask = 0u8.wrapping_sub(((mask >> i) & 1) as u8);
            if (mr[i] ^ unmask) != er[i] {
                return false;
            }
            matched += 1;
        }
        matched == mr.len()
    };

    for mask in [0u32, 0xFFFF_FFFF, 0x5A5A_A5A5] {
        let out = vk.slh_verify_hardened(&[MSG], &sig, mask);
        assert!(unmask_compare(&out, mask), "C vector must verify via hardened path");
    }

    let tampered: &[u8] = b"NIST SP 800-230 SLH-DSA vector.";
    let out = vk.slh_verify_hardened(&[tampered], &sig, 0xC0FF_EE00);
    assert!(!unmask_compare(&out, 0xC0FF_EE00), "tampered message must mismatch");
}
