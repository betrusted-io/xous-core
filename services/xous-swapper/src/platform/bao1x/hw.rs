use core::fmt::Write;
use core::mem::size_of;
use std::cell::RefCell;
use std::convert::TryInto;

use aes_gcm_siv::{AeadInPlace, Aes256GcmSiv, Error, KeyInit, Nonce, Tag};
use bao1x_api::*;
use bao1x_hal::board::{SPIM_FLASH_IFRAM_ADDR, SPIM_RAM_IFRAM_ADDR};
use bao1x_hal::ifram::IframRange;
use bao1x_hal::udma::*;
use loader::swap::SwapSpec;
use xous::arch::SWAP_HAL_VADDR;

use crate::debug::*;

pub const PAGE_SIZE: usize = xous::arch::PAGE_SIZE;

/// This is an implementation for SMTs that are accessible only through a SPI
/// register interface. The base and bounds must be translated to SPI accesses
/// in a hardware-specific manner.
pub struct SwapHal {
    swap_mac_start: usize,
    cipher: Aes256GcmSiv,
    ram_spim: Spim,
    flash_spim: RefCell<Spim>,
    // pre-allocate space for merging writes to SPI erase sectors
    buffer: RefCell<[u8; FLASH_SECTOR_LEN]>,
}
impl SwapHal {
    pub fn new(spec: &SwapSpec) -> Self {
        writeln!(DebugUart {}, "Swap HAL init",).ok();

        // compute the MAC area needed for the total RAM size. This is a slight over-estimate
        // because once we remove the MAC area, we need even less storage, but it's a small error.
        let ram_size_actual = loader::swap::derive_usable_swap(spec.swap_len as usize);

        let channel = SpimChannel::Channel1;
        Self {
            swap_mac_start: ram_size_actual,
            cipher: Aes256GcmSiv::new((&spec.key).into()),
            // safety: this is safe because the global clocks were gated on by the bootloader
            // note that also the IFRAM0 range is pre-allocated by the bootloader, and pre-mapped
            // into the correct virtual address as well.
            ram_spim: unsafe {
                Spim::new_with_ifram(
                    channel,
                    25_000_000,
                    50_000_000,
                    SpimClkPol::LeadingEdgeRise,
                    SpimClkPha::CaptureOnLeading,
                    SpimCs::Cs1,
                    0,
                    0,
                    None,
                    1024, // this is limited by the page length
                    1024,
                    Some(6),
                    Some(SpimMode::Quad),
                    IframRange::from_raw_parts(SPIM_RAM_IFRAM_ADDR, SWAP_HAL_VADDR, PAGE_SIZE),
                )
            },
            flash_spim: RefCell::new(unsafe {
                Spim::new_with_ifram(
                    channel,
                    25_000_000,
                    50_000_000,
                    SpimClkPol::LeadingEdgeRise,
                    SpimClkPha::CaptureOnLeading,
                    SpimCs::Cs0,
                    0,
                    0,
                    None,
                    1024, // this is limited by the page length
                    4096,
                    Some(6),
                    Some(SpimMode::Quad), // we're in quad because the loader put us here
                    IframRange::from_raw_parts(SPIM_FLASH_IFRAM_ADDR, SPIM_FLASH_IFRAM_ADDR, PAGE_SIZE * 2),
                )
            }),
            buffer: RefCell::new([0u8; PAGE_SIZE]),
        }
    }

