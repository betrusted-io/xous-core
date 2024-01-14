//! SHA-512
mod soft;
use core::sync::atomic::{AtomicU32, Ordering};

use num_traits::ToPrimitive;
use soft::compress;
use xous::{send_message, Message};
use xous_ipc::Buffer;

use crate::api::*;
use crate::consts::*;
/// we have to make the HW_CONN static because the Digest crate assumes you can clone objects
/// and recycle them. However, it's not a problem for every server to have a unique connection
/// to the hasher service, if that's what it comes down to. The burden for tracking connections is on the
/// connector's side, not on the server's side; so when the connecting process that calls this
/// library dies, this static data dies with it.
static HW_CONN: AtomicU32 = AtomicU32::new(0);
/// a unique-enough random ID number to prove we own our connection to the hashing engine hardware
static TOKEN: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];

use core::slice::from_ref;

use block_buffer::BlockBuffer;
use digest::consts::{U128, U28, U32, U48, U64};
use digest::generic_array::GenericArray;
use digest::{BlockInput, FixedOutputDirty, Reset, Update};
type BlockSize = U128;

/*
  Software emulation vendored from https://github.com/RustCrypto/hashes/tree/master/sha2/src
  License is Apache 2.0
*/
/// Structure that keeps state of the software-emulated Sha-512 operation and
/// contains the logic necessary to perform the final calculations.
#[derive(Clone)]
struct Engine512 {
    len: u128,
    buffer: BlockBuffer<BlockSize>,
    state: [u64; 8],
}

impl Engine512 {
    fn new(h: &[u64; STATE_LEN]) -> Engine512 { Engine512 { len: 0, buffer: Default::default(), state: *h } }

    fn update(&mut self, input: &[u8]) {
        self.len += (input.len() as u128) << 3;
        let s = &mut self.state;
        self.buffer.input_blocks(input, |b| compress512(s, b));
    }

    fn finish(&mut self) {
        let s = &mut self.state;
        self.buffer.len128_padding_be(self.len, |d| compress512(s, from_ref(d)));
    }

    fn reset(&mut self, h: &[u64; crate::consts::STATE_LEN]) {
        self.len = 0;
        self.buffer.reset();
        self.state = *h;
    }
}

