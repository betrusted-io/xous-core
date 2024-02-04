use core::sync::atomic::{AtomicU32, Ordering};

use engine_25519::*;
use num_traits::*;
use xous_ipc::String;

use crate::{CommonEnv, ShellCmdApi};
static CB_ID: AtomicU32 = AtomicU32::new(0);

// these vectors come from running `cargo test field::test::make_vectors` inside
// https://github.com/betrusted-io/curve25519-dalek. The output file is originally called `test_vectors.bin`.
// a portion of this vector set involves random numbers in addition to deterministic corner cases,
// therefore, every invocation will create a slightly different set of vectors
#[export_name = "engine_vectors"]
pub static ENGINE_VECTORS: &[u8; 15652] = include_bytes!("engine25519_vectors.bin");

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum BenchOp {
    StartEngine,
    StartDh,
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum BenchResult {
    EngineDone,
    DhDone,
}

const TEST_ITERS: usize = 10;
const TEST_ITERS_DH: usize = 200;

fn vector_read(word_offset: usize) -> u32 {
    let mut bytes: [u8; 4] = [0; 4];
    for i in 0..4 {
        bytes[i] = ENGINE_VECTORS[word_offset * 4 + i];
    }
    u32::from_le_bytes(bytes)
}

fn run_vectors(engine: &mut Engine25519) -> (usize, usize) {
    let mut test_offset: usize = 0x0;
    let mut passes: usize = 0;
    let mut fails: usize = 0;
    loop {
        let magic_number = vector_read(test_offset);
        if magic_number != 0x5645_4354 {
            break;
        }
        log::debug!("test suite at 0x{:x}", test_offset);
        test_offset += 1;

        let load_addr = (vector_read(test_offset) >> 16) & 0xFFFF;
        let code_len = vector_read(test_offset) & 0xFFFF;
        test_offset += 1;
        let num_args = (vector_read(test_offset) >> 27) & 0x1F;
        let window = (vector_read(test_offset) >> 23) & 0xF;
        let num_vectors = (vector_read(test_offset) >> 0) & 0x3F_FFFF;
        test_offset += 1;

        let mut job = Job {
            id: None,
            uc_start: load_addr,
            uc_len: code_len,
            ucode: [0; 1024],
            rf: [0; RF_SIZE_IN_U32],
            window: Some(window as u8),
        };

        for i in load_addr as usize..(load_addr + code_len) as usize {
            job.ucode[i] = vector_read(test_offset);
            test_offset += 1;
        }

        test_offset = test_offset + (8 - (test_offset % 8)); // skip over padding

        // copy in the arguments
        for vector in 0..num_vectors {
            // a test suite can have numerous vectors against a common code base
            for argcnt in 0..num_args {
                for word in 0..8 {
                    job.rf[(/* window * 32 * 8 + */argcnt * 8 + word) as usize] = vector_read(test_offset);
                    test_offset += 1;
                }
            }

            let mut passed = true;
            log::trace!("spawning job");
            match engine.spawn_job(job) {
                Ok(rf_result) => {
                    for word in 0..8 {
                        let expect = vector_read(test_offset);
                        test_offset += 1;
                        let actual = rf_result[(/* window * 32 * 8 + */31 * 8 + word) as usize];
                        if expect != actual {
                            log::error!("e/a {:08x}/{:08x}", expect, actual);
                            passed = false;
                        }
                    }
                }
                Err(e) => {
                    log::error!(
                        "system error {:?} in running test vector: {}/0x{:x}",
                        e,
                        vector,
                        test_offset
                    );
                    passed = false;
                }
            }

            if passed {
                passes += 1;
            } else {
                log::error!(
                    "arithmetic or system error in running test vector: {}/0x{:x}",
                    vector,
                    test_offset
                );
                fails += 1;
            }
        }
    }
    (passes, fails)
}
/*
benchmark notes:

+59mA +/-1mA current draw off fully charged battery when running the benchmark
1246-1261ms/check vector iteration (10 iters total, 1450 vectors total)
*/
pub fn benchmark_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let xns = xous_names::XousNames::new().unwrap();
    let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();

    let mut engine = engine_25519::Engine25519::new();

    let mut trng = trng::Trng::new(&xns).unwrap();

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("benchmark got msg {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(BenchOp::StartEngine) => {
                let mut passes = 0;
                let mut fails = 0;
                for _ in 0..TEST_ITERS {
                    let (p, f) = run_vectors(&mut engine);
                    passes += p;
                    fails += f;
                }

                xous::send_message(
                    callback_conn,
                    xous::Message::new_scalar(
                        CB_ID.load(Ordering::Relaxed) as usize,
                        passes as usize,
                        fails as usize,
                        BenchResult::EngineDone.to_usize().unwrap(),
                        TEST_ITERS as usize,
                    ),
                )
                .unwrap();
            }
            /*
                2xop => each iteration has 2x DH ops in it (one for alice, one for bob)
                202ms/2xop (10 x 10 iters - sw)
                40.5ms/2xop (10 x 10 iters - hw)
                33ms/2xop (200 iters - hw) - affine done in software
                190ms/2xop (200 iters - sw)
                26ms/2xop (200 iters -hw) -- affine done in hardware

                13.54ms/2xop (200 iters -hw) -- microcode pre-loaded, minimal registers transferred using MontgomeryJob call
                12.2ms/2xop (200 iters -hw) -- with L2 cache on (128k)
                12.29ms/2xop (200 iters -hw) -- with L2 cache on (64k)
            */
            Some(BenchOp::StartDh) => {
                let mut passes = 0;
                let mut fails = 0;

                use x25519_dalek::{PublicKey, StaticSecret};
                let alice_secret = StaticSecret::new(&mut trng);
                let alice_public = PublicKey::from(&alice_secret);
                let bob_secret = StaticSecret::new(&mut trng);
                let bob_public = PublicKey::from(&bob_secret);
                for _ in 0..TEST_ITERS_DH {
                    let alice_shared_secret = alice_secret.diffie_hellman(&bob_public);
                    let bob_shared_secret = bob_secret.diffie_hellman(&alice_public);
                    let mut pass = true;
                    for (&alice, &bob) in
                        alice_shared_secret.as_bytes().iter().zip(bob_shared_secret.as_bytes().iter())
                    {
                        if alice != bob {
                            pass = false;
                        }
                    }
                    if pass {
                        passes += 2; // 2 diffie_hellman ops / iter
                    } else {
                        fails += 2;
                    }
                }
                xous::send_message(
                    callback_conn,
                    xous::Message::new_scalar(
                        CB_ID.load(Ordering::Relaxed) as usize,
                        passes as usize,
                        fails as usize,
                        BenchResult::DhDone.to_usize().unwrap(),
                        TEST_ITERS_DH as usize,
                    ),
                )
                .unwrap();
            }
            Some(BenchOp::Quit) => {
                log::info!("quitting benchmark thread");
                break;
            }
            None => {
                log::error!("received unknown opcode");
            }
        }
    }
    xous::destroy_server(sid).unwrap();
}

