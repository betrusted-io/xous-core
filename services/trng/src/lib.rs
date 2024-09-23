#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::{TRNG_TEST_BUF_LEN, TrngTestMode};
use flatipc::{IntoIpc, Ipc};
use num_traits::*;
// the 0.5.1 API is necessary for compatibility with curve25519-dalek crates
use rand_core::{CryptoRng, RngCore};
use xous::{CID, send_message};
use xous_ipc::Buffer;

#[derive(Debug)]
pub struct Trng {
    conn: CID,
    error_sid: Option<xous::SID>,
}
impl Trng {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(api::SERVER_NAME_TRNG).expect("Can't connect to TRNG server");
        Ok(Trng { conn, error_sid: None })
    }

    pub fn get_u32(&self) -> Result<u32, xous::Error> {
        let response = send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                api::Opcode::GetTrng.to_usize().unwrap(),
                1, /* count */
                0,
                0,
                0,
            ),
        )
        .expect("TRNG|LIB: can't get_u32");
        if let xous::Result::Scalar2(trng, _) = response {
            Ok(trng as u32)
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn get_u64(&self) -> Result<u64, xous::Error> {
        let response = send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                api::Opcode::GetTrng.to_usize().unwrap(),
                2, /* count */
                0,
                0,
                0,
            ),
        )
        .expect("TRNG|LIB: can't get_u32");
        if let xous::Result::Scalar2(lo, hi) = response {
            Ok(lo as u64 | ((hi as u64) << 32))
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }

    pub fn fill_buf(&self, data: &mut [u32]) -> Result<(), xous::Error> {
        let mut tb = api::TrngBuf { data: [0; 1020], len: 0 };
        if data.len() > tb.data.len() {
            return Err(xous::Error::OutOfMemory);
        }
        tb.len = data.len() as u16;
        let mut buf = tb.into_ipc();
        buf.lend_mut(self.conn, api::Opcode::FillTrng.to_usize().unwrap())
            .or(Err(xous::Error::InternalError))?;
        if buf.len as usize != data.len() {
            return Err(xous::Error::InternalError);
        }
        data.copy_from_slice(&buf.data[..data.len()]);
        Ok(())
    }

    pub fn hook_error_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.error_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.error_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(
                error_cb_server,
                sid_tuple.0 as usize,
                sid_tuple.1 as usize,
                sid_tuple.2 as usize,
                sid_tuple.3 as usize,
            )
            .unwrap();
            let hookdata = api::ScalarHook { sid: sid_tuple, id, cid };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, api::Opcode::ErrorSubscribe.to_u32().unwrap()).map(|_| ())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }

    pub fn get_health_tests(&self) -> Result<api::HealthTests, xous::Error> {
        let ht = api::HealthTests::default();
        let mut buf = Buffer::into_buf(ht).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, api::Opcode::HealthStats.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        Ok(buf.to_original().unwrap())
    }

    pub fn get_error_stats(&self) -> Result<api::TrngErrors, xous::Error> {
        let errs = api::TrngErrors::default();
        let mut buf = Buffer::into_buf(errs).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, api::Opcode::ErrorStats.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        Ok(buf.to_original().unwrap())
    }

    /// This is copied out of the 0.5 API for rand_core
    pub fn fill_bytes_via_next(&mut self, dest: &mut [u8]) {
        use core::mem::transmute;
        let mut left = dest;
        while left.len() >= 8 {
            let (l, r) = { left }.split_at_mut(8);
            left = r;
            let chunk: [u8; 8] = unsafe { transmute(self.next_u64().to_le()) };
            l.copy_from_slice(&chunk);
        }
        let n = left.len();
        if n > 4 {
            let chunk: [u8; 8] = unsafe { transmute(self.next_u64().to_le()) };
            left.copy_from_slice(&chunk[..n]);
        } else if n > 0 {
            let chunk: [u8; 4] = unsafe { transmute(self.next_u32().to_le()) };
            left.copy_from_slice(&chunk[..n]);
        }
    }

    /// Sets the test mode according to the argument. Blocks until mode is set.
    pub fn set_test_mode(&self, test_mode: TrngTestMode) {
        send_message(
            self.conn,
            xous::Message::new_blocking_scalar(
                api::Opcode::TestSetMode.to_usize().unwrap(),
                test_mode.to_usize().unwrap(),
                0,
                0,
                0,
            ),
        )
        .expect("TRNG|LIB: can't set test mode");
    }

    /// Gets test data from the TRNG. If the test mode was not previously set, this will
    /// eventually cause a panic. We don't add extra overhead code to make this safer
    /// because as a test mode the caller expected to know what they are doing (and adding
    /// more safety code increases overhead for the 99.9999999% of the time when we aren't
    /// using this test code).
    pub fn get_test_data(&self) -> Result<[u8; TRNG_TEST_BUF_LEN], xous::Error> {
        let tb = api::TrngTestBuf { data: [0; TRNG_TEST_BUF_LEN], len: 0 };
        let mut buf = Buffer::into_buf(tb).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, api::Opcode::TestGetData.to_u32().unwrap())
            .or(Err(xous::Error::InternalError))?;
        let rtb: api::TrngTestBuf = buf.to_original().unwrap();
        if rtb.len as usize != TRNG_TEST_BUF_LEN {
            // we somehow couldn't get enough test data from the TRNG.
            Err(xous::Error::OutOfMemory)
        } else {
            Ok(rtb.data)
        }
    }
}

