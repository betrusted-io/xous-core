//! Baosec's TRNG implementation blends entropy from an external TRNG generator
//! (nominally an avalanche TRNG) to supplement the on-chip ring oscillator TRNG.
//! This belt-and-suspenders approach ensures high quality entropy even if one
//! of the entropy sources experiences some sort of unforeseen wear-out mechanism,
//! or in particular if the RO TRNG has some sort of process-based bias drift
//! over lot-to-lot variation that causes the output quality to degrade in a way
//! that is hard to measure/characterize on every unit produced, or if there is
//! some sort of phase locking mechanism that is subtle and hard to see with just
//! Dieharder runs but maybe a more advanced analytical technique can find it.
//!
//! Using the Avalanche TRNG requires access to the BIO, and thus a slower
//! (compared to Dabao's kernel-direct path), server-based mechanism for
//! generating random numbers is needed.

use flatipc::{IntoIpc, Ipc};
use num_traits::*;
// the 0.5.1 API is necessary for compatibility with curve25519-dalek crates
use rand_core::{CryptoRng, RngCore};
use xous::{CID, send_message};
use xous_ipc::Buffer;

use crate::trng::api;

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
        let mut left = dest;
        while left.len() >= 8 {
            let (l, r) = { left }.split_at_mut(8);
            left = r;
            let chunk: [u8; 8] = self.next_u64().to_ne_bytes();
            l.copy_from_slice(&chunk);
        }
        let n = left.len();
        if n > 4 {
            let chunk: [u8; 8] = self.next_u64().to_ne_bytes();
            left.copy_from_slice(&chunk[..n]);
        } else if n > 0 {
            let chunk: [u8; 4] = self.next_u32().to_ne_bytes();
            left.copy_from_slice(&chunk[..n]);
        }
    }

    /// This is a version that fills a buffer using a seeded CSPRNG. So every API call
    /// only grabs a smaller amount of entropy from the TRNG pool, and generates the rest
    /// from the seed.
    pub fn fill_seeded_bytes(&mut self, dest: &mut [u8]) {
        use rand_chacha::ChaCha8Rng;
        use rand_chacha::rand_core::RngCore;
        use rand_chacha::rand_core::SeedableRng;

        if dest.len() < 64 {
            self.fill_bytes_via_next(dest);
            return;
        }
        let mut seed = [0u8; 32];
        self.fill_bytes_via_next(&mut seed);
        let mut cstrng = ChaCha8Rng::from_seed(seed);
        cstrng.fill_bytes(dest);
    }
}

impl RngCore for Trng {
    // legacy (0.5) trng apis
    fn next_u32(&mut self) -> u32 { self.get_u32().expect("couldn't get random u32 from server") }

    fn next_u64(&mut self) -> u64 { self.get_u64().expect("couldn't get random u64 from server") }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        // smaller than 64 bytes (512 bits), just use 8x next_u64 calls to fill.
        if dest.len() < 64 {
            self.fill_bytes_via_next(dest);
            return;
        }
        // use the api TrngBuf to hold the data so the compiler enforces exact data lengths
        // the `len` field is actually disregarded here, we're simply using this structure to
        // ensure that the `data` allocated matches the native size of the TrngBuf.
        let mut api_buf = api::TrngBuf { data: [0u32; 1020], len: 1020 };
        let buf = &mut api_buf.data;
        let mut filled = 0;
        while filled < dest.len() {
            let fetch_len = ((dest.len() - filled) / size_of::<u32>()).min(buf.len());
            self.fill_buf(&mut buf[..fetch_len]).expect("couldn't fill temp buffer");
            for &w in buf[..fetch_len].iter() {
                let bytes = w.to_le_bytes();
                let remaining = dest.len() - filled;

                if remaining == 0 {
                    return;
                } else if remaining < 4 {
                    // Only need a tail that is not a multiple of 4
                    dest[filled..filled + remaining].copy_from_slice(&bytes[..remaining]);
                    return;
                } else {
                    // Can take a full word
                    dest[filled..filled + 4].copy_from_slice(&bytes);
                    filled += 4;
                }
            }
        }
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
