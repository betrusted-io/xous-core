use alloc::boxed::Box;
use alloc::vec::Vec;
use core::convert::TryFrom;

use bao1x_hal::rram::Reram;
use bao1x_hal::udma::UartChannel;
use bao1x_hal::udma::UartIrq;
use hex;

mod shiftxor;
use crate::SerialInteract;
use crate::erase::shiftxor::ShiftXor;
use crate::platform;

/// Represents a contiguous block of memory to overwrite or read back.
trait MemRegion {
    /// Start address.
    fn start(&self) -> u32;
    /// End address.
    fn end(&self) -> u32;
    /// Write a slice to the start of the region.
    fn write_slice(&self, data: &[u8]) -> Result<(), xous::Error>;
    /// Clip off a portion of the start of the region.
    fn advance(&mut self, nbytes: usize) -> Result<(), xous::Error>;

    /// Total size of the region in bytes.
    fn len(&self) -> usize { (self.end() - self.start()) as usize }
    /// Returns a slice representing the region. The caller must ensure no one else owns this region
    /// for the duration of the slice's lifetime.
    unsafe fn as_slice(&self) -> &[u8] { core::slice::from_raw_parts(self.start() as *const u8, self.len()) }
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
    fn start(&self) -> u32 { self.start }

    fn end(&self) -> u32 { self.end }

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
    // Empirically, the baremetal code actually starts at an offset of
    // 1024 from BAREMETAL_START. (TODO: what is there and can we overwrite it?)
    const CODE_START: usize = bao1x_api::BAREMETAL_START + 1024;

    /// Get the remainder of RRAM that past a certain start offset from Self::CODE_START.  The
    /// offset must be aligned to the erasure key size.
    fn new(start_offset: usize) -> Result<Self, xous::Error> {
        let start = Self::CODE_START + start_offset;
        let end = utralib::HW_RERAM_MEM + bao1x_api::RRAM_STORAGE_LEN;
        if start_offset % Erasure::KEY_BYTES != 0 || end < start {
            return Err(xous::Error::ParseError);
        }
        Ok(ReramRegion { start: start as u32, end: end as u32 })
    }

    /// Get the region of RRAM that represents the text region of the loaded baremetal image.
    fn code(code_size: usize) -> Self {
        let start = Self::CODE_START;
        let end = start + code_size;
        ReramRegion { start: start as u32, end: end as u32 }
    }
}

impl MemRegion for ReramRegion {
    fn start(&self) -> u32 { self.start }

    fn end(&self) -> u32 { self.end }

