use core::sync::atomic::{AtomicU32, Ordering};

use num_traits::*;
use sha2::Digest;
use String;

use crate::{CommonEnv, ShellCmdApi};
static CB_ID: AtomicU32 = AtomicU32::new(0);

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum BenchOp {
    StartSha512Hw,
    StartSha512Sw,
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum BenchResult {
    Sha512HwDone,
    Sha512SwDone,
}

const TEST_ITERS: usize = 1000;
const TEST_MAX_LEN: usize = 8192;
const TEST_FIXED_LEN: bool = true;
/*
benchmark notes:
TEST_MAX_LEN = 16384 (fixed length) / TEST_ITERS = 1000: hw 25.986ms/hash, sw 155.11ms/hash
TEST_MAX_LEN = 16384 (random length) / TEST_ITERS = 1000: hw 15.262ms/hash, sw 80.053ms/hash
TEST_MAX_LEN = 256 (random length) / TEST_ITERS = 1000: hw 6.968ms/hash, sw 3.987ms/hash
TEST_MAX_LEN = 512 (random length) / TEST_ITERS = 1000: hw 7.332ms/hash, sw 5.485ms/hash
TEST_MAX_LEN = 1024 (fixed length) / TEST_ITERS = 1000: hw 7.257ms/hash, sw 11.676ms/hash
TEST_MAX_LEN = 8192 (random length) / TEST_ITERS = 1000: hw 10.528ms/hash, sw 40.631ms/hash
TEST_MAX_LEN = 8192 (fixed length) / TEST_ITERS = 1000: hw 17.035ms/hash, sw 78.633ms/hash

with 128k L2 cache on:
TEST_MAX_LEN = 8192 (fixed length) / TEST_ITERS = 1000: hw 13.798ms/hash, sw 29.034ms/hash
with 64k L2 cache on:
TEST_MAX_LEN = 8192 (fixed length) / TEST_ITERS = 1000: hw 13.798ms/hash, sw 30.029ms/hash

power consumption -
4.1V system voltage
159mA nominal
182mA while running hw benchmark (17.569ms/hash)
~ 23mA for SHA hardware unit doing 8k fixed length hashes, ~14% extra power, 1.65mJ/hash
172mA while running sw benchmark (78.525ms/hash) -> ~10mA excess power for software -> 3.22mJ/hash
~50% power savings to use hardware hasher

v0.10.8 API implementation
TEST_MAX_LEN = 8192 (fixed length) / TEST_ITERS = 1000: hw 11.464ms/hash, sw 21.502ms/hash (with trng ID bug (ID was not regenerated on each iteration), oops)
TEST_MAX_LEN = 8192 (fixed length) / TEST_ITERS = 1000: hw 12.196ms/hash, sw 21.643ms/hash
 */

pub fn benchmark_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let mut dataset: [u8; TEST_MAX_LEN] = [0; TEST_MAX_LEN];
    let xns = xous_names::XousNames::new().unwrap();
    let trng = trng::Trng::new(&xns).unwrap();
    let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();

    let mut last_result: [u8; 64] = [0; 64];
    let mut first_time = true;

    // fill a random array with words
    for chunk in dataset.chunks_exact_mut(8) {
        chunk.clone_from_slice(&trng.get_u64().unwrap().to_be_bytes());
    }

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("benchmark got msg {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(BenchOp::StartSha512Hw) | Some(BenchOp::StartSha512Sw) => {
                let mut hw_mode = true;
                let mut accumulator = [0 as u8; 64];
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(BenchOp::StartSha512Sw) => {
                        hw_mode = false;
                        let mut hasher = sha2::Sha512Sw::new();
                        // have to duplicate this code because the hasher is a different type.
                        for i in 0..TEST_ITERS {
                            //log::debug!("iter {}", i);
                            // pick a random length for the test -- this helps to exercise corner cases in the
                            // hash handler core
                            let iterlen = if TEST_FIXED_LEN {
                                TEST_MAX_LEN
                            } else {
                                if i < TEST_MAX_LEN - 2 {
                                    (dataset[i] as usize) | ((dataset[i + 1] as usize) << 8) % TEST_MAX_LEN
                                } else {
                                    TEST_MAX_LEN
                                }
                            };
                            hasher.update(&dataset[..iterlen]);
                            let digest = hasher.finalize_reset();
                            for (&src, dest) in digest.iter().zip(&mut accumulator.iter_mut()) {
                                *dest = (*dest).wrapping_add(src);
                            }
                        }
                    }
                    _ => {
                        // should be "wait for hardware", but is currently "hardware-then-software"...
                        let mut hasher = sha2::Sha512Hw::new();
                        // have to duplicate this code because the hasher is a different type.
                        for i in 0..TEST_ITERS {
                            //log::debug!("iter {}", i);
                            // pick a random length for the test -- this helps to exercise corner cases in the
                            // hash handler core
                            let iterlen = if TEST_FIXED_LEN {
                                TEST_MAX_LEN
                            } else {
                                if i < TEST_MAX_LEN - 2 {
                                    (dataset[i] as usize) | ((dataset[i + 1] as usize) << 8) % TEST_MAX_LEN
                                } else {
                                    TEST_MAX_LEN
                                }
                            };
                            hasher.update(&dataset[..iterlen]);
                            let digest = hasher.finalize_reset();
                            for (&src, dest) in digest.iter().zip(&mut accumulator.iter_mut()) {
                                *dest = (*dest).wrapping_add(src);
                            }
                        }
                    }
                };

                let mut pass = true;
                for (&current, previous) in accumulator.iter().zip(last_result.iter_mut()) {
                    if current != *previous {
                        pass = false;
                    }
                    *previous = current;
                }
                xous::send_message(
                    callback_conn,
                    xous::Message::new_scalar(
                        CB_ID.load(Ordering::Relaxed) as usize,
                        if pass { 1 } else { 0 },
                        if first_time { 1 } else { 0 },
                        if hw_mode { 1 } else { 0 },
                        0,
                    ),
                )
                .unwrap();
                first_time = false;
                log::debug!("accumulated result: {:x?}", last_result);
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
pub struct Sha {
    susres: susres::Susres,
    benchmark_cid: xous::CID,
    start_time: Option<u64>,
}
impl Sha {
    pub fn new(xns: &xous_names::XousNames, env: &mut CommonEnv) -> Self {
        let sid = xous::create_server().unwrap();
        let sid_tuple = sid.to_u32();

        let cb_id = env.register_handler(String::from("sha"));
        CB_ID.store(cb_id, Ordering::Relaxed);

        xous::create_thread_4(
            benchmark_thread,
            sid_tuple.0 as usize,
            sid_tuple.1 as usize,
            sid_tuple.2 as usize,
            sid_tuple.3 as usize,
        )
        .unwrap();
        Sha {
            susres: susres::Susres::new_without_hook(&xns).unwrap(),
            benchmark_cid: xous::connect(sid).unwrap(),
            start_time: None,
        }
    }
}

impl<'a> ShellCmdApi<'a> for Sha {
    cmd_api!(sha);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "sha [check] [check256] [hwbench] [swbench] [susres]";

        let mut tokens = &args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "check" => {
                    // check the sha512 operation with the test string from the OpenTitan reference
                    // implementation
                    const K_DATA: &'static [u8; 142] = b"Every one suspects himself of at least one of the cardinal virtues, and this is mine: I am one of the few honest people that I have ever known";
                    const K_EXPECTED_DIGEST: [u8; 64] = [
                        0x7a, 0x72, 0x6b, 0xd1, 0xc0, 0x78, 0xfc, 0x02, 0x7b, 0xc9, 0xe6, 0x79, 0x32, 0x0a,
                        0x57, 0x18, 0x51, 0x20, 0xe9, 0xb2, 0x71, 0x88, 0x3b, 0x11, 0xdf, 0xfe, 0x69, 0x01,
                        0xb2, 0x47, 0x09, 0x4c, 0x31, 0xd0, 0x4a, 0xd0, 0x4a, 0x09, 0x67, 0x1a, 0x01, 0x50,
                        0x12, 0x40, 0xc3, 0x8c, 0x5f, 0xab, 0x3a, 0x3a, 0x6d, 0xf3, 0x7a, 0x7d, 0xbd, 0xff,
                        0x6d, 0xd8, 0xbb, 0x73, 0x5d, 0x46, 0xe8, 0xf7,
                    ];

                    let mut pass: bool = true;
                    let mut hasher = sha2::Sha512::new();

                    hasher.update(K_DATA);
                    let digest = hasher.finalize();

                    for (&expected, result) in K_EXPECTED_DIGEST.iter().zip(digest) {
                        if expected != result {
                            pass = false;
                        }
                    }
                    if pass {
                        write!(ret, "Sha512 passed.").unwrap();
                    } else {
                        write!(ret, "Sha512 failed: {:x?}", digest).unwrap();
                    }
                }
                "check256" => {
                    // check the sha512 operation with the test string from the OpenTitan reference
                    // implementation
                    const K_DATA: &'static [u8; 142] = b"Every one suspects himself of at least one of the cardinal virtues, and this is mine: I am one of the few honest people that I have ever known";
                    const K_EXPECTED_DIGEST_256: [u8; 32] = [
                        0x3d, 0xfb, 0xf2, 0x09, 0x57, 0x9a, 0xfe, 0x4e, 0xb9, 0x1c, 0xaf, 0xe6, 0xf5, 0x8a,
                        0x53, 0x56, 0xcc, 0xc4, 0xce, 0x36, 0xf1, 0xed, 0x77, 0x44, 0xe9, 0x52, 0x34, 0x7f,
                        0x79, 0x61, 0x9a, 0x9f,
                    ];

                    let mut pass: bool = true;
                    let mut hasher = sha2::Sha512_256::new();

                    hasher.update(K_DATA);
                    let digest = hasher.finalize();

                    for (&expected, result) in K_EXPECTED_DIGEST_256.iter().zip(digest) {
                        if expected != result {
                            pass = false;
                        }
                    }
                    if pass {
                        write!(ret, "Sha512/256 passed.").unwrap();
                    } else {
                        write!(ret, "Sha512/256 failed: {:x?}", digest).unwrap();
                    }
                }
                "hwbench" => {
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(
                        self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartSha512Hw.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .unwrap();
                    write!(
                        ret,
                        "Starting Sha512 hardware benchmark with {} iters, {} max_len, {} fixed_len",
                        TEST_ITERS, TEST_MAX_LEN, TEST_FIXED_LEN
                    )
                    .unwrap();
                }
                "swbench" => {
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(
                        self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartSha512Sw.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .unwrap();
                    write!(
                        ret,
                        "Starting Sha512 software benchmark with {} iters, {} max_len, {} fixed_len",
                        TEST_ITERS, TEST_MAX_LEN, TEST_FIXED_LEN
                    )
                    .unwrap();
                }
                "susres" => {
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(
                        self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartSha512Hw.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .unwrap();
                    let wait_time = (env.trng.get_u32().unwrap() % 2000) + 500; // at least half a second wait, up to 2 seconds
                    env.ticktimer.sleep_ms(wait_time as _).unwrap();
                    self.susres.initiate_suspend().unwrap();
                    write!(ret, "Interrupted Sha512 hardware benchmark with a suspend/resume").unwrap();
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
        use core::fmt::Write;

        log::debug!("benchmark callback");
        let mut ret = String::new();

        xous::msg_scalar_unpack!(msg, pass, first_time, hw_mode, _, {
            let end = env.ticktimer.elapsed_ms();
            let elapsed: f32 = ((end - self.start_time.unwrap()) as f32) / TEST_ITERS as f32;
            let modestr = if hw_mode != 0 { &"hw" } else { &"sw" };
            if first_time != 0 {
                write!(ret, "[{}] first pass: {}ms/hash", modestr, elapsed).unwrap();
                log::info!("{}BENCH,SHA,FIRST,{}ms/hash,{}", xous::BOOKEND_START, elapsed, xous::BOOKEND_END);
            } else {
                if pass != 0 {
                    write!(ret, "[{}] match to previous pass: {}ms/hash", modestr, elapsed).unwrap();
                    log::info!(
                        "{}BENCH,SHA,PASS,{}ms/hash,{}",
                        xous::BOOKEND_START,
                        elapsed,
                        xous::BOOKEND_END
                    );
                } else {
                    // pass was 0, we failed
                    write!(ret, "[{}] FAILED match to previous pass: {}ms/hash", modestr, elapsed).unwrap();
                    log::info!(
                        "{}BENCH,SHA,FAIL,{}ms/hash,{}",
                        xous::BOOKEND_START,
                        elapsed,
                        xous::BOOKEND_END
                    );
                }
            }
        });
        Ok(Some(ret))
    }
}