    /// `buf` contents are replaced with encrypted data
    pub fn encrypt_swap_to(
        &mut self,
        buf: &mut [u8],
        swap_count: u32,
        dest_offset: usize,
        src_vaddr: usize,
        src_pid: u8,
    ) {
        assert!(buf.len() == PAGE_SIZE);
        assert!(dest_offset & (PAGE_SIZE - 1) == 0);
        let mut nonce = [0u8; size_of::<Nonce>()];
        nonce[0..4].copy_from_slice(&swap_count.to_be_bytes()); // this is the `swap_count` field
        nonce[5] = src_pid;
        let ppage_masked = dest_offset & !(PAGE_SIZE - 1);
        nonce[6..9].copy_from_slice(&(ppage_masked as u32).to_be_bytes()[..3]);
        let vpage_masked = src_vaddr & !(PAGE_SIZE - 1);
        nonce[9..12].copy_from_slice(&(vpage_masked as u32).to_be_bytes()[..3]);
        let aad: &[u8] = &[];
        match self.cipher.encrypt_in_place_detached(Nonce::from_slice(&nonce), aad, buf) {
            Ok(tag) => {
                self.ram_spim.mem_ram_write(dest_offset as u32, buf, false);
                self.ram_spim.mem_ram_write(
                    (self.swap_mac_start + (dest_offset / PAGE_SIZE) * size_of::<Tag>()) as u32,
                    tag.as_slice(),
                    false,
                );
            }
            Err(e) => panic!("Encryption error to swap ram: {:?}", e),
        }
    }

    /// Swap is assumed to start at offset 0 in the target device, allowing src_offset to be used
    /// by the offset tracker (outside this crate) directly
    pub fn decrypt_swap_from(
        &mut self,
        buf: &mut [u8],
        swap_count: u32,
        src_offset: usize,
        dst_vaddr: usize,
        dst_pid: u8,
    ) -> Result<(), Error> {
        assert!(src_offset & (PAGE_SIZE - 1) == 0);
        assert!(buf.len() == PAGE_SIZE);
        let mut nonce = [0u8; size_of::<Nonce>()];
        nonce[0..4].copy_from_slice(&swap_count.to_be_bytes()); // this is the `swap_count` field
        nonce[5] = dst_pid;
        let ppage_masked = src_offset & !(PAGE_SIZE - 1);
        nonce[6..9].copy_from_slice(&(ppage_masked as u32).to_be_bytes()[..3]);
        let vpage_masked = dst_vaddr & !(PAGE_SIZE - 1);
        nonce[9..12].copy_from_slice(&(vpage_masked as u32).to_be_bytes()[..3]);
        let aad: &[u8] = &[];
        let mut tag = [0u8; size_of::<Tag>()];
        if !self.ram_spim.mem_read(
            (self.swap_mac_start + (src_offset / PAGE_SIZE) * size_of::<Tag>()) as u32,
            &mut tag,
            false,
        ) {
            writeln!(
                DebugUart {},
                "Read timeout of MAC at offset {:x}; data result: {:x?}",
                (self.swap_mac_start + (src_offset / PAGE_SIZE) * size_of::<Tag>()) as u32,
                &tag
            )
            .ok();
        }
        // Retry code added mostly as a diagnostic. Currently, if the SPIM interface
        // fails due to hardware contention, the problem seems to be unrecoverable. We
        // need a way inside mem_read() to reset the PHY entirely, but the hardware interface
        // does not have an obvious way to do it.
        let mut retries = 0;
        while !self.ram_spim.mem_read(src_offset as u32, buf, false) {
            writeln!(
                DebugUart {},
                "Read timeout of data at offset {:x}; data result: {:x?} .. {:x?}",
                src_offset,
                &buf[..16],
                &buf[buf.len() - 16..]
            )
            .ok();
            retries += 1;
            if retries > 2 {
                break;
            }
        }
        self.cipher.decrypt_in_place_detached(Nonce::from_slice(&nonce), aad, buf, (&tag).into())
    }

    /// This wrapper handles arbitrary alignments of `offset` nad sizes of `buf`
    pub fn flash_read(&self, buf: &mut [u8], offset: usize) {
        let mut retries = 0;
        while !self.flash_spim.borrow_mut().mem_read(offset as u32, buf, false) {
            writeln!(
                DebugUart {},
                "FLASH read timeout of data at offset {:x}; data result: {:x?} .. {:x?}",
                offset,
                &buf[..16],
                &buf[buf.len() - 16..]
            )
            .ok();
            retries += 1;
            if retries > 2 {
                break;
            }
        }
    }

