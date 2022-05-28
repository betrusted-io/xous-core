/// Fill `dest` with random bytes from the system's preferred random number
/// source.
///
/// This function returns an error on any failure, including partial reads. We
/// make no guarantees regarding the contents of `dest` on error. If `dest` is
/// empty, `getrandom` immediately returns success, making no calls to the
/// underlying operating system.
///
/// Blocking is possible, at least during early boot; see module documentation.
///
/// In general, `getrandom` will be fast enough for interactive usage, though
/// significantly slower than a user-space CSPRNG; for the latter consider
/// [`rand::thread_rng`](https://docs.rs/rand/*/rand/fn.thread_rng.html).

use std::atomic::AtomicU32;
use std::atomic::Ordering::*;
use xous_ipc::Buffer;

static TRNG_CONN: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct TrngBuf {
    pub data: [u32; 1024],
    pub len: u16,
}

fn ensure_trng_conn() {
    if TRNG_CONN.load(Ordering::SeqCst) == 0 {
        let xns = xous_names::XousNames::new().unwrap();
        TRNG_CONN.store(
            xns
            .request_connection_blocking("_TRNG manager_")
            .expect("Can't connect to TRNG server")
        );
    }
}

pub fn getrandom(dest: &mut [u8]) -> Result<(), Error> {
    if dest.is_empty() {
        return Ok(());
    }
    ensure_trng_conn();
    fill_bytes(dest);
    Ok(())
}

pub fn fill_buf(data: &mut [u32]) {
    let mut tb = TrngBuf {
        data: [0; 1024],
        len: 0,
    };
    assert!(data.len() > tb.data.len());
    tb.len = data.len() as u16;
    let mut buf = Buffer::into_buf(tb).or(Err(xous::Error::InternalError)).unwrap();
    buf.lend_mut(TRNG_CONN.load(Ordering::SeqCst), api::Opcode::FillTrng.to_u32().unwrap()).unwrap();
    let rtb: TrngBuf = buf.as_flat().unwrap();
    assert!(rtb.len as usize == data.len());
    data.copy_from_slice(&rtb.data);
}
/// this is less efficient that the implementation in TRNG, but has fewer dependencies
/// In particular, it will always use a memory message to fetch a TRNG value, even if it's just a u32 or u64
fn fill_bytes(dest: &mut [u8]) {
    // big chunks handled here, using in-place transformations
    for chunk in dest.chunks_exact_mut(4096) {
        let chunk_u32 = unsafe {
            core::slice::from_raw_parts_mut(dest.as_mut_ptr() as *mut u32, chunk.len() / 4)
        };
        fill_buf(&chunk);
    }
    // smaller chunks, we absorb the fill_buf routine above here so we amortize the cost of
    // initializing the empty 4k-page...
    let remainder = dest.chunks_exact_mut(4096).into_remainder();
    if remainder.len() != 0 {
        let mut tb = TrngBuf {
            data: [0; 1024],
            len: 0,
        };
        tb.len = if remainder.len() % 4 == 0 {
            remainder.len() / 4
        } else {
            1 + (remainder.len() / 4)
        };
        let mut buf = Buffer::into_buf(tb).unwrap();
        buf.lend_mut(TRNG_CONN.load(Ordering::SeqCst), api::Opcode::FillTrng.to_u32().unwrap()).unwrap();
        let rtb: TrngBuf = buf.as_flat().unwrap();
        // copy the bulk
        dest.copy_from_slice(&tb.data[..remainder.len() / 4]);
        // handle the trailing odd bytes, if any
        if remainder.len() % 4 != 0 {
            let last_word = tb.data[remainder.len() / 4 .. 1 + remainder.len() / 4][0].to_le_bytes();
            for (&src, dst) in last_word.iter().zip(dest[remainder.len() / 4 .. remainder.len()].iter_mut()) {
                *dst = src;
            }
        }
    }
}