// a macro for common communications libraries for SHA2 hardware interfacing
// you can't reference fields in a trait. looks like a macro is the accepted way
// of not having to repeat this code over and over again.
macro_rules! sha512_comms {
    () => {
        pub(crate) fn ensure_conn(&self) -> u32 {
            if HW_CONN.load(Ordering::Relaxed) == 0 {
                let xns = xous_names::XousNames::new().unwrap();
                HW_CONN.store(
                    xns.request_connection_blocking(crate::api::SERVER_NAME_SHA512)
                        .expect("Can't connect to Sha512 server"),
                    Ordering::Relaxed,
                );
                let trng = trng::Trng::new(&xns).expect("Can't connect to TRNG server");
                let id1 = trng.get_u64().unwrap();
                let id2 = trng.get_u32().unwrap();
                TOKEN[0].store((id1 >> 32) as u32, Ordering::Relaxed);
                TOKEN[1].store(id1 as u32, Ordering::Relaxed);
                TOKEN[2].store(id2, Ordering::Relaxed);
            }
            HW_CONN.load(Ordering::Relaxed)
        }
        pub fn is_idle(&self) -> Result<bool, xous::Error> {
            let response = send_message(
                self.ensure_conn(),
                Message::new_blocking_scalar(Opcode::IsIdle.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .expect("Couldn't make IsIdle query");
            if let xous::Result::Scalar1(result) = response {
                if result != 0 { Ok(true) } else { Ok(false) }
            } else {
                Err(xous::Error::InternalError)
            }
        }
        pub fn acquire_suspend_lock(&self) -> Result<bool, xous::Error> {
            let response = send_message(
                self.ensure_conn(),
                Message::new_blocking_scalar(Opcode::AcquireSuspendLock.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .expect("Couldn't issue AcquireSuspendLock message");
            if let xous::Result::Scalar1(result) = response {
                if result != 0 { Ok(true) } else { Ok(false) }
            } else {
                Err(xous::Error::InternalError)
            }
        }
        pub fn abort_suspend(&self) -> Result<(), xous::Error> {
            // we ignore the result and just turn it into () once we get anything back, as abort_suspend
            // "can't fail"
            send_message(
                self.ensure_conn(),
                Message::new_blocking_scalar(Opcode::AbortSuspendLock.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .map(|_| ())
        }
        pub(crate) fn try_acquire_hw(&mut self, config: Sha2Config) {
            if !self.in_progress && (self.strategy != FallbackStrategy::SoftwareOnly) {
                loop {
                    let conn = self.ensure_conn(); // also ensures the ID
                    let response = send_message(
                        conn,
                        Message::new_blocking_scalar(
                            Opcode::AcquireExclusive.to_usize().unwrap(),
                            TOKEN[0].load(Ordering::Relaxed) as usize,
                            TOKEN[1].load(Ordering::Relaxed) as usize,
                            TOKEN[2].load(Ordering::Relaxed) as usize,
                            config.to_usize().unwrap(),
                        ),
                    )
                    .expect("couldn't send AcquireExclusive message to Sha2 hardware!");
                    if let xous::Result::Scalar1(result) = response {
                        if result != 0 {
                            self.use_soft = false;
                            self.in_progress = true;
                            break;
                        } else {
                            if self.strategy == FallbackStrategy::HardwareThenSoftware {
                                self.use_soft = true;
                                self.in_progress = true;
                                break;
                            } else {
                                // this is hardware-exclusive mode, we block until we can get the hardware
                                xous::yield_slice();
                            }
                        }
                    } else {
                        log::error!("AcquireExclusive had an unexpected error: {:?}", response);
                        panic!("Internal error in AcquireExclusive");
                    }
                }
            } else if self.strategy == FallbackStrategy::SoftwareOnly {
                self.use_soft = true;
                self.in_progress = true;
            }
        }
        pub(crate) fn reset_hw(&mut self) {
            send_message(
                self.ensure_conn(),
                Message::new_blocking_scalar(
                    Opcode::Reset.to_usize().unwrap(),
                    TOKEN[0].load(Ordering::Relaxed) as usize,
                    TOKEN[1].load(Ordering::Relaxed) as usize,
                    TOKEN[2].load(Ordering::Relaxed) as usize,
                    0,
                ),
            )
            .expect("couldn't send reset to hardware");
            // reset internal flags
            self.length = 0;
            self.in_progress = false;
            self.use_soft = true;
        }
    };
}

/// The SHA-512 hash algorithm with the SHA-512 initial hash value.
#[derive(Clone)]
pub struct Sha512 {
    /// software fallback engine
    engine: Engine512,
    /// whether or not this current hasher instance will use software or hardware acceleration
    use_soft: bool,
    /// specifies the strategy for fallback in case multiple hashes are initiated simultaneously
    strategy: FallbackStrategy,
    /// track if a hash is in progress
    in_progress: bool,
    /// track the length of the message processed so far
    length: u64,
}
impl Sha512 {
    // make all the boilerplate comms code shared between all sizes of digest
    sha512_comms!();

    // use this function instead of default for more control over configuration of the hardware engine
    pub fn new() -> Self {
        Sha512 {
            use_soft: false,
            strategy: FallbackStrategy::HardwareThenSoftware,
            engine: Engine512::new(&H512),
            in_progress: false,
            length: 0,
        }
    }

    pub fn new_with_strategy(strat: FallbackStrategy) -> Self {
        Sha512 {
            use_soft: false,
            strategy: strat,
            engine: Engine512::new(&H512),
            in_progress: false,
            length: 0,
        }
    }
}

impl Default for Sha512 {
    fn default() -> Self {
        Sha512::new_with_strategy(FallbackStrategy::HardwareThenSoftware)
        // xns should Drop here and release the connection allocated by it automatically
    }
}

impl Drop for Sha512 {
    fn drop(&mut self) {
        // normally, we would de-allocate a connection but because the Digest API assumes that
        // all instances are fungible we can't do that, as the connection needs to be persistent
        // between invocations of the object.
        if !self.use_soft {
            self.reset_hw();
        }
    }
}

impl BlockInput for Sha512 {
    type BlockSize = BlockSize;
}

impl Update for Sha512 {
    fn update(&mut self, input: impl AsRef<[u8]>) {
        self.try_acquire_hw(Sha2Config::Sha512);
        // split the incoming blocks to page size and send to the engine
        if self.use_soft {
            self.engine.update(input.as_ref());
        } else {
            for chunk in input.as_ref().chunks(3968) {
                // one SHA512 block (128 bytes) short of 4096 to give space for struct overhead in page remap
                // handling
                let mut update = Sha2Update {
                    id: [
                        TOKEN[0].load(Ordering::Relaxed),
                        TOKEN[1].load(Ordering::Relaxed),
                        TOKEN[2].load(Ordering::Relaxed),
                    ],
                    buffer: [0; 3968],
                    len: 0,
                };
                self.length += (chunk.len() as u64) * 8; // we need to keep track of length in bits
                for (&src, dest) in chunk.iter().zip(&mut update.buffer) {
                    *dest = src;
                }
                update.len = chunk.len() as u16;
                let buf = Buffer::into_buf(update).expect("couldn't map chunk into IPC buffer");
                buf.lend(self.ensure_conn(), Opcode::Update.to_u32().unwrap())
                    .expect("hardware rejected our hash chunk!");
            }
        }
    }
}

impl FixedOutputDirty for Sha512 {
    type OutputSize = U64;

    fn finalize_into_dirty(&mut self, out: &mut digest::Output<Self>) {
        if self.use_soft {
            self.engine.finish();
            let s = self.engine.state;
            for (chunk, v) in out.chunks_exact_mut(8).zip(s.iter()) {
                chunk.copy_from_slice(&v.to_be_bytes());
            }
        } else {
            let result = Sha2Finalize {
                id: [
                    TOKEN[0].load(Ordering::Relaxed),
                    TOKEN[1].load(Ordering::Relaxed),
                    TOKEN[2].load(Ordering::Relaxed),
                ],
                result: Sha2Result::Uninitialized,
                length_in_bits: None,
            };
            let mut buf = Buffer::into_buf(result).expect("couldn't map memory for the return buffer");
            buf.lend_mut(self.ensure_conn(), Opcode::Finalize.to_u32().unwrap()).expect("couldn't finalize");

            let returned: Sha2Finalize = buf.to_original().expect("couldn't decode return buffer");
            match returned.result {
                Sha2Result::Sha512Result(s) => {
                    log::debug!("bits hashed: {}", self.length);
                    if self.length
                        != returned.length_in_bits.expect("hardware did not return a length field!")
                    {
                        panic!("Sha512 hardware did not hash as many bits as we had expected!")
                    }
                    for (dest, &src) in out.chunks_exact_mut(1).zip(s.iter()) {
                        dest.copy_from_slice(&[src])
                    }
                }
                Sha2Result::Sha512Trunc256Result(_) => {
                    panic!("Sha512 hardware returned the wrong type of buffer!");
                }
                Sha2Result::SuspendError => {
                    panic!("Hardware was suspended during Sha512 operation, result is invalid.");
                }
                Sha2Result::Uninitialized => {
                    panic!("Hardware didn't copy Sha512 hash result to the return buffer.");
                }
                Sha2Result::IdMismatch => {
                    panic!("Hardware is not currently processing our block, finalize call has no meaning.");
                }
            }
        }
    }
}

impl Reset for Sha512 {
    fn reset(&mut self) {
        if self.use_soft {
            self.engine.reset(&H512);
        } else {
            self.reset_hw();
        }
    }
}

/// The SHA-512 hash algorithm with the SHA-512/256 initial hash value. The
/// result is truncated to 256 bits.

/// TODO: this is a software-only implementation; this needs to be extended to have hardware
/// support as the constants for this mode are supported in-hardware, once we have validated
/// that the core hardware API even works...
#[derive(Clone)]
pub struct Sha512Trunc256 {
    engine: Engine512,
    /// whether or not this current hasher instance will use software or hardware acceleration
    use_soft: bool,
    /// specifies the strategy for fallback in case multiple hashes are initiated simultaneously
    strategy: FallbackStrategy,
    /// track if a hash is in progress
    in_progress: bool,
    /// track the length of the message processed so far
    length: u64,
}
impl Sha512Trunc256 {
    // make all the boilerplate comms code shared between all sizes of digest
    sha512_comms!();

    // use this function instead of default for more control over configuration of the hardware engine
    pub fn new() -> Self {
        Sha512Trunc256 {
            use_soft: false,
            strategy: FallbackStrategy::HardwareThenSoftware,
            engine: Engine512::new(&H512_TRUNC_256),
            in_progress: false,
            length: 0,
        }
    }

    pub fn new_with_strategy(strat: FallbackStrategy) -> Self {
        Sha512Trunc256 {
            use_soft: false,
            strategy: strat,
            engine: Engine512::new(&H512_TRUNC_256),
            in_progress: false,
            length: 0,
        }
    }
}

impl Drop for Sha512Trunc256 {
    fn drop(&mut self) {
        if !self.use_soft {
            self.reset_hw();
        }
    }
}

impl Default for Sha512Trunc256 {
    fn default() -> Self { Sha512Trunc256::new_with_strategy(FallbackStrategy::HardwareThenSoftware) }
}

impl BlockInput for Sha512Trunc256 {
    type BlockSize = BlockSize;
}

impl Update for Sha512Trunc256 {
    fn update(&mut self, input: impl AsRef<[u8]>) {
        self.try_acquire_hw(Sha2Config::Sha512Trunc256);
        if self.use_soft {
            self.engine.update(input.as_ref());
        } else {
            for chunk in input.as_ref().chunks(3968) {
                // one SHA512 block (128 bytes) short of 4096 to give space for struct overhead in page remap
                // handling
                let mut update = Sha2Update {
                    id: [
                        TOKEN[0].load(Ordering::Relaxed),
                        TOKEN[1].load(Ordering::Relaxed),
                        TOKEN[2].load(Ordering::Relaxed),
                    ],
                    buffer: [0; 3968],
                    len: 0,
                };
                self.length += (chunk.len() as u64) * 8; // we need to keep track of length in bits
                for (&src, dest) in chunk.iter().zip(&mut update.buffer) {
                    *dest = src;
                }
                update.len = chunk.len() as u16;
                let buf = Buffer::into_buf(update).expect("couldn't map chunk into IPC buffer");
                buf.lend(self.ensure_conn(), Opcode::Update.to_u32().unwrap())
                    .expect("hardware rejected our hash chunk!");
            }
        }
    }
}

impl FixedOutputDirty for Sha512Trunc256 {
    type OutputSize = U32;

    fn finalize_into_dirty(&mut self, out: &mut digest::Output<Self>) {
        if self.use_soft {
            self.engine.finish();
            let s = &self.engine.state[..4];
            for (chunk, v) in out.chunks_exact_mut(8).zip(s.iter()) {
                chunk.copy_from_slice(&v.to_be_bytes());
            }
        } else {
            let result = Sha2Finalize {
                id: [
                    TOKEN[0].load(Ordering::Relaxed),
                    TOKEN[1].load(Ordering::Relaxed),
                    TOKEN[2].load(Ordering::Relaxed),
                ],
                result: Sha2Result::Uninitialized,
                length_in_bits: None,
            };
            let mut buf = Buffer::into_buf(result).expect("couldn't map memory for the return buffer");
            buf.lend_mut(self.ensure_conn(), Opcode::Finalize.to_u32().unwrap()).expect("couldn't finalize");

            let returned: Sha2Finalize = buf.to_original().expect("couldn't decode return buffer");
            match returned.result {
                Sha2Result::Sha512Trunc256Result(s) => {
                    if self.length
                        != returned.length_in_bits.expect("hardware did not return a length field!")
                    {
                        panic!("Sha512 hardware did not hash as many bits as we had expected!")
                    }
                    for (dest, &src) in out.chunks_exact_mut(1).zip(s.iter()) {
                        dest.copy_from_slice(&[src])
                    }
                }
                Sha2Result::Sha512Result(_) => {
                    panic!("Sha512 hardware returned the wrong type of buffer!");
                }
                Sha2Result::SuspendError => {
                    panic!("Hardware was suspended during Sha512 operation, result is invalid.");
                }
                Sha2Result::Uninitialized => {
                    panic!("Hardware didn't copy Sha512 hash result to the return buffer.");
                }
                Sha2Result::IdMismatch => {
                    panic!("Hardware is not currently processing our block, finalize call has no meaning.");
                }
            }
        }
    }
}

impl Reset for Sha512Trunc256 {
    fn reset(&mut self) {
        if self.use_soft {
            self.engine.reset(&H512_TRUNC_256);
        } else {
            self.reset_hw();
        }
    }
}

//////////////////////////////////////////////
//////////////// the following modes are software-only emulation, as we currently do not have the constants in
//////////////// hardware to support these. //////////////////////////////

/// The SHA-512 hash algorithm with the SHA-384 initial hash value. The result
/// is truncated to 384 bits.
#[derive(Clone)]
pub struct Sha384 {
    engine: Engine512,
}

impl Default for Sha384 {
    fn default() -> Self { Sha384 { engine: Engine512::new(&H384) } }
}

impl BlockInput for Sha384 {
    type BlockSize = BlockSize;
}

impl Update for Sha384 {
    fn update(&mut self, input: impl AsRef<[u8]>) { self.engine.update(input.as_ref()); }
}

impl FixedOutputDirty for Sha384 {
    type OutputSize = U48;

    fn finalize_into_dirty(&mut self, out: &mut digest::Output<Self>) {
        self.engine.finish();
        let s = &self.engine.state[..6];
        for (chunk, v) in out.chunks_exact_mut(8).zip(s.iter()) {
            chunk.copy_from_slice(&v.to_be_bytes());
        }
    }
}

impl Reset for Sha384 {
    fn reset(&mut self) { self.engine.reset(&H384); }
}

/// The SHA-512 hash algorithm with the SHA-512/224 initial hash value.
/// The result is truncated to 224 bits.
#[derive(Clone)]
pub struct Sha512Trunc224 {
    engine: Engine512,
}

impl Default for Sha512Trunc224 {
    fn default() -> Self { Sha512Trunc224 { engine: Engine512::new(&H512_TRUNC_224) } }
}

impl BlockInput for Sha512Trunc224 {
    type BlockSize = BlockSize;
}

impl Update for Sha512Trunc224 {
    fn update(&mut self, input: impl AsRef<[u8]>) { self.engine.update(input.as_ref()); }
}

impl FixedOutputDirty for Sha512Trunc224 {
    type OutputSize = U28;

    fn finalize_into_dirty(&mut self, out: &mut digest::Output<Self>) {
        self.engine.finish();
        let s = &self.engine.state;
        for (chunk, v) in out.chunks_exact_mut(8).zip(s[..3].iter()) {
            chunk.copy_from_slice(&v.to_be_bytes());
        }
        out[24..28].copy_from_slice(&s[3].to_be_bytes()[..4]);
    }
}

impl Reset for Sha512Trunc224 {
    fn reset(&mut self) { self.engine.reset(&H512_TRUNC_224); }
}

opaque_debug::implement!(Sha384);
opaque_debug::implement!(Sha512);
opaque_debug::implement!(Sha512Trunc224);
opaque_debug::implement!(Sha512Trunc256);

digest::impl_write!(Sha384);
digest::impl_write!(Sha512);
digest::impl_write!(Sha512Trunc224);
digest::impl_write!(Sha512Trunc256);

/// Raw SHA-512 compression function.
///
/// This is a low-level "hazmat" API which provides direct access to the core
/// functionality of SHA-512.
#[cfg_attr(docsrs, doc(cfg(feature = "compress")))]
pub fn compress512(state: &mut [u64; 8], blocks: &[GenericArray<u8, U128>]) {
    // SAFETY: GenericArray<u8, U128> and [u8; 128] have
    // exactly the same memory layout
    #[allow(unsafe_code)]
    let blocks: &[[u8; 128]] = unsafe { &*(blocks as *const _ as *const [[u8; 128]]) };
    compress(state, blocks)
}
