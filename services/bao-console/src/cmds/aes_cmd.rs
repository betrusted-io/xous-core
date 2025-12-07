use core::fmt::Write;

mod aes128tests;
mod aes256tests;

/// Define block cipher test
macro_rules! block_cipher_test {
    ($name:ident, $test_name:expr, $test_case_name:ident, $cipher:ty) => {
        fn $name() -> String {
            use aes::cipher::{
                BlockDecryptMut, BlockEncryptMut, KeyInit, consts::U16, generic_array::GenericArray,
            };

            fn run_test(key: &[u8], pt: &[u8], ct: &[u8]) -> (bool, GenericArray<u8, U16>) {
                let mut state = <$cipher as KeyInit>::new_from_slice(key).unwrap();

                let mut block = GenericArray::clone_from_slice(pt);
                state.encrypt_block_mut(&mut block);
                if ct != block.as_slice() {
                    return (false, block);
                }

                state.decrypt_block_mut(&mut block);
                if pt != block.as_slice() {
                    return (false, block);
                }

                (true, block)
            }

            fn run_par_test(key: &[u8], pt: &[u8]) -> bool {
                type Block = aes::cipher::Block<$cipher>;

                let mut state = <$cipher as KeyInit>::new_from_slice(key).unwrap();

                let block = Block::clone_from_slice(pt);
                let mut blocks1 = vec![block; 101];
                for (i, b) in blocks1.iter_mut().enumerate() {
                    *b = block;
                    b[0] = b[0].wrapping_add(i as u8);
                }
                let mut blocks2 = blocks1.clone();

                // check that `encrypt_blocks` and `encrypt_block`
                // result in the same ciphertext
                state.encrypt_blocks_mut(&mut blocks1);
                for b in blocks2.iter_mut() {
                    state.encrypt_block_mut(b);
                }
                if blocks1 != blocks2 {
                    return false;
                }

                // check that `encrypt_blocks` and `encrypt_block`
                // result in the same plaintext
                state.decrypt_blocks_mut(&mut blocks1);
                for b in blocks2.iter_mut() {
                    state.decrypt_block_mut(b);
                }
                if blocks1 != blocks2 {
                    return false;
                }

                true
            }

            let mut ret = String::new();
            write!(ret, "test {} passed", $test_name).unwrap();
            for (i, test) in $test_case_name.iter().enumerate() {
                let (pass, block) = run_test(&test.key, &test.pt, &test.ct);
                if !pass {
                    ret.clear();
                    write!(
                        ret,
                        "\n\
                         Failed test №{}\n\
                         key:\t{:x?}\n\
                         plaintext:\t{:x?}\n\
                         ciphertext:\t{:x?}\n\
                         block:\t{:x?}\n",
                        i, test.key, test.pt, test.ct, block
                    )
                    .unwrap();
                    return ret;
                }

                // test parallel blocks encryption/decryption
                if !run_par_test(&test.key, &test.pt) {
                    ret.clear();
                    write!(
                        ret,
                        "\n\
                         Failed parallel test №{}\n\
                         key:\t{:x?}\n\
                         plaintext:\t{:x?}\n\
                         ciphertext:\t{:x?}\n",
                        i, test.key, test.pt, test.ct,
                    )
                    .unwrap();
                    return ret;
                }
            }
            // test if cipher can be cloned
            let key = Default::default();
            let _ = <$cipher>::new(&key).clone();

            ret
        }
    };
}

use core::sync::atomic::{AtomicU32, Ordering};

use String;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::{Aes128, Aes128Soft, Aes256, Aes256Soft};
use num_traits::*;

