use core::mem::MaybeUninit;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use flatipc::{IntoIpc, Ipc};

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
use crate::util::slice_as_uninit;

static TRNG_CONN: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, flatipc::Ipc)]
#[repr(C)]
struct TrngBuf {
    pub data: [u32; 1020],
    pub len: u16,
}

fn ensure_trng_conn() {
    if TRNG_CONN.load(Ordering::SeqCst) == 0 {
        let xns = xous_names::XousNames::new().unwrap();
        TRNG_CONN.store(
            xns.request_connection_blocking("_TRNG manager_").expect("Can't connect to TRNG server"),
            Ordering::SeqCst,
        );
    }
}

pub fn getrandom_inner(dest: &mut [MaybeUninit<u8>]) -> Result<(), crate::error::Error> {
    if dest.is_empty() {
        return Ok(());
    }
    ensure_trng_conn();
    fill_bytes(dest);
    Ok(())
}

pub fn fill_buf(data: &mut [u32]) {
    let mut tb = TrngBuf { data: [0; 1020], len: 0 };
    assert!(data.len() <= tb.data.len());
    tb.len = data.len() as u16;
    let mut buf = tb.into_ipc();
    buf.lend_mut(TRNG_CONN.load(Ordering::SeqCst), 1 /* FillTrng */).unwrap();
    assert!(usize::from(buf.len) == data.len());
    let len = buf.len as usize;
    data.copy_from_slice(&buf.data[..len]);
}

pub fn next_u32() -> u32 {
    let response = xous::send_message(
        TRNG_CONN.load(Ordering::SeqCst),
        xous::Message::new_blocking_scalar(0 /* GetTrng */, 1 /* count */, 0, 0, 0),
    )
    .expect("TRNG|LIB: can't get_u32");
    if let xous::Result::Scalar2(trng, _) = response {
        trng as u32
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn next_u64() -> u64 {
    let response = xous::send_message(
        TRNG_CONN.load(Ordering::SeqCst),
        xous::Message::new_blocking_scalar(0 /* GetTrng */, 2 /* count */, 0, 0, 0),
    )
    .expect("TRNG|LIB: can't get_u32");
    if let xous::Result::Scalar2(lo, hi) = response {
        lo as u64 | ((hi as u64) << 32)
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn fill_bytes_via_next(dest: &mut [MaybeUninit<u8>]) {
    let mut left = dest;
    while left.len() >= 8 {
        let (l, r) = { left }.split_at_mut(8);
        left = r;
        let chunk: [u8; 8] = next_u64().to_ne_bytes();
        l.copy_from_slice(slice_as_uninit(&chunk));
    }
    let n = left.len();
    if n > 4 {
        let chunk: [u8; 8] = next_u64().to_ne_bytes();
        left.copy_from_slice(slice_as_uninit(&chunk[..n]));
    } else if n > 0 {
        let chunk: [u8; 4] = next_u32().to_ne_bytes();
        left.copy_from_slice(slice_as_uninit(&chunk[..n]));
    }
}

/// This implementation will try to fill bytes using the more efficient but smaller scalar messages,
/// until it becomes faster to use a memory message.
fn fill_bytes(dest: &mut [MaybeUninit<u8>]) {
    if dest.len() < 64 {
        fill_bytes_via_next(dest);
    } else {
        // big chunks handled here, using in-place transformations
        for chunk in dest.chunks_exact_mut(4096) {
            let chunk_u32 =
                unsafe { core::slice::from_raw_parts_mut(chunk.as_mut_ptr() as *mut u32, chunk.len() / 4) };
            fill_buf(chunk_u32);
        }
        // smaller chunks, we absorb the fill_buf routine above here so we amortize the cost of
        // initializing the empty 4k-page...
        let remainder = dest.chunks_exact_mut(4096).into_remainder();
        if remainder.len() != 0 {
            let mut tb = TrngBuf { data: [0; 1020], len: 0 };
            tb.len = if remainder.len() % 4 == 0 {
                (remainder.len() / 4) as u16
            } else {
                1 + (remainder.len() / 4) as u16
            };
            let mut buf = tb.into_ipc();
            buf.lend_mut(TRNG_CONN.load(Ordering::SeqCst), 1 /* FillTrng */).unwrap();

            // transform the whole buffer into a ret_u8 slice (including trailing zeroes)
            let ret_u8 =
                unsafe { core::slice::from_raw_parts(buf.data.as_ptr() as *mut u8, buf.data.len() * 4) };
            // we've allocated an extra remainder word to handle the last word overflow, if anything
            // we'll end up throwing away a couple of unused bytes, but better than copying zeroes!
            remainder.copy_from_slice(slice_as_uninit(&ret_u8[..remainder.len()]));
        }
    }
}