    fn write_slice(&self, data: &[u8]) -> Result<(), xous::Error> {
        if data.len() > self.len() {
            return Err(xous::Error::MemoryInUse);
        }
        let offset = self.start as usize - utralib::HW_RERAM_MEM;
        let mut rram = Reram::new();
        // disable interrupts temporarily during the RRAM write; otherwise we interrupt to receive
        // new characters partway through and it causes havoc.
        let mut uart_irq = UartIrq::new();
        uart_irq.rx_irq_ena(UartChannel::Uart2, false);
        let len = rram.write_slice(offset, data)?;
        uart_irq.rx_irq_ena(UartChannel::Uart2, true);
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

    unsafe fn as_slice(&self) -> &[u8] { core::slice::from_raw_parts(self.start as *const u8, self.len()) }
}

/// Traverses through multiple non-contiguous memory blocks.
struct MemoryTraversal {
    idx: usize,
    blocks: Vec<Box<dyn MemRegion>>,
}

macro_rules! mem {
    ($start:ident, $len:ident) => {
        GenericMemRegion::new(utralib::generated::$start, utralib::generated::$len)
    };
}

impl MemoryTraversal {
    fn new(code_size: usize) -> Result<Self, xous::Error> {
        let mut blocks: Vec<Box<dyn MemRegion>> = Vec::new();

        // RRAM: get the region past the (block-aligned) end of the code.
        let rram_start_offset = if code_size % Erasure::KEY_BYTES == 0 {
            code_size
        } else {
            code_size - code_size % Erasure::KEY_BYTES + Erasure::KEY_BYTES
        };
        blocks.push(Box::new(ReramRegion::new(rram_start_offset)?));

        // SRAM: get the region past the stack, heap, exception scratch, etc.
        let sram_start = platform::SRAM_RESERVED_END;
        let sram_len =
            utralib::HW_SRAM_MEM + utralib::HW_SRAM_MEM_LEN - platform::STACK_SIZE_BYTES - sram_start;
        blocks.push(Box::new(GenericMemRegion::new(sram_start, sram_len)));

        // TODO: erase the fictitious stack region?

        // IFRAM0: erase non-reserved pages
        let ifram0_start = utralib::HW_IFRAM0_MEM;
        let ifram0_len = bao1x_hal::board::IFRAM0_RESERVED_PAGE_RANGE[0] * 0x1000;
        blocks.push(Box::new(GenericMemRegion::new(ifram0_start, ifram0_len)));

        // dabao only uses IFRAM1 for USB; if USB is not enabled we can erase the whole thing.
        #[cfg(all(feature = "board-dabao", not(feature = "bao1x-usb")))]
        blocks.push(Box::new(mem!(HW_IFRAM1_MEM, HW_IFRAM1_MEM_LEN)));

        // otherwise, we have to be more careful I guess
        #[cfg(any(not(feature = "board-dabao"), feature = "bao1x-usb"))]
        {
            let ifram1_start = utralib::HW_IFRAM1_MEM;
            let ifram1_len = bao1x_hal::board::IFRAM1_RESERVED_PAGE_RANGE[0] * 0x1000;
            blocks.push(Box::new(GenericMemRegion::new(ifram1_start, ifram1_len)));
        }

        blocks.push(Box::new(mem!(HW_BIO_IMEM0_MEM, HW_BIO_IMEM0_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_BIO_IMEM1_MEM, HW_BIO_IMEM1_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_BIO_IMEM2_MEM, HW_BIO_IMEM2_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_BIO_IMEM3_MEM, HW_BIO_IMEM3_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_LKEY_MEM, HW_SEG_LKEY_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_KEY_MEM, HW_SEG_KEY_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_SKEY_MEM, HW_SEG_SKEY_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_SCRT_MEM, HW_SEG_SCRT_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_SOB_MEM, HW_SEG_SOB_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_PCON_MEM, HW_SEG_PCON_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_PKB_MEM, HW_SEG_PKB_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_PIB_MEM, HW_SEG_PIB_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_POB_MEM, HW_SEG_POB_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_PSOB_MEM, HW_SEG_PSOB_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_AKEY_MEM, HW_SEG_AKEY_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_AIB_MEM, HW_SEG_AIB_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_AOB_MEM, HW_SEG_AOB_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_RNGA_MEM, HW_SEG_RNGA_MEM_LEN)));
        blocks.push(Box::new(mem!(HW_SEG_RNGB_MEM, HW_SEG_RNGB_MEM_LEN)));

        // The following regions interfere with the key recovery process.
        //
        // blocks.push(Box::new(mem!(HW_SEG_MSG_MEM, HW_SEG_MSG_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_SEG_HOUT_MEM, HW_SEG_HOUT_MEM_LEN)));

        // The following regions seem to block on write. Perhaps they need extra intialization steps
        // to become writeable?
        //
        // blocks.push(Box::new(mem!(HW_BIO_BDMA_MEM, HW_BIO_BDMA_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_IFRAM1_MEM, HW_IFRAM1_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_BIO_FIFO0_MEM, HW_BIO_FIFO0_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_BIO_FIFO1_MEM, HW_BIO_FIFO1_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_BIO_FIFO2_MEM, HW_BIO_FIFO2_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_BIO_FIFO3_MEM, HW_BIO_FIFO3_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_CSR_MEM, HW_CSR_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_SCE_MEM, HW_SCE_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_SYSCTRL_MEM, HW_SYSCTRL_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_IFSUB_MEM, HW_IFSUB_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_CORESUB_MEM, HW_CORESUB_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_RRC_MEM, HW_RRC_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_SECSUB_MEM, HW_SECSUB_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_AO_MEM, HW_AO_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_AOPERI_MEM, HW_AOPERI_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_NULL_MEM, HW_NULL_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_UDMA_MEM, HW_UDMA_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_UDP_MEM, HW_UDP_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_SDDC_DAT_MEM, HW_SDDC_DAT_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_UDC_MEM, HW_UDC_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_XIP_MEM, HW_XIP_MEM_LEN))); // huge! what is this?
        // blocks.push(Box::new(mem!(HW_PL230_MEM, HW_PL230_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_MDMA_MEM, HW_MDMA_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_MBOX_APB_MEM, HW_MBOX_APB_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_IOX_MEM, HW_IOX_MEM_LEN)));
        // blocks.push(Box::new(mem!(HW_AORAM_MEM, HW_AORAM_MEM_LEN)));

        Ok(MemoryTraversal { idx: 0, blocks })
    }

