use num_traits::*;
use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Memtest {
    memtest_cid: xous::CID,
    start_time: Option<u64>,
}
impl Memtest {
    pub fn new(_xns: &xous_names::XousNames, env: &mut CommonEnv) -> Self {
        let sid = xous::create_server().unwrap();
        let sid_tuple = sid.to_u32();

        let cb_id = env.register_handler(String::from("memtest"));
        CB_ID.store(cb_id, Ordering::Relaxed);

        xous::create_thread_4(
            test_thread,
            sid_tuple.0 as usize,
            sid_tuple.1 as usize,
            sid_tuple.2 as usize,
            sid_tuple.3 as usize,
        )
        .unwrap();

        Memtest { memtest_cid: xous::connect(sid).unwrap(), start_time: None }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static CB_ID: AtomicU32 = AtomicU32::new(0);

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum TestOp {
    StartBasic,
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum TestResult {
    EngineDone,
    DhDone,
}

pub fn test_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let xns = xous_names::XousNames::new().unwrap();
    let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();

    let trng = trng::Trng::new(&xns).unwrap();

    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("memtest got msg {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(TestOp::StartBasic) => xous::msg_scalar_unpack!(msg, iters, _, _, _, {
                let mut testsrc = xous::syscall::map_memory(
                    None,
                    None,
                    256 * 1024, // min 128k so we are working through the L2 cache
                    xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::RESERVE,
                )
                .expect("couldn't allocate RAM for testing");
                let mut testdst = xous::syscall::map_memory(
                    None,
                    None,
                    256 * 1024,
                    xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::RESERVE,
                )
                .expect("couldn't allocate RAM for testing");

                let testsrc_slice: &mut [u32] = testsrc.as_slice_mut();
                let testdst_slice: &mut [u32] = testdst.as_slice_mut();
                let chunks_page = testsrc_slice.chunks_exact_mut(1024);
                for buf in chunks_page.into_iter() {
                    let mut incoming: [u32; 1024] = [0; 1024];
                    trng.fill_buf(&mut incoming).unwrap();
                    for (&src, dst) in incoming.iter().zip(buf.iter_mut()) {
                        *dst = src;
                    }
                }

                let mut errs = 0;
                let mut dummy = 0; // this prevents things from being optimized out
                for i in 0..iters {
                    // pick two random offsets to report data, just to make sure the test is going as expected
                    let c1 = trng.get_u32().unwrap() & ((256 * 1024 / 4) - 1);
                    let c2 = trng.get_u32().unwrap() & ((256 * 1024 / 4) - 1);
                    log::info!("starting memory test iter {}", i);
                    // copy random data into destination
                    for (&src, dst) in testsrc_slice.iter().zip(testdst_slice.iter_mut()) {
                        *dst = src;
                        dummy += *dst;
                    }
                    // compare random data once
                    let mut offset = 0;
                    for (&src, &dst) in testsrc_slice.iter().zip(testdst_slice.iter()) {
                        dummy += dst;
                        if dst != src {
                            dummy += src;
                            log::error!("* {:<4} | 0x{:08x}: e:0x{:08x} o:0x{:08x}", i, offset * 4, src, dst);
                            errs += 1;
                        }
                        if offset == c1 {
                            log::info!("  {:<4} | 0x{:08x}: e:0x{:08x} o:0x{:08x}", i, offset * 4, src, dst);
                        }
                        offset += 1;
                    }
                    // compare random data twice -- if read error, error locs will differ; if write error,
                    // errors locs are identical
                    offset = 0;
                    for (&src, &dst) in testsrc_slice.iter().zip(testdst_slice.iter()) {
                        dummy += src;
                        if dst != src {
                            dummy += dst;
                            log::error!("* {:<4} | 0x{:08x}: e:0x{:08x} o:0x{:08x}", i, offset * 4, src, dst);
                            errs += 1;
                        }
                        if offset == c2 {
                            log::info!("  {:<4} | 0x{:08x}: e:0x{:08x} o:0x{:08x}", i, offset * 4, src, dst);
                        }
                        offset += 1;
                    }
                    // reseed
                    let reseed = trng.get_u32().unwrap();
                    for dat in testsrc_slice.iter_mut() {
                        *dat = *dat ^ reseed;
                        dummy += *dat;
                    }
                }
                log::info!("test completed, {} errors", errs);

                xous::syscall::unmap_memory(testsrc).expect("couldn't de-allocate test region");
                xous::syscall::unmap_memory(testdst).expect("couldn't de-allocate test region");

                xous::send_message(
                    callback_conn,
                    xous::Message::new_scalar(
                        CB_ID.load(Ordering::Relaxed) as usize,
                        errs as usize,
                        iters as usize,
                        dummy as usize,
                        0,
                    ),
                )
                .unwrap();
            }),
            Some(TestOp::Quit) => {
                log::info!("quitting memtest thread");
                break;
            }
            None => {
                log::error!("received unknown opcode");
            }
        }
    }
    xous::destroy_server(sid).unwrap();
}

impl<'a> ShellCmdApi<'a> for Memtest {
    cmd_api!(memtest);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "memest [test [iters]]";

        let mut tokens = &args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "test" => {
                    let iters = if let Some(iter_str) = tokens.next() {
                        match iter_str.parse::<u32>() {
                            Ok(i) => i,
                            Err(_) => 10,
                        }
                    } else {
                        10
                    };
                    let start = env.ticktimer.elapsed_ms();
                    self.start_time = Some(start);
                    xous::send_message(
                        self.memtest_cid,
                        xous::Message::new_scalar(
                            TestOp::StartBasic.to_usize().unwrap(),
                            iters as usize,
                            0,
                            0,
                            0,
                        ),
                    )
                    .unwrap();
                    write!(ret, "Starting memtest with {} iters", iters).unwrap();
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

        let mut ret = String::new();

        xous::msg_scalar_unpack!(msg, errs, iters, dummy, _, {
            let end = env.ticktimer.elapsed_ms();
            let elapsed: f32 = ((end - self.start_time.unwrap()) as f32) / iters as f32;
            write!(ret, "memtest finished: {} errs, {}ms/iter", errs, elapsed).unwrap();
            log::info!("dummy var: {}", dummy); // just make sure it's not optimized out for any reason!
        });
        Ok(Some(ret))
    }
}
