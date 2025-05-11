use core::mem::size_of;
use std::fmt::Write;

use aes_gcm_siv::{AeadInPlace, Aes256GcmSiv, Error, KeyInit, Nonce, Tag};
use loader::swap::{SWAP_HAL_VADDR, SwapSpec};

use crate::debug::DebugUart;

pub const PAGE_SIZE: usize = xous::arch::PAGE_SIZE;

/// This defines a set of functions to get and receive MACs (message
/// authentication codes, also referred to as the tag in AES-GCM-SIV.
pub struct SwapHal {
    dst_data_area: &'static mut [u8],
    dst_mac_area: &'static mut [u8],
    cipher: Aes256GcmSiv,
}
impl SwapHal {
    pub fn new(spec: &SwapSpec) -> Self {
        let ram_size_actual = loader::swap::derive_usable_swap(spec.swap_len as usize);
        Self {
            // safety: the ram swap area is pre-mapped into our virtual address by the loader, and our
            // calculations on lengths ensure area alignment
            dst_data_area: unsafe {
                core::slice::from_raw_parts_mut(SWAP_HAL_VADDR as *mut u8, ram_size_actual)
            },
            // safety: the ram swap area is pre-mapped into our virtual address by the loader, and our
            // calculations on lengths ensure area alignment
            dst_mac_area: unsafe {
                core::slice::from_raw_parts_mut(
                    (SWAP_HAL_VADDR as *mut u8).add(ram_size_actual),
                    loader::swap::derive_mac_size(spec.swap_len as usize),
                )
            },
            cipher: Aes256GcmSiv::new((&spec.key).into()),
        }
    }

    /// The data to be encrypted is provided in `buf`, and is replaced with part of the encrypted data upon
    /// completion of the routine.
    pub fn encrypt_swap_to(
        &mut self,
        buf: &mut [u8],
        swap_count: u32,
        dest_offset: usize,
        src_vaddr: usize,
        src_pid: u8,
    ) {
        /*
            writeln!(
                DebugUart {},
                "enc_to: pid: {}, src_vaddr: {:x} dest_offset: {:x}, buf addr: {:x}",
                src_pid,
                src_vaddr,
                dest_offset,
                buf.as_ptr() as usize
            )
            .ok();
        */
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
        // writeln!(DebugUart {}, "bef enc: nonce {:x?} aad {:x?} buf {:x?}", &nonce, aad, &buf[..32]).ok();
        match self.cipher.encrypt_in_place_detached(Nonce::from_slice(&nonce), aad, buf) {
            Ok(tag) => {
                // writeln!(DebugUart {}, "Nonce: {:x?}, tag: {:x?}", &nonce, tag.as_slice()).ok();
                self.dst_data_area[dest_offset..dest_offset + PAGE_SIZE].copy_from_slice(buf);
                let mac_offset = (dest_offset / PAGE_SIZE) * size_of::<Tag>();
                self.dst_mac_area[mac_offset..mac_offset + size_of::<Tag>()].copy_from_slice(tag.as_slice());
                // writeln!(DebugUart {}, "dst_mac_area: {:x?}", &self.dst_mac_area[..32]).ok();
            }
            Err(e) => {
                writeln!(DebugUart {}, "Encryption error to swap ram: {:?}", e).ok();
                panic!("Encryption error to swap ram: {:?}", e)
            }
        }
    }

    /// Used to examine contents of swap RAM. Decrypted data is returned as a slice.
    /// Swap is a 0-offset slice, allowing src_offset to be used by the offset
    /// tracker (outside this crate) directly
    pub fn decrypt_swap_from(
        &mut self,
        buf: &mut [u8],
        swap_count: u32,
        src_offset: usize,
        dst_vaddr: usize,
        dst_pid: u8,
    ) -> Result<(), Error> {
        // use core::fmt::Write;
        // use crate::debug::*;
        // writeln!(DebugUart {}, "Decrypt swap:").ok();
        // writeln!(DebugUart {}, "  offset: {:x}, vaddr: {:x}, pid: {}", src_offset, dst_vaddr,
        // dst_pid).ok();
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
        let mac_offset = (src_offset / PAGE_SIZE) * size_of::<Tag>();
        tag.copy_from_slice(&self.dst_mac_area[mac_offset..mac_offset + size_of::<Tag>()]);
        // writeln!(DebugUart {}, "dst_mac_area: {:x?}", &self.dst_mac_area[..32]).ok();
        buf.copy_from_slice(&self.dst_data_area[src_offset..src_offset + PAGE_SIZE]);
        // writeln!(DebugUart {}, "Nonce: {:x?}, tag: {:x?}", &nonce, &tag).ok();
        let result =
            self.cipher.decrypt_in_place_detached(Nonce::from_slice(&nonce), aad, buf, (&tag).into());
        // writeln!(DebugUart {}, "result: {:?}, buf: {:x?}", result, &buf[..16]).ok();
        result
    }
}