use crate::{CommonEnv, ShellCmdApi};
static CB_ID: AtomicU32 = AtomicU32::new(0);

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum BenchOp {
    StartAesHw,
    StartAesSw,
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
#[allow(dead_code)]
pub(crate) enum BenchResult {
    AesHwDone,
    AesSwDone,
}

const TEST_ITERS: usize = 500;
const TEST_MAX_LEN: usize = 8192;
use aes::cipher::generic_array::GenericArray;

/*
hardware: -148mA @ 100% CPU usage, 77.74us/block enc+dec AES128 (500 iters, 8192 len)
software: -151mA @ 100% CPU usage, 158.36us/block enc+dec AES128 (500 iters, 8192 len)

hardware: -148mA @ 100% CPU usage, 103.73us/block enc+dec AES256 (500 iters, 8192 len)
software: -149mA @ 100% CPU usage, 217.95us/block enc+dec AES256 (500 iters, 8192 len)

with L2 cache on (128k):
hardware 92.24us/block enc+dec aes256 (500 iters, 8192 len)
software 158.48us/block enc+dec aes256 (500 iters, 8192 len)

with L2 cache on (64k):
hardware 92.24us/block enc+dec aes256 (500 iters, 8192 len)
software 154.05us/block enc+dec aes256 (500 iters, 8192 len)

with L2 cache on (64k) and 0.8.2 fixedslice implementation:
hardware 95.8us/block enc+dec aes256 (500 iters, 8192 len)
software 131.66us/block enc+dec aes256 (500 iters, 8192 len)

with L2 cache on (64k) and 0.9.13 release candidate:
hardware 85.52us/block enc+dec aes256 (500 iters, 8192 len)
software 140.84us/block enc+dec aes256 (500 iters, 8192 len)

with Vex-ii CPU (50MHz) (note that Vex-i runs at 100MHz) on vexii-testing branch:
software 202us/block enc+dec aes256 (500 iters, 8192 len)
hardware 60us/block enc+dec aes256 (500 iters, 8192 len) [initial]
hardware 25.9us/block enc+dec aes256 (500 iters, 8192 len) [optimized]

with vex bao1x CPU (400MHz)
hardware 4.65µs/block enc+dec AES256
software 45.79µs/block enc+dec AES256

with vex bao1x CPU (350MHz)
hardware 5.36µs/block enc+dec AES256
software 52.45µs/block enc+dec AES256
hardware - with chaffing - 10.27µs/block enc+dec AES256
*/
pub fn benchmark_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let mut dataset_ref: [u8; TEST_MAX_LEN] = [0; TEST_MAX_LEN];
    let xns = xous_names::XousNames::new().unwrap();
    let trng = bao1x_hal_service::trng::Trng::new(&xns).unwrap();
    let callback_conn = xns.request_connection_blocking(crate::shell::SERVER_NAME_SHELLCHAT).unwrap();

    // fill a random array with words
    for chunk in dataset_ref.chunks_exact_mut(8) {
        chunk.clone_from_slice(&trng.get_u64().unwrap().to_be_bytes());
    }
    // pick a random key
    let mut key_array: [u8; 32] = [0; 32];
    for k in key_array.chunks_exact_mut(8) {
        k.clone_from_slice(&trng.get_u64().unwrap().to_be_bytes());
    }
    let key = GenericArray::from_slice(&key_array);
    let cipher_hw = Aes256::new(&key);
    let cipher_sw = Aes256Soft::new(&key);

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("benchmark got msg {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(BenchOp::StartAesHw) | Some(BenchOp::StartAesSw) => {
                let hw_mode = match FromPrimitive::from_usize(msg.body.id()) {
                    Some(BenchOp::StartAesSw) => false,
                    _ => true,
                };
                let mut dataset_op: [u8; TEST_MAX_LEN] = [0; TEST_MAX_LEN];
                for (&src, dst) in dataset_ref.iter().zip(dataset_op.iter_mut()) {
                    *dst = src;
                }

                for _ in 0..TEST_ITERS {
                    if hw_mode {
                        for mut chunk in dataset_op.chunks_exact_mut(aes::BLOCK_SIZE) {
                            let mut block = GenericArray::clone_from_slice(&mut chunk);
                            cipher_hw.encrypt_block(&mut block);
                            for (&src, dst) in block.iter().zip(chunk.iter_mut()) {
                                *dst = src;
                            }
                        }
                        for mut chunk in dataset_op.chunks_exact_mut(aes::BLOCK_SIZE) {
                            let mut block = GenericArray::clone_from_slice(&mut chunk);
                            cipher_hw.decrypt_block(&mut block);
                            for (&src, dst) in block.iter().zip(chunk.iter_mut()) {
                                *dst = src;
                            }
                        }
                    } else {
                        for mut chunk in dataset_op.chunks_exact_mut(aes::BLOCK_SIZE) {
                            let mut block = GenericArray::clone_from_slice(&mut chunk);
                            cipher_sw.encrypt_block(&mut block);
                            for (&src, dst) in block.iter().zip(chunk.iter_mut()) {
                                *dst = src;
                            }
                        }
                        for mut chunk in dataset_op.chunks_exact_mut(aes::BLOCK_SIZE) {
                            let mut block = GenericArray::clone_from_slice(&mut chunk);
                            cipher_sw.decrypt_block(&mut block);
                            for (&src, dst) in block.iter().zip(chunk.iter_mut()) {
                                *dst = src;
                            }
                        }
                    }
                }
                let mut pass = true;
                for (&current, &previous) in dataset_ref.iter().zip(dataset_op.iter()) {
                    if current != previous {
                        pass = false;
                    }
                }
                xous::send_message(
                    callback_conn,
                    xous::Message::new_scalar(
                        CB_ID.load(Ordering::Relaxed) as usize,
                        if pass { 1 } else { 0 },
                        if hw_mode { 1 } else { 0 },
                        cipher_hw.key_size(),
                        0,
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

#[derive(Debug)]
pub struct Aes {
    benchmark_cid: xous::CID,
    start_time: Option<u64>,
}
impl Aes {
    pub fn new(_xns: &xous_names::XousNames, env: &mut CommonEnv) -> Self {
        let sid = xous::create_server().unwrap();
        let sid_tuple = sid.to_u32();

        let cb_id = env.register_handler(String::from("aes"));
        CB_ID.store(cb_id, Ordering::Relaxed);

        xous::create_thread_4(
            benchmark_thread,
            sid_tuple.0 as usize,
            sid_tuple.1 as usize,
            sid_tuple.2 as usize,
            sid_tuple.3 as usize,
        )
        .unwrap();
        Aes { benchmark_cid: xous::connect(sid).unwrap(), start_time: None }
    }
}

use aes128tests::AES128_TESTS;
use aes256tests::AES256_TESTS;
block_cipher_test!(aes128_test, "aes128", AES128_TESTS, Aes128);
block_cipher_test!(aes128soft_test, "aes128", AES128_TESTS, Aes128Soft);
block_cipher_test!(aes256_test, "aes256", AES256_TESTS, Aes256);
block_cipher_test!(aes256soft_test, "aes256", AES256_TESTS, Aes256Soft);

impl<'a> ShellCmdApi<'a> for Aes {
    cmd_api!(aes);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        let helpstring = "Aes [check128] [check128sw] [check256] [check256sw] [hwbench] [swbench]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "check128" => {
                    write!(ret, "{}", aes128_test()).unwrap();
                }
                "check128sw" => {
                    write!(ret, "{}", aes128soft_test()).unwrap();
                }
                "check256" => {
                    write!(ret, "{}", aes256_test()).unwrap();
                }
                "check256sw" => {
                    write!(ret, "{}", aes256soft_test()).unwrap();
                }
                "hwbench" => {
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(
                        self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartAesHw.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .unwrap();
                    write!(
                        ret,
                        "Starting Aes hardware benchmark with {} iters of {} blocks",
                        TEST_ITERS,
                        TEST_MAX_LEN / aes::BLOCK_SIZE
                    )
                    .unwrap();
                }
                "swbench" => {
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(
                        self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartAesSw.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .unwrap();
                    write!(
                        ret,
                        "Starting Aes software benchmark with {} iters of {} blocks",
                        TEST_ITERS,
                        TEST_MAX_LEN / aes::BLOCK_SIZE
                    )
                    .unwrap();
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
    ) -> Result<Option<String>, xous::Error> {
        log::debug!("benchmark callback");
        let mut ret = String::new();

        xous::msg_scalar_unpack!(msg, pass, hw_mode, keybits, _, {
            let end = env.ticktimer.elapsed_ms();
            let elapsed: f32 = ((end - self.start_time.unwrap()) as f32)
                / (TEST_ITERS as f32 * (TEST_MAX_LEN / aes::BLOCK_SIZE) as f32);
            let modestr = if hw_mode != 0 { &"hw" } else { &"sw" };
            if pass != 0 {
                write!(ret, "[{}] passed: {:.02}µs/block enc+dec AES{}", modestr, elapsed * 1000.0, keybits)
                    .unwrap();
                log::info!(
                    "{}BENCH,AES,PASS,{}us/block,{}",
                    bao1x_hal::board::BOOKEND_START,
                    elapsed * 1000.0,
                    bao1x_hal::board::BOOKEND_END
                );
            } else {
                // pass was 0, we failed
                write!(ret, "[{}] FAILED: {:.02}µs/block enc+dec AES{}", modestr, elapsed * 1000.0, keybits)
                    .unwrap();
                log::info!(
                    "{}BENCH,AES,FAIL,{}us/block,{}",
                    bao1x_hal::board::BOOKEND_START,
                    elapsed * 1000.0,
                    bao1x_hal::board::BOOKEND_END
                );
            }
        });
        Ok(Some(ret))
    }
}