    /// Creates an empty erasure representing no memory.
    pub fn empty() -> Self { MemoryTraversal { idx: 0, blocks: Vec::new() } }

    fn len(&self) -> usize {
        let mut total = 0;
        for i in self.idx..self.blocks.len() {
            total += self.blocks[i].len();
        }
        return total;
    }

    fn peek(&self) -> u32 { self.blocks[self.idx].start() }

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

    fn all_blocks(&self) -> &[Box<dyn MemRegion>] { return &self.blocks; }
}

pub struct Erasure {
    traversal: MemoryTraversal,
    code_size: usize,
    bytes_written: usize,
}

impl Erasure {
    // Determines the chunk size for ShiftXor.
    const KEY_BYTES: usize = 16;

    pub fn new(code_size: u32) -> Result<Self, xous::Error> {
        Ok(Erasure {
            traversal: MemoryTraversal::new(code_size as usize)?,
            code_size: code_size as usize,
            bytes_written: 0,
        })
    }

    /// Creates an empty erasure representing no memory.
    pub fn empty() -> Self { Erasure { traversal: MemoryTraversal::empty(), code_size: 0, bytes_written: 0 } }

    /// Remaining length to fill.
    pub fn len(&self) -> usize { self.traversal.len() }

    /// Next address to fill.
    pub fn peek(&self) -> u32 { self.traversal.peek() }

    pub fn write_slice(&mut self, data: &[u8]) {
        self.traversal.write_slice(data).unwrap();
        self.bytes_written += data.len();
    }

