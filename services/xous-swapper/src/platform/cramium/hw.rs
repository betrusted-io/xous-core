use core::fmt::Write;
use core::mem::size_of;

use aes_gcm_siv::{AeadInPlace, Aes256GcmSiv, Error, KeyInit, Nonce, Tag};
use cramium_api::*;
use cramium_hal::board::SPIM_RAM_IFRAM_ADDR;
use cramium_hal::ifram::IframRange;
use cramium_hal::udma::*;
use loader::swap::{SWAP_HAL_VADDR, SwapSpec};

use crate::debug::*;

pub const PAGE_SIZE: usize = 4096;

/// This is an implementation for SMTs that are accessible only through a SPI
/// register interface. The base and bounds must be translated to SPI accesses
/// in a hardware-specific manner.
pub struct SwapHal {
    swap_mac_start: usize,
    cipher: Aes256GcmSiv,
    ram_spim: Spim,
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
}
