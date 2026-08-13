use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use bao1x_hal::rram::Reram;
use core::convert::TryFrom;

mod shiftxor;
use crate::erase::shiftxor::ShiftXor;

use crate::SerialInteract;

/// Represents a contiguous block of memory to overwrite or read back.
trait MemRegion {
    /// Start address.
    fn start(&self) -> u32;
    /// End address.
    fn end(&self) -> u32;
    /// Write a slice to the start of the region. Returns a BadAlignment error if the update is not
    /// a multiple of the minimum update size.
    fn write_slice(&self, data: &[u8]) -> Result<(), xous::Error>;
    /// Clip off a portion of the start of the region.
    fn advance(&mut self, nbytes: usize) -> Result<(), xous::Error>;

    /// Total size of the region in bytes.
    fn len(&self) -> usize {
        (self.end() - self.start()) as usize
    }
    /// Returns a slice representing the region. The caller must ensure no one else owns this region
    /// for the duration of the slice's lifetime.
    unsafe fn as_slice(&self) -> &[u8] {
        core::slice::from_raw_parts(self.start() as *const u8, self.len())
    }
}

struct GenericMemRegion {
    start: u32,
    end: u32,
}

impl GenericMemRegion {
    fn new(start: usize, len: usize) -> Self {
        GenericMemRegion { start: start as u32, end: (start + len) as u32 }
    }
}

impl MemRegion for GenericMemRegion {
    fn start(&self) -> u32 {
        self.start
    }

    fn end(&self) -> u32 {
        self.end
    }

    fn write_slice(&self, data: &[u8]) -> Result<(), xous::Error> {
        if data.len() > self.len() {
            return Err(xous::Error::MemoryInUse);
        }
        // Safety: we need to ensure nothing else takes ownership of this memory during the erasure
        // process.
        let dst = unsafe { core::slice::from_raw_parts_mut(self.start as *mut u8, data.len()) };
        dst.copy_from_slice(data);

        // Read back to check that the write worked.
        bao1x_hal::cache_flush();
        if dst != data {
            return Err(xous::Error::AccessDenied);
        }
        Ok(())
    }

    fn advance(&mut self, nbytes: usize) -> Result<(), xous::Error> {
        if self.len() < nbytes {
            Err(xous::Error::Unavailable)
        } else {
            self.start += nbytes as u32;
            Ok(())
        }
    }
}

struct ReramRegion {
    start: u32,
    end: u32,
}

impl ReramRegion {
    fn new(baremetal_rram_offset: u32) -> Result<Self, xous::Error> {
        // Get the writeable section of RRAM that is past boot1 and the specified range reserved for
        // the baremetal image. The baremetal code actually starts at an offset of
        // 1024 from BAREMETAL_START.
        let mut start = bao1x_api::BAREMETAL_START + 1024 + baremetal_rram_offset as usize;
        let end = utralib::HW_RERAM_MEM + bao1x_api::RRAM_STORAGE_LEN;
        if start % Erasure::KEY_BYTES != 0 {
            // If the start address is not a multiple of the key size, write some zeroes as padding.
            let offset = start as usize - utralib::HW_RERAM_MEM;
            let nbytes = Erasure::KEY_BYTES - offset % Erasure::KEY_BYTES;
            start += nbytes;
            let data = vec![0u8; nbytes];
            let mut rram = Reram::new();
            let len = rram.write_slice(offset, data.as_slice())?;
            if len != data.len() {
                return Err(xous::Error::InternalError);
            }
        }
        if end < start {
            return Err(xous::Error::ParseError);
        }
        Ok(ReramRegion { start: start as u32, end: end as u32 })
    }
}

impl MemRegion for ReramRegion {
    fn start(&self) -> u32 {
        self.start
    }

    fn end(&self) -> u32 {
        self.end
    }

    fn write_slice(&self, data: &[u8]) -> Result<(), xous::Error> {
        if data.len() > self.len() {
            return Err(xous::Error::MemoryInUse);
        }
        let offset = self.start as usize - utralib::HW_RERAM_MEM;
        let mut rram = Reram::new();
        let len = rram.write_slice(offset, data)?;
        if len != data.len() {
            return Err(xous::Error::InternalError);
        }
        Ok(())
    }

    fn advance(&mut self, nbytes: usize) -> Result<(), xous::Error> {
        if self.len() < nbytes {
            Err(xous::Error::Unavailable)
        } else {
            self.start += nbytes as u32;
            Ok(())
        }
    }

    unsafe fn as_slice(&self) -> &[u8] {
        core::slice::from_raw_parts(self.start as *const u8, self.len())
    }
}

/// Traverses through multiple non-contiguous memory blocks.
struct MemoryTraversal {
    idx: usize,
    blocks: Vec<Box<dyn MemRegion>>,
    // TODO: investigate/add mem regions from utralib/src/generated/bao1x.rs
}