impl RngCore for Trng {
    // legacy (0.5) trng apis
    fn next_u32(&mut self) -> u32 { self.get_u32().expect("couldn't get random u32 from server") }

    fn next_u64(&mut self) -> u64 { self.get_u64().expect("couldn't get random u64 from server") }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        // smaller than 64 bytes (512 bits), just use 8x next_u64 calls to fill.
        if dest.len() < 64 {
            return self.fill_bytes_via_next(dest);
        }
        // one page is the max size we can do with the fill_buf() API
        let chunks_page = dest.chunks_exact_mut(4096);
        for chunks in chunks_page.into_iter() {
            let mut data: [u32; 4080 / 4] = [0; 4080 / 4];
            self.fill_buf(&mut data).expect("couldn't fill page-sized TRNG buffer");
            for (&src, dst) in data.iter().zip(chunks.chunks_exact_mut(4)) {
                for (&src_byte, dst_byte) in src.to_le_bytes().iter().zip(dst.iter_mut()) {
                    *dst_byte = src_byte;
                }
            }
        }
        // a mid-sized chunk to span the gap between page and our smallest granularity
        let chunks_512 = dest.chunks_exact_mut(4096).into_remainder().chunks_exact_mut(512);
        for chunks in chunks_512.into_iter() {
            let mut data: [u32; 512 / 4] = [0; 512 / 4];
            self.fill_buf(&mut data).expect("couldn't fill mid-sized TRNG buffer");
            for (&src, dst) in data.iter().zip(chunks.chunks_exact_mut(4)) {
                for (&src_byte, dst_byte) in src.to_le_bytes().iter().zip(dst.iter_mut()) {
                    *dst_byte = src_byte;
                }
            }
        }
        // our smallest-sized "standard" chunk
        let chunks_smallest = dest
            .chunks_exact_mut(4096)
            .into_remainder()
            .chunks_exact_mut(512)
            .into_remainder()
            .chunks_exact_mut(64);
        for chunks in chunks_smallest.into_iter() {
            let mut data: [u32; 64 / 4] = [0; 64 / 4];
            self.fill_buf(&mut data).expect("couldn't fill small-sized TRNG buffer");
            for (&src, dst) in data.iter().zip(chunks.chunks_exact_mut(4)) {
                for (&src_byte, dst_byte) in src.to_le_bytes().iter().zip(dst.iter_mut()) {
                    *dst_byte = src_byte;
                }
            }
        }
        // any leftover bytes
        let leftovers = dest
            .chunks_exact_mut(4096)
            .into_remainder()
            .chunks_exact_mut(512)
            .into_remainder()
            .chunks_exact_mut(64)
            .into_remainder();
        self.fill_bytes_via_next(leftovers);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        Ok(self.fill_bytes(dest))
    }
}

impl CryptoRng for Trng {}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Trng {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the
        // connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}

fn error_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::EventCallback::Event) => xous::msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32, xous::Message::new_scalar(id, 0, 0, 0, 0)).unwrap();
            }),
            Some(api::EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
