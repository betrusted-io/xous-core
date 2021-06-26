use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;

use engine_25519::*;

use num_traits::*;

use core::sync::atomic::{AtomicU32, Ordering};
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
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum BenchResult {
    EngineDone,
}

const TEST_ITERS: usize = 10;

fn vector_read(word_offset: usize) -> u32 {
    let mut bytes: [u8; 4] = [0; 4];
    for i in 0..4 {
        bytes[i] = ENGINE_VECTORS[word_offset*4 + i];
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
                    job.rf[(/*window * 32 * 8 +*/ argcnt * 8 + word) as usize] = vector_read(test_offset);
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
                        let actual = rf_result[(/*window * 32 * 8 + */ 31 * 8 + word) as usize];
                        if expect != actual {
                            log::error!("e/a {:08x}/{:08x}", expect, actual);
                            passed = false;
                        }
                    }
                },
                Err(e) => {
                    log::error!("system error {:?} in running test vector: {}/0x{:x}", e, vector, test_offset);
                    passed = false;
                }
            }

            if passed {
                passes += 1;
            } else {
                log::error!("arithmetic or system error in running test vector: {}/0x{:x}", vector, test_offset);
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

                xous::send_message(callback_conn,
                    xous::Message::new_scalar(CB_ID.load(Ordering::Relaxed) as usize,
                    passes as usize,
                    fails as usize,
                    0, 0)
                ).unwrap();
            },
            Some(BenchOp::Quit) => {
                log::info!("quitting benchmark thread");
                break;
            },
            None => {
                log::error!("received unknown opcode");
            }
        }
    }
    xous::destroy_server(sid).unwrap();
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

        xous::create_thread_4(benchmark_thread, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
        Engine {
            susres: susres::Susres::new_without_hook(&xns).unwrap(),
            benchmark_cid: xous::connect(sid).unwrap(),
            start_time: None,
        }
    }
}

impl<'a> ShellCmdApi<'a> for Engine {
    cmd_api!(engine); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "engine [check] [bench] [susres]";

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
                    xous::send_message(self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartEngine.to_usize().unwrap(), 0, 0, 0, 0)
                    ).unwrap();
                    write!(ret, "Starting Engine hardware benchmark with {} iters", TEST_ITERS).unwrap();
                }
                "susres" => {
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(self.benchmark_cid,
                        xous::Message::new_scalar(BenchOp::StartEngine.to_usize().unwrap(), 0, 0, 0, 0)
                    ).unwrap();
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
                    for (&alice, &bob) in alice_shared_secret.as_bytes().iter().zip(bob_shared_secret.as_bytes().iter()) {
                        if alice != bob {
                            pass = false;
                        }
                    }
                    log::info!("alice: {:?}", alice_shared_secret.as_bytes());
                    log::info!("bob: {:?}", bob_shared_secret.as_bytes());
                    if pass {
                        write!(ret, "x25519 key exchange pass").unwrap();
                    } else {
                        write!(ret, "x25519 key exchange fail").unwrap();
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

    fn callback(&mut self, msg: &xous::MessageEnvelope, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        log::debug!("benchmark callback");
        let mut ret = String::<1024>::new();

        xous::msg_scalar_unpack!(msg, passes, fails, _, _, {
            let end = env.ticktimer.elapsed_ms();
            let elapsed: f64 = ((end - self.start_time.unwrap()) as f64) / TEST_ITERS as f64;
            write!(ret, "{}ms/check_iter; In total, Engine passed {} vectors, failed {} vectors", elapsed, passes, fails).unwrap();
        });
        Ok(Some(ret))
    }
}