macro_rules! mem {
    ( $start: ident, $len: ident ) => {
        GenericMemRegion::new(utralib::generated::$start, utralib::generated::$len)
    };
}

impl MemoryTraversal {
    fn new(baremetal_rram_offset: u32) -> Result<Self, xous::Error> {
        let mut blocks: Vec<Box<dyn MemRegion>> = Vec::new();
        blocks.push(Box::new(ReramRegion::new(baremetal_rram_offset)?));
        blocks.push(Box::new(mem!(HW_BIO_IMEM0_MEM, HW_BIO_IMEM0_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_BIO_IMEM1_MEM, HW_BIO_IMEM1_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_BIO_IMEM2_MEM, HW_BIO_IMEM2_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_BIO_IMEM3_MEM, HW_BIO_IMEM3_MEM_LEN)));
        // TODO: IFRAM0 works but might overwrite some USB stuff, needs further checking. IFRAM1
        // seems to block.
        // blocks.push(Box::new(mem!(HW_IFRAM0_MEM, HW_IFRAM0_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_IFRAM1_MEM, HW_IFRAM1_MEM_LEN)));
        Ok(MemoryTraversal { idx: 0, blocks })
    }

    /// Creates an empty erasure representing no memory.
    pub fn empty() -> Self {
        MemoryTraversal { idx: 0, blocks: Vec::new() }
    }

    fn len(&self) -> usize {
        let mut total = 0;
        for i in self.idx..self.blocks.len() {
            total += self.blocks[i].len();
        }
        return total;
    }

    fn peek(&self) -> u32 {
        self.blocks[self.idx].start()
    }

    fn advance_block(&mut self) -> Result<(), xous::Error> {
        if self.idx < self.blocks.len() - 1 {
            self.idx += 1;
            Ok(())
        } else {
            Err(xous::Error::OutOfMemory)
        }
    }

    fn write_slice(&mut self, data: &[u8]) -> Result<(), xous::Error> {
        if data.len() <= self.blocks[self.idx].len() {
            self.blocks[self.idx].write_slice(data)?;
            self.blocks[self.idx].advance(data.len())
        } else {
            let (head, tail) = data.split_at(self.blocks[self.idx].len());
            self.blocks[self.idx].write_slice(head)?;
            self.blocks[self.idx].advance(head.len())?;
            self.advance_block()?;
            self.write_slice(tail)
        }
    }

    fn all_blocks(&self) -> &[Box<dyn MemRegion>] {
        return &self.blocks;
    }
}

pub struct Erasure {
    traversal: MemoryTraversal,
    bytes_written: usize,
}

impl Erasure {
    // Determines the chunk size for ShiftXor.
    const KEY_BYTES: usize = 16;

    pub fn new(baremetal_rram_offset: u32) -> Result<Self, xous::Error> {
        Ok(Erasure { traversal: MemoryTraversal::new(baremetal_rram_offset)?, bytes_written: 0 })
    }

    /// Creates an empty erasure representing no memory.
    pub fn empty() -> Self {
        Erasure { traversal: MemoryTraversal::empty(), bytes_written: 0 }
    }

    /// Remaining length to fill.
    pub fn len(&self) -> usize {
        self.traversal.len()
    }

    /// Next address to fill.
    pub fn peek(&self) -> u32 {
        self.traversal.peek()
    }

    pub fn write_slice(&mut self, data: &[u8]) {
        self.traversal.write_slice(data).unwrap();
        self.bytes_written += data.len();
    }

    /// Recover the key from the ciphertext, shift seed, and key block.
    pub fn recover_key(&self, shift_seed: &[u8], key_block: &[u8]) -> Result<[u8; 16], xous::Error> {
        let mut shifter = ShiftXor::<{ Self::KEY_BYTES }>::new(shift_seed, key_block);
        // Start a traversal that *includes* the baremetal rram code.
        let reader = MemoryTraversal::new(0)?;
        for block in reader.all_blocks() {
            // safety: we need to ensure no one else takes ownership of this memory while we're
            // reading it. The program is single-threaded and memory is only allocated from SRAM.
            // TODO: when writing SRAM, make sure we can block off only a small section of it for
            // runtime allocations.
            let mem = unsafe { block.as_slice() };
            shifter.absorb(mem);
        }

        // Interpret the key as an array.
        <[u8; 16]>::try_from(shifter.key()).map_err(|_| xous::Error::InternalError)
    }
}

/// Convenience helper function for sending numbers over USB/UART.
fn send_u32(x: u32) {
    let uart = crate::debug::Uart {};
    for b in x.to_le_bytes() {
        uart.putc(b);
    }
}

enum State {
    NotStarted,
    Erase,
    GetSeed,
    GetKeyBlock,
    RecoverKey,
    Done,
}