mod wycheproof {
    pub const WYCHEPROOF_NO_TEST_CASES: usize = 518;
    pub const WHYCHEPROOF_TEST_CASE_SIZE: usize = 96;

    #[export_name = "wycheproof_vectors"]
    pub static WYCHEPROOF_VECTORS: &[u8; WYCHEPROOF_NO_TEST_CASES * WHYCHEPROOF_TEST_CASE_SIZE] =
        include_bytes!("x25519_test.bin");

    pub struct WycheproofTestCase {
        // inferred from the index; not read from the binary data
        public: [u8; 32],
        id: usize,
        private: [u8; 32],
        shared: [u8; 32],
    }

    impl WycheproofTestCase {
        pub fn read(test_index: usize) -> Self {
            use std::io::Read;
            let test_offset = test_index * WHYCHEPROOF_TEST_CASE_SIZE;
            let mut public = [0u8; 32];
            (&WYCHEPROOF_VECTORS[test_offset..test_offset + 32]).read_exact(&mut public).unwrap();
            let mut private = [0u8; 32];
            (&WYCHEPROOF_VECTORS[test_offset + 32..test_offset + 64]).read_exact(&mut private).unwrap();
            let mut shared = [0u8; 32];
            (&WYCHEPROOF_VECTORS[test_offset + 64..test_offset + 96]).read_exact(&mut shared).unwrap();

            WycheproofTestCase { id: test_index + 1, public, private, shared }
        }

