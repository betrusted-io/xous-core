use core::mem::size_of;

use aes_gcm_siv::{AeadInPlace, Aes256GcmSiv, KeyInit, Nonce, Tag};

use crate::bootconfig::BootConfig;
use crate::println;
use crate::swap::*;

pub struct SwapHal {
    data_area: &'static [u8],
    mac_area: &'static [u8],
    partial_nonce: [u8; 8],
    aad: &'static [u8],
    cipher: Aes256GcmSiv,
    buf: RawPage,
}

impl SwapHal {
    pub fn new(cfg: &BootConfig) -> Option<SwapHal> {
        if let Some(swap) = cfg.swap {
            // safety: the swap source header is guaranteed to be aligned and initialized in memory
            // by the image creator.
            let ssh: &SwapSourceHeader =
                unsafe { (swap.flash_offset as *const SwapSourceHeader).as_ref().unwrap() };
            let swap_len = ssh.mac_offset as usize;
            let mut hal = SwapHal {
                // safety: the swap raw array is guaranteed to be correctly aligned by the image maker
                data_area: unsafe {
                    core::slice::from_raw_parts((swap.flash_offset as usize + 4096) as *const u8, swap_len)
                },
                // safety: the mac raw array is guaranteed to be correctly aligned by the image maker
                mac_area: unsafe {
                    core::slice::from_raw_parts(
                        (ssh.mac_offset + swap.flash_offset + 4096) as *const u8,
                        (swap_len / 4096) * size_of::<Tag>(),
                    )
                },
                partial_nonce: [0u8; 8],
                aad: &ssh.aad[..ssh.aad_len as usize],
                cipher: Aes256GcmSiv::new((&swap.key).into()),
                buf: RawPage { data: [0u8; 4096] },
            };
            hal.partial_nonce.copy_from_slice(&ssh.parital_nonce);
            Some(hal)
        } else {
            None
        }
    }

    pub fn decrypt_page_at(&mut self, offset: usize) -> &[u8] {
        assert!((offset & 0xFFF) == 0, "offset is not page-aligned");
        // println!("data area: {:x?}", &self.data_area[..4]);
        // println!("offset: {:x}", offset);
        self.buf.data.copy_from_slice(&self.data_area[offset..offset + 4096]);
        let mut nonce = [0u8; size_of::<Nonce>()];
        nonce[..4].copy_from_slice(&(offset as u32).to_be_bytes());
        nonce[4..].copy_from_slice(&self.partial_nonce);
        let tag = &self.mac_area
            [(offset / 4096) * size_of::<Tag>()..(offset / 4096) * size_of::<Tag>() + size_of::<Tag>()];
        // println!("nonce: {:x?}", nonce);
        // println!("tag: {:x?}", tag);
        // println!("aad: {:x?}", self.aad);
        // println!("data: {:x?}", &self.buf.data[0..32]);
        match self.cipher.decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            self.aad,
            &mut self.buf.data,
            tag.into(),
        ) {
            Ok(_) => &self.buf.data,
            Err(e) => panic!("Decryption error in swap: {:?}", e),
        }
    }
}