/// Erasure variant designed to be run without interactivity; the data is sent as raw bytes without
/// a repl-like interface.
///
/// The expected interaction is:
/// 1. Host sends:
///    1a. 4 bytes indicating requested ack frequency in bytes.
///    1b. 4 bytes indicating the rram offset to start erasure from.
/// 2. Device sends 4 bytes indicating error code (0 = no error).
///    2a. If there was an error, the protocol does not continue.
/// 3. Device sends 4 bytes indicating requested total byte length.
/// 4. Repeat until total byte length is reached:
///    3a. Host sends <ack frequency> bytes, or remaining bytes if less.
///    3b. Device sends 4 bytes, encoding the total bytes received so far.
/// 5. Host sends the key and seed blocks.
/// 6. Device sends the recovered key.
///
/// Each party must wait for the other's messages before proceeding. For example, the host cannot
/// keep sending bytes without getting an ack in step 3. This prevents situations where due to
/// different clock frequencies, one party can fill a serial buffer much faster than the other one
/// can empty it.
pub struct OneShotErasure {
    state: State,
    erasure: Erasure,
    rx: Vec<u8>,
    seed: Vec<u8>,
    key_block: Vec<u8>,
    bytes_written: usize,
    bytes_to_fill: usize,
    last_ack: usize,
    ack_stride: usize,
}

impl OneShotErasure {
    // Determines how often we actually write the data. Buffering more data causes more stack usage;
    // buffering less incurs more overhead and internal buffering in the erase procedure.
    const WRITE_INTERVAL: usize = 256;

    // Sizes of seed and key block.
    const SEED_BYTES: usize = 16;
    const KEY_BYTES: usize = 16;

    pub fn new() -> Self {
        Self {
            state: State::NotStarted,
            erasure: Erasure::empty(),
            seed: Vec::with_capacity(Self::SEED_BYTES),
            key_block: Vec::with_capacity(Self::KEY_BYTES),
            rx: Vec::with_capacity(Self::WRITE_INTERVAL),
            bytes_written: 0,
            bytes_to_fill: 0,
            ack_stride: 0,
            last_ack: 0,
        }
    }
}

impl SerialInteract for OneShotErasure {
    fn rx_char(&mut self, c: u8) {
        match self.state {
            State::GetSeed => {
                self.seed.push(c);
            }
            State::GetKeyBlock => {
                self.key_block.push(c);
            }
            _ => {
                self.rx.push(c);
            }
        }
    }

    fn process(&mut self) {
        match &self.state {
            State::NotStarted => {
                if self.rx.len() >= 8 {
                    let (chunks, _) = self.rx.as_chunks::<4>();
                    let stride = u32::from_le_bytes(chunks[0]);
                    let rram_offset = u32::from_le_bytes(chunks[1]);
                    self.ack_stride = stride as usize;
                    self.rx.clear();
                    match Erasure::new(rram_offset) {
                        Ok(erasure) => {
                            self.state = State::Erase;
                            self.bytes_to_fill = erasure.len();
                            self.erasure = erasure;
                            send_u32(0); // "no error" code
                            send_u32(self.bytes_to_fill as u32);
                        }
                        Err(e) => {
                            send_u32(e.to_usize() as u32);
                        }
                    }
                }
            }
            State::Erase => {
                if self.rx.len() >= Self::WRITE_INTERVAL
                    || self.rx.len() >= self.last_ack + self.ack_stride
                    || self.rx.len() >= self.bytes_to_fill
                {
                    let src = &self.rx[..self.rx.len().min(self.bytes_to_fill)];
                    self.erasure.write_slice(src);
                    self.bytes_written += src.len();
                    self.bytes_to_fill -= src.len();
                    self.rx.clear();
                    if self.bytes_written >= self.last_ack + self.ack_stride {
                        send_u32(self.bytes_written as u32);
                        self.last_ack += self.ack_stride;
                    }
                    if self.bytes_to_fill == 0 {
                        self.state = State::GetSeed;
                    }
                }
            }
            State::GetSeed => {
                if self.seed.len() == Self::SEED_BYTES {
                    self.state = State::GetKeyBlock;
                }
            }
            State::GetKeyBlock => {
                if self.key_block.len() == Self::KEY_BYTES {
                    self.state = State::RecoverKey;
                }
            }
            State::RecoverKey => {
                // Perform key recovery.
                let key: [u8; 16] =
                    self.erasure.recover_key(self.seed.as_slice(), self.key_block.as_slice()).unwrap();

                // send the key to the host; despite the name, Uart::putc sends over USB if possible
                let uart = crate::debug::Uart {};
                for b in key {
                    uart.putc(b);
                }
                self.state = State::Done;
            }
            State::Done => (),
        }
    }
}