    /// Recover the key from the ciphertext, shift seed, and key block.
    pub fn recover_key(&self, shift_seed: &[u8], key_block: &[u8]) -> Result<[u8; 16], xous::Error> {
        let mut shifter = ShiftXor::<{ Self::KEY_BYTES }>::new(shift_seed, key_block);
        // Absorb the code region (with zero-padding for block alignment).
        let text_region = ReramRegion::code(self.code_size);
        // safety: this data should remain constant, so reading it is hopefully OK.
        let text_mem = unsafe { text_region.as_slice() };
        shifter.absorb_with_padding(text_mem);

        // Traverse the data the host wrote.
        let reader = MemoryTraversal::new(self.code_size)?;
        for block in reader.all_blocks() {
            // safety: we need to ensure no one else takes ownership of this memory while we're
            // reading it. The program is single-threaded and memory is only allocated from SRAM.
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
    let mut bs = [0u8; 8];
    hex::encode_to_slice(x.to_le_bytes(), &mut bs).unwrap();
    for b in bs {
        uart.putc(b);
    }
}

enum State {
    NotStarted,
    Erase,
    GetSeed,
    RecoverKey,
    Done,
}

/// Erasure variant designed to be run without interactivity; the data is sent as raw bytes without
/// a repl-like interface.
///
/// The expected interaction is:
/// 1. Host sends: 1a. 4 bytes indicating requested ack frequency in bytes. 1b. 4 bytes indicating the rram
///    offset to start erasure from.
/// 2. Device sends 8 bytes* indicating error code (0 = no error). 2a. If there was an error, the protocol
///    does not continue.
/// 3. Device sends 8 bytes* indicating requested total byte length.
/// 4. Repeat until total byte length is reached: 4a. Host sends <ack frequency> bytes, or remaining bytes if
///    less. 4b. Device sends 8 bytes*, encoding the total bytes received so far.
/// 5. Host sends the key and seed blocks.
/// 6. Device sends the recovered key*.
///
/// *All device-sent values are encoded in ASCII hexadecimal. 32-bit values are encoded in
/// little-endian order.
///
/// Each party must wait for the other's messages before proceeding. For example, the host cannot
/// keep sending bytes without getting an ack in step 4. This prevents situations where due to
/// different clock frequencies, one party can fill a serial buffer much faster than the other one
/// can empty it.
///
/// Flow control should be implemented on the *host side only*. That is, the device does not listen
/// for XON/XOFF bytes, but it does send them and expect the host to respect them (otherwise UART
/// interrupts for new characters might get dropped in critical sections and rram writes, where
/// interrupts are disabled). Software flow control with XON/XOFF is not compatible with pure binary
/// data, since XON/XOFF bytes can occur by chance.  For this reason, data sent from the device to
/// the host is encoded as ascii hex characters, while the host sends pure binary data (this is
/// preferable for performance reasons, since the host sends a high volume of data).
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
    // Sizes of seed and key block.
    const KEY_BYTES: usize = 16;
    const SEED_BYTES: usize = 16;
    // Determines how often we actually write the data. Buffering more data causes more stack usage;
    // buffering less incurs more overhead and internal buffering in the erase procedure.
    const WRITE_INTERVAL: usize = 1024;

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
    fn rx_char(&mut self, c: u8) { self.rx.push(c); }

    fn process(&mut self) {
        match &self.state {
            State::NotStarted => {
                if self.rx.len() >= 8 {
                    let (chunks, _) = self.rx.as_chunks::<4>();
                    let stride = u32::from_le_bytes(chunks[0]);
                    let code_size = u32::from_le_bytes(chunks[1]);
                    self.ack_stride = stride as usize;
                    self.rx.clear();
                    match Erasure::new(code_size) {
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
                if self.rx.len() >= Self::WRITE_INTERVAL || self.rx.len() >= self.bytes_to_fill {
                    let nbytes = self.rx.len().min(self.bytes_to_fill);
                    self.erasure.write_slice(&self.rx[..nbytes]);
                    self.rx.clear();
                    self.bytes_written += nbytes;
                    self.bytes_to_fill -= nbytes;
                    if self.bytes_to_fill == 0 {
                        self.state = State::GetSeed;
                    }
                }
                let bytes_received = self.bytes_written + self.rx.len();
                if bytes_received >= self.last_ack + self.ack_stride || self.bytes_to_fill == 0 {
                    send_u32(bytes_received as u32);
                    self.last_ack += self.ack_stride;
                }
            }
            State::GetSeed => {
                if self.rx.len() == Self::SEED_BYTES + Self::KEY_BYTES {
                    self.seed.extend_from_slice(&self.rx[..Self::SEED_BYTES]);
                    self.key_block
                        .extend_from_slice(&self.rx[Self::SEED_BYTES..Self::SEED_BYTES + Self::KEY_BYTES]);
                    self.state = State::RecoverKey;
                }
            }
            State::RecoverKey => {
                // Perform key recovery.
                let key: [u8; 16] =
                    self.erasure.recover_key(self.seed.as_slice(), self.key_block.as_slice()).unwrap();

                // send the key to the host; despite the name, Uart::putc sends over USB if possible
                let mut key_hex = [0u8; Self::KEY_BYTES * 2];
                hex::encode_to_slice(key, &mut key_hex).unwrap();
                let uart = crate::debug::Uart {};
                for b in key_hex {
                    uart.putc(b);
                }
                self.state = State::Done;
            }
            State::Done => (),
        }
    }
}