        pub fn run(&self) -> Option<TestCaseError> {
            use x25519_dalek::{PublicKey, StaticSecret};
            let private = StaticSecret::from(self.private);
            let public = PublicKey::from(self.public);
            let shared = private.diffie_hellman(&public).to_bytes();
            if shared != self.shared {
                Some(TestCaseError { test_id: self.id, expected: self.shared, actual: shared })
            } else {
                None
            }
        }
    }

    pub struct TestCaseError {
        pub(crate) test_id: usize,
        pub(crate) expected: [u8; 32],
        pub(crate) actual: [u8; 32],
    }
}
#[derive(Debug)]
pub struct Engine {
    susres: susres::Susres,
    benchmark_cid: xous::CID,
    start_time: Option<u64>,
}

impl Engine {
    pub fn new(xns: &xous_names::XousNames, env: &mut CommonEnv) -> Self {
        let sid = xous::create_server().unwrap();
        let sid_tuple = sid.to_u32();

        let cb_id = env.register_handler(String::<256>::from_str("engine"));
        CB_ID.store(cb_id, Ordering::Relaxed);

        xous::create_thread_4(
            benchmark_thread,
            sid_tuple.0 as usize,
            sid_tuple.1 as usize,
            sid_tuple.2 as usize,
            sid_tuple.3 as usize,
        )
        .unwrap();
        Engine {
            susres: susres::Susres::new_without_hook(&xns).unwrap(),
            benchmark_cid: xous::connect(sid).unwrap(),
            start_time: None,
        }
    }
}

impl<'a> ShellCmdApi<'a> for Engine {
    cmd_api!(engine);

    // inserts boilerplate for command API

