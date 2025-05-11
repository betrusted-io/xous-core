use core::fmt;
use core::mem::size_of;

use aes_gcm_siv::Tag;

use crate::PAGE_SIZE;
use crate::SWAP_FLG_WIRED;
use crate::SWAPPER_PID;

/// Virtual address fields:
///  31            22 21               12 11               0
/// |    L1 index    |      L0 index     |    LSB of addr   |
///
/// For regular memory, the L1 page is found by taking a 22-bit PPN from the
/// SATP and shifting it left 10 bits and indexing it with 10 bits at [31:22].
///
/// For swap, the L1 page is found by indexing into a table of roots by PID,
/// and then indexing into the resulting root table with 10 bits of VA at [31:22].
///
/// The returned value in the 1024-entry L1 page table is a PA entry where the bottom
/// 10 bits are flags, and top 22 bits are an address. The flags should have V=1,
/// and RWX =0, to indicate an L1 page table entry (we don't use mixed L1->PA indexing).
/// Bits [31:10] are a 22-bit number:
///    - The top 2 bits are 0
///    - The middle 20 bits are the MSB of the address to the PA of the L0 PTE
///
/// The value retrieved from L1 is thus shifted left by 2 bits, and used as a pointer
/// to retrieve the L0 page table.
///
/// L0 page table consists of 1024 entries. This is indexed using 10 bits of the VA
/// at [21:12]. The resulting value consists of a 22-bit physical address, and 10 bits
/// of flags.
///    - The bottom 10 bits are flags
///    - The top 2 bits of the physical address are 0
///    - The middle 20 bits the PA are the MSB of the address to the PA of the target page

#[repr(C)]
pub struct SwapDescriptor {
    pub ram_offset: u32,
    pub ram_size: u32,
    pub name: u32,
    pub key: [u8; 32],
    pub flash_offset: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct SwapSourceHeader {
    pub version: u32,
    pub partial_nonce: [u8; 8],
    pub mac_offset: u32,
    pub aad_len: u32,
    // aad is limited to 64 bytes!
    pub aad: [u8; 64],
}

#[repr(C, align(16))]
pub struct RawPage {
    pub data: [u8; 4096],
}

/// Structure passed by the loader into this process at SWAP_RPT_VADDR
#[cfg(feature = "swap")]
#[repr(C)]
pub struct SwapSpec {
    pub key: [u8; 32],
    /// Count of PIDs in the system. Could be a u8, but, make it a u32 because we have
    /// the space and word alignment is good for stuff being tossed through unsafe pointers.
    pub pid_count: u32,
    /// Physical address of the RPT base (the table for main RAM allocs)
    pub rpt_base_phys: u32,
    /// Length of the memory tracker mapping region in *pages*. This can correspond to a region
    /// that is strictly larger than needed by the RPT.
    pub rpt_len_pages: u32,
    /// Base address of swap memory. If swap is memory-mapped, this is a virtual address.
    /// If swap is device-mapped, it's the physical offset in the device.
    pub swap_base: u32,
    /// Length of swap region in bytes
    pub swap_len: u32,
    /// Base of the message authentication code (MAC) region
    pub mac_base: u32,
    /// Length of the MAC region in bytes
    pub mac_len: u32,
    /// Start of the main memory (i.e., actual physical RAM available for OS use)
    pub sram_start: u32,
    /// Size of the main memory in bytes
    pub sram_size: u32,
}

/// Function that derives the usable amount of swap space from the total length of swap memory available.
/// This is used repeatedly in the initialization process to define the boundary between the swap page
/// storage and the message authentication code (MAC) tables.
///
/// This is a slight over-estimate because once we remove the MAC area, we need even less storage,
/// but it's a small error.
pub fn derive_usable_swap(swap_len: usize) -> usize {
    let mac_size = (swap_len as usize / 4096) * size_of::<Tag>();
    let mac_size_to_page = (mac_size + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);
    let swap_size_usable = (swap_len as usize & !(PAGE_SIZE - 1)) - mac_size_to_page;
    swap_size_usable
}

pub fn derive_mac_size(swap_len: usize) -> usize { (swap_len / 4096) * size_of::<Tag>() }

#[repr(C)]
#[derive(Copy, Clone)]

pub struct SwapAlloc {
    timestamp: u32,
    /// virtual_page_number[19:0] | flags[3:0] | pid[7:0]
    vpn: u32,
}

impl SwapAlloc {
    /// Allocations in the loader all start out as "wired", with no virtual address for tracking.
    pub fn from(pid: u8) -> SwapAlloc { SwapAlloc { timestamp: 0, vpn: pid as u32 | SWAP_FLG_WIRED } }

    /// As the page tables are laid in, we can update the tracker with the virtual address. If
    /// the page it belongs to the kernel or swapper, mark it as unswappable (WIRED)
    pub fn update(&mut self, pid: u8, vaddr: u32) {
        self.vpn = pid as u32
            | vaddr & !0xFFFu32
            | if (pid == 1) || (pid == SWAPPER_PID) { SWAP_FLG_WIRED } else { 0 };
    }

    /// Sets the wired bit. Used for marking page table elements as unswappable.
    pub fn set_wired(&mut self) { self.vpn |= SWAP_FLG_WIRED; }

    /// This is a slight abuse of the naming system to provide us cross-compatibility with the case where the
    /// structure is defined as an overload of the `u8` type
    pub fn to_le(&self) -> u8 { self.vpn as u8 }

    pub fn is_wired(&self) -> bool { (self.vpn & SWAP_FLG_WIRED) != 0 }

    pub fn is_valid(&self) -> bool { self.vpn != 0 }

    pub fn raw_pid(&self) -> u8 { self.vpn as u8 }

    pub fn vaddr(&self) -> usize { (self.vpn & !0xFFFu32) as usize }

    pub fn vaddr_prefix(&self) -> u8 { (self.vpn >> 24) as u8 }

    pub fn raw_vpn(&self) -> u32 { self.vpn }

    pub fn timestamp(&self) -> u32 { self.timestamp }
}

impl fmt::Debug for SwapAlloc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SwapAlloc")
            .field("pid", &(self.vpn as u8))
            .field("vaddr", &(self.vpn & !0xFFF))
            .field("flags", &(if self.vpn & SWAP_FLG_WIRED != 0 { "WIRED" } else { "NONE" }))
            .finish()
    }
}

impl PartialEq for SwapAlloc {
    fn eq(&self, other: &Self) -> bool { self.timestamp == other.timestamp }
}

impl Eq for SwapAlloc {}

impl PartialOrd for SwapAlloc {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> { Some(self.cmp(other)) }
}

impl Ord for SwapAlloc {
    // Select this for smallest timestamps on pop()
    fn cmp(&self, other: &Self) -> core::cmp::Ordering { other.timestamp.cmp(&self.timestamp) }

    // Select this for biggest timestamps on pop()
    // fn cmp(&self, other: &Self) -> core::cmp::Ordering { self.timestamp.cmp(&other.timestamp) }
}