    /// This wrapper handles arbitrary alignments of `offset` and sizes of `buf`
    /// `offset` is the offset from the start of FLASH in bytes.
    pub fn flash_write(&mut self, buf: &[u8], offset: usize) {
        #[cfg(feature = "debug-verbose")]
        writeln!(DebugUart {}, "flash_write offset: {:x}", offset).ok();
        let mut written = 0;

        // compute amount of data in the FLASH sector buffer to preserve: everything up to the offset
        let preserve_to = offset & (FLASH_SECTOR_LEN - 1);
        // 1. check for misaligned page starts
        if preserve_to != 0 {
            let replace_end = if buf.len() + preserve_to >= FLASH_SECTOR_LEN {
                FLASH_SECTOR_LEN
            } else {
                buf.len() + preserve_to
            };
            let preserve_start = offset & !(FLASH_SECTOR_LEN - 1);
            {
                // read in the ROM contents for the page in question. This fully replaces self.buffer,
                // so we don't need to zeroize it.
                let mut buff_mut = self.buffer.borrow_mut();
                self.flash_read(&mut buff_mut[..], preserve_start);
            }
            self.buffer.borrow_mut()[preserve_to..replace_end]
                .copy_from_slice(&buf[..replace_end - preserve_to]);
            // erase the containing sector
            self.flash_spim.borrow_mut().flash_erase_sector((offset & !(FLASH_SECTOR_LEN - 1)) as u32);
            // program the sector: we can use write_page because we're strictly aligned and page-length
            for (page, addr) in self
                .buffer
                .borrow()
                .chunks_exact(FLASH_PAGE_LEN)
                .zip((preserve_start..(preserve_start + FLASH_SECTOR_LEN)).step_by(FLASH_PAGE_LEN))
            {
                // only do the write-page if something is not in the erased state
                // this allows us to re-use flash_write as a "slow erase" for things smaller than a block
                if !page.iter().all(|&b| b == 0xff) {
                    self.flash_spim.borrow_mut().mem_flash_write_page(addr as u32, page.try_into().unwrap());
                }
            }
            written += replace_end - preserve_to;
        }
        if written == buf.len() {
            return;
        }
        // 2. write remaining pages
        let remaining_len = buf.len() - written;
        // this is OK here because we took care of any misaligned start data in step 1.
        let aligned_start = (offset + (FLASH_SECTOR_LEN - 1)) & !(FLASH_SECTOR_LEN - 1);
        #[cfg(feature = "debug-verbose")]
        writeln!(
            DebugUart {},
            "aligned_start {:x} end {:x}",
            aligned_start,
            (aligned_start + (remaining_len + (FLASH_SECTOR_LEN - 1)) & !(FLASH_SECTOR_LEN - 1))
        )
        .ok();
        for sector in (aligned_start
            ..(aligned_start + (remaining_len + (FLASH_SECTOR_LEN - 1)) & !(FLASH_SECTOR_LEN - 1)))
            .step_by(FLASH_SECTOR_LEN)
        {
            assert!(buf.len() >= written);
            let remaining_chunk = buf.len() - written;
            {
                let mut buff_mut = self.buffer.borrow_mut();
                if remaining_chunk < FLASH_SECTOR_LEN {
                    // this full replaces self.buffer, so we don't need to zeroize it
                    self.flash_read(&mut buff_mut[..], sector);
                    buff_mut[..remaining_chunk].copy_from_slice(&buf[written..]);
                    written += remaining_chunk;
                } else {
                    buff_mut.copy_from_slice(&buf[written..(written + FLASH_SECTOR_LEN)]);
                    written += FLASH_SECTOR_LEN;
                }
            }
            self.flash_spim.borrow_mut().flash_erase_sector((sector & !(FLASH_SECTOR_LEN - 1)) as u32);
            for (page, addr) in self
                .buffer
                .borrow()
                .chunks_exact(FLASH_PAGE_LEN)
                .zip((sector..(sector + FLASH_SECTOR_LEN)).step_by(FLASH_PAGE_LEN))
            {
                #[cfg(feature = "debug-verbose")]
                writeln!(DebugUart {}, "write_page {:x} {:x?}", addr, &page[..8]).ok();
                self.flash_spim.borrow_mut().mem_flash_write_page(addr as u32, page.try_into().unwrap());
            }
        }
    }

    pub fn block_erase(&mut self, block: usize, len: usize) -> bool {
        self.flash_spim.borrow_mut().flash_erase_block(block, len)
    }
}