    fn process(
        &mut self,
        args: String<1024>,
        env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "engine [check] [bench] [benchdh] [susres] [dh] [ed] [wycheproof]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "check" => {
                    let mut engine = engine_25519::Engine25519::new();
                    log::debug!("running vectors");
                    let (passes, fails) = run_vectors(&mut engine);

                    write!(ret, "Engine passed {} vectors, failed {} vectors", passes, fails).unwrap();
                }
                "bench" => {
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(
                        self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartEngine.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .unwrap();
                    write!(ret, "Starting Engine hardware benchmark with {} iters", TEST_ITERS).unwrap();
                }
                "benchdh" => {
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(
                        self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartDh.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .unwrap();
                    write!(ret, "Starting DH hardware benchmark").unwrap();
                }
                "susres" => {
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(
                        self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartEngine.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .unwrap();
                    let wait_time = (env.trng.get_u32().unwrap() % 2000) + 500; // at least half a second wait, up to 2 seconds
                    env.ticktimer.sleep_ms(wait_time as _).unwrap();
                    self.susres.initiate_suspend().unwrap();
                    write!(ret, "Interrupted Engine hardware benchmark with a suspend/resume").unwrap();
                }
                "dh" => {
                    use x25519_dalek::{EphemeralSecret, PublicKey};
                    let alice_secret = EphemeralSecret::new(&mut env.trng);
                    let alice_public = PublicKey::from(&alice_secret);
                    let bob_secret = EphemeralSecret::new(&mut env.trng);
                    let bob_public = PublicKey::from(&bob_secret);
                    let alice_shared_secret = alice_secret.diffie_hellman(&bob_public);
                    let bob_shared_secret = bob_secret.diffie_hellman(&alice_public);
                    let mut pass = true;
                    for (&alice, &bob) in
                        alice_shared_secret.as_bytes().iter().zip(bob_shared_secret.as_bytes().iter())
                    {
                        if alice != bob {
                            pass = false;
                        }
                    }
                    log::info!("alice: {:02x?}", alice_shared_secret.as_bytes());
                    log::info!("bob: {:02x?}", bob_shared_secret.as_bytes());
                    if pass {
                        write!(ret, "x25519 key exchange pass").unwrap();
                    } else {
                        write!(ret, "x25519 key exchange fail").unwrap();
                    }

                    /////////////////////// fixed vectors from x25519-dalek tests
                    use curve25519_dalek::montgomery::MontgomeryPoint;
                    use curve25519_dalek::scalar::Scalar;
                    /// "Decode" a scalar from a 32-byte array.
                    ///
                    /// By "decode" here, what is really meant is applying key clamping by twiddling
                    /// some bits.
                    ///
                    /// # Returns
                    ///
                    /// A `Scalar`.
                    fn clamp_scalar(mut scalar: [u8; 32]) -> Scalar {
                        scalar[0] &= 248;
                        scalar[31] &= 127;
                        scalar[31] |= 64;

                        Scalar::from_bits(scalar)
                    }

                    /// The bare, byte-oriented x25519 function, exactly as specified in RFC7748.
                    ///
                    /// This can be used with [`X25519_BASEPOINT_BYTES`] for people who
                    /// cannot use the better, safer, and faster DH API.
                    fn x25519(k: [u8; 32], u: [u8; 32]) -> [u8; 32] {
                        (clamp_scalar(k) * MontgomeryPoint(u)).to_bytes()
                    }
                    {
                        let input_scalar: [u8; 32] = [
                            0xa5, 0x46, 0xe3, 0x6b, 0xf0, 0x52, 0x7c, 0x9d, 0x3b, 0x16, 0x15, 0x4b, 0x82,
                            0x46, 0x5e, 0xdd, 0x62, 0x14, 0x4c, 0x0a, 0xc1, 0xfc, 0x5a, 0x18, 0x50, 0x6a,
                            0x22, 0x44, 0xba, 0x44, 0x9a, 0xc4,
                        ];
                        let input_point: [u8; 32] = [
                            0xe6, 0xdb, 0x68, 0x67, 0x58, 0x30, 0x30, 0xdb, 0x35, 0x94, 0xc1, 0xa4, 0x24,
                            0xb1, 0x5f, 0x7c, 0x72, 0x66, 0x24, 0xec, 0x26, 0xb3, 0x35, 0x3b, 0x10, 0xa9,
                            0x03, 0xa6, 0xd0, 0xab, 0x1c, 0x4c,
                        ];
                        let expected: [u8; 32] = [
                            0xc3, 0xda, 0x55, 0x37, 0x9d, 0xe9, 0xc6, 0x90, 0x8e, 0x94, 0xea, 0x4d, 0xf2,
                            0x8d, 0x08, 0x4f, 0x32, 0xec, 0xcf, 0x03, 0x49, 0x1c, 0x71, 0xf7, 0x54, 0xb4,
                            0x07, 0x55, 0x77, 0xa2, 0x85, 0x52,
                        ];
                        let result = x25519(input_scalar, input_point);
                        if result != expected {
                            write!(ret, "x25519 vector 1 failed!\n").unwrap();
                            log::error!("result: {:x?}", result);
                            log::error!("expected: {:x?}", expected);
                        } else {
                            write!(ret, "x25519 vector 1 passed\n").unwrap();
                        }
                    }
                    {
                        let input_scalar: [u8; 32] = [
                            0x4b, 0x66, 0xe9, 0xd4, 0xd1, 0xb4, 0x67, 0x3c, 0x5a, 0xd2, 0x26, 0x91, 0x95,
                            0x7d, 0x6a, 0xf5, 0xc1, 0x1b, 0x64, 0x21, 0xe0, 0xea, 0x01, 0xd4, 0x2c, 0xa4,
                            0x16, 0x9e, 0x79, 0x18, 0xba, 0x0d,
                        ];
                        let input_point: [u8; 32] = [
                            0xe5, 0x21, 0x0f, 0x12, 0x78, 0x68, 0x11, 0xd3, 0xf4, 0xb7, 0x95, 0x9d, 0x05,
                            0x38, 0xae, 0x2c, 0x31, 0xdb, 0xe7, 0x10, 0x6f, 0xc0, 0x3c, 0x3e, 0xfc, 0x4c,
                            0xd5, 0x49, 0xc7, 0x15, 0xa4, 0x93,
                        ];
                        let expected: [u8; 32] = [
                            0x95, 0xcb, 0xde, 0x94, 0x76, 0xe8, 0x90, 0x7d, 0x7a, 0xad, 0xe4, 0x5c, 0xb4,
                            0xb8, 0x73, 0xf8, 0x8b, 0x59, 0x5a, 0x68, 0x79, 0x9f, 0xa1, 0x52, 0xe6, 0xf8,
                            0xf7, 0x64, 0x7a, 0xac, 0x79, 0x57,
                        ];
                        let result = x25519(input_scalar, input_point);
                        if result != expected {
                            write!(ret, "x25519 vector 2 failed!\n").unwrap();
                            log::error!("result: {:x?}", result);
                            log::error!("expected: {:x?}", expected);
                        } else {
                            write!(ret, "x25519 vector 2 passed\n").unwrap();
                        }
                    }
                }
                "ed2" => {
                    use ed25519_dalek::{Signature, Signer, SigningKey};
                    let good: &[u8] = "test message".as_bytes();
                    let bad: &[u8] = "wrong message".as_bytes();

                    let signingkey = SigningKey::generate(&mut env.trng);
                    let good_sig = signingkey.sign(&good);
                    let bad_sig = signingkey.sign(&bad);

                    if signingkey.verify(&good, &good_sig).is_ok() {
                        write!(ret, "Verification of valid signtaure passed!\n").unwrap();
                    } else {
                        write!(ret, "Verification of valid signtaure failed!\n").unwrap();
                    }
                    if signingkey.verify(&good, &bad_sig).is_err() {
                        write!(
                            ret,
                            "Verification of a signature on a different message failed, as expected.\n"
                        )
                        .unwrap();
                    } else {
                        write!(ret, "Verification of a signature on a different message passed (this is unexpected)!\n").unwrap();
                    }
                    if signingkey.verify(&bad, &good_sig).is_err() {
                        write!(
                            ret,
                            "Verification of a signature on a different message failed, as expected.\n"
                        )
                        .unwrap();
                    } else {
                        write!(ret, "Verification of a signature on a different message passed (this is unexpected)!\n").unwrap();
                    }
                }
                "ed" => {
                    use ed25519_dalek::*;
                    // use ed25519::signature::Signature as _;
                    use hex::FromHex;
                    let secret_key: &[u8] =
                        b"833fe62409237b9d62ec77587520911e9a759cec1d19755b7da901b96dca3d42";
                    let signing_key: &[u8] =
                        b"ec172b93ad5e563bf4932c70e1245034c35467ef2efd4d64ebf819683467e2bf";
                    let message: &[u8] = b"616263";
                    let signature: &[u8] = b"98a70222f0b8121aa9d30f813d683f809e462b469c7ff87639499bb94e6dae4131f85042463c2a355a2003d062adf5aaa10b8c61e636062aaad11c2a26083406";
                    let msg_bytes = <[u8; 3]>::from_hex(message).unwrap();

                    let secret_key_bytes = <[u8; SECRET_KEY_LENGTH]>::from_hex(secret_key).unwrap();
                    let signing_key_bytes = <[u8; PUBLIC_KEY_LENGTH]>::from_hex(signing_key).unwrap();
                    let signature_bytes = <[u8; SIGNATURE_LENGTH]>::from_hex(signature).unwrap();

                    let signingkey: SigningKey = SigningKey::from_bytes(&signing_key_bytes);
                    let sig1: Signature = Signature::from_bytes(&signature_bytes);

                    //let mut prehash_for_signing = engine_sha512::Sha512::default(); // this defaults to Hw
                    // then Sw strategy let mut prehash_for_verifying =
                    // engine_sha512::Sha512::default();
                    let mut prehash_for_signing = sha2::Sha512::default(); // this defaults to Hw then Sw strategy
                    let mut prehash_for_verifying = sha2::Sha512::default();

                    prehash_for_signing.update(&msg_bytes[..]);
                    prehash_for_verifying.update(&msg_bytes[..]);

                    let sig2: Signature = signingkey.sign_prehashed(prehash_for_signing, None).unwrap();

                    log::info!("original: {:02x?}", sig1);
                    log::info!("produced: {:02x?}", sig2);
                    let mut pass = true;
                    if sig1 != sig2 {
                        pass = false;
                        write!(
                            ret,
                            "Original signature from test vectors doesn't equal signature produced:\
                            \noriginal:\n{:?}\nproduced:\n{:?}",
                            sig1, sig2
                        )
                        .unwrap();
                    }
                    if signingkey.verify_prehashed(prehash_for_verifying, None, &sig2).is_err() {
                        pass = false;
                        write!(ret, "Could not verify ed25519ph signature!").unwrap();
                    }
                    if pass {
                        write!(ret, "Passed ed25519 simple check").unwrap();
                    }
                }
                "wycheproof" => {
                    use hex::ToHex;
                    use wycheproof::*;
                    let failures: Vec<TestCaseError> = (0..WYCHEPROOF_NO_TEST_CASES)
                        .filter_map(|test_index| WycheproofTestCase::read(test_index).run())
                        .collect();
                    write!(ret, "Ran {} tests. {} failures.", WYCHEPROOF_NO_TEST_CASES, failures.len())
                        .unwrap();
                    let num_failures = failures.len();
                    if failures.len() > 0 {
                        write!(
                            ret,
                            "\nFailed tests: {:?}",
                            failures.iter().map(|tc| tc.test_id).collect::<Vec<usize>>()
                        )
                        .unwrap();
                        for failure in failures {
                            log::error!("wycheproof test #{} failed:", failure.test_id);
                            log::error!("expected: {}", failure.expected.encode_hex::<std::string::String>());
                            log::error!("actual:   {}", failure.actual.encode_hex::<std::string::String>());
                        }
                        log::info!(
                            "{}BENCH,WYCHEPROOF,FAIL,{},{}",
                            xous::BOOKEND_START,
                            num_failures,
                            xous::BOOKEND_END
                        );
                    } else {
                        log::info!("{}BENCH,WYCHEPROOF,PASS,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    }
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }

    fn callback(
        &mut self,
        msg: &xous::MessageEnvelope,
        env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        use core::fmt::Write;

        log::debug!("benchmark callback");
        let mut ret = String::<1024>::new();

        xous::msg_scalar_unpack!(msg, passes, fails, result_type, iters, {
            let end = env.ticktimer.elapsed_ms();
            let elapsed: f32 = ((end - self.start_time.unwrap()) as f32) / iters as f32;
            match FromPrimitive::from_usize(result_type) {
                Some(BenchResult::EngineDone) => {
                    write!(
                        ret,
                        "{}ms/check_iter; In total, Engine passed {} vectors, failed {} vectors",
                        elapsed, passes, fails
                    )
                    .unwrap();
                }
                Some(BenchResult::DhDone) => {
                    write!(ret, "{}ms/DH_iter; Passed {} ops, failed {} ops", elapsed, passes, fails)
                        .unwrap();
                    if fails == 0 {
                        log::info!("{}BENCH,DH,PASS,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    } else {
                        log::info!("{}BENCH,DH,FAIL,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    }
                }
                _ => {
                    write!(ret, "Engine bench callback with unknown bench type").unwrap();
                }
            }
        });
        Ok(Some(ret))
    }
}
