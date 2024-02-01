#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(not(target_os = "xous"))]
mod main_hosted;
#[cfg(any(feature = "precursor", feature = "renode"))]
mod main_hw;

mod api;
#[rustfmt::skip] // don't format lookup tables
mod mappings;

use api::*;
#[cfg(any(feature = "precursor", feature = "renode"))]
mod hw;
#[cfg(any(feature = "precursor", feature = "renode"))]
use hw::*;
#[cfg(any(feature = "precursor", feature = "renode"))]
mod spinal_udc;
#[cfg(any(feature = "precursor", feature = "renode"))]
use packed_struct::PackedStructSlice;
#[cfg(any(feature = "precursor", feature = "renode"))]
use spinal_udc::*;
#[cfg(all(any(feature = "precursor", feature = "renode"), feature = "mass-storage"))]
mod apps_block_device;

#[cfg(any(feature = "precursor", feature = "renode"))]
mod hid;
#[cfg(not(target_os = "xous"))]
mod hosted;
use std::collections::BTreeMap;

#[cfg(not(target_os = "xous"))]
use hosted::*;
#[cfg(any(feature = "precursor", feature = "renode"))]
use num_traits::*;

fn main() -> ! {
    #[cfg(any(feature = "precursor", feature = "renode"))]
    main_hw::main_hw();
    #[cfg(not(target_os = "xous"))]
    main_hosted::main_hosted();
}

#[cfg(any(feature = "precursor", feature = "renode"))]
pub(crate) const START_OFFSET: u32 = 0x0048 + 8 + 16; // align spinal free space to 16-byte boundary + 16 bytes for EP0 read
#[cfg(any(feature = "precursor", feature = "renode"))]
pub(crate) const END_OFFSET: u32 = 0x1000; // derived from RAMSIZE parameter: this could be a dynamically read out constant, but, in practice, it's part of the hardware
/// USB endpoint allocator. The SpinalHDL USB controller appears as a block of
/// unstructured memory to the host. You can specify pointers into the memory with
/// an offset and length to define where various USB descriptors should be placed.
/// This allocator manages that space.
///
/// Note that all allocations must be aligned to 16-byte boundaries. This is a restriction
/// of the USB core.
///
/// Returns a full memory address as the pointer. Must be shifted left by 4 to get the
/// aligned representation used by the SpinalHDL block.
#[cfg(any(feature = "precursor", feature = "renode"))]
pub(crate) fn alloc_inner(allocs: &mut BTreeMap<u32, u32>, requested: u32) -> Option<u32> {
    if requested == 0 {
        return None;
    }
    let with_descriptor = requested + 16; // the descriptor takes 3 words; add 4 because of the alignment requirement
    let mut alloc_offset = START_OFFSET;
    for (&offset, &length) in allocs.iter() {
        // round length up to the nearest 16-byte increment
        let length = if length & 0xF == 0 { length } else { (length + 16) & !0xF };
        // println!("aoff: {}, cur: {}+{}", alloc_offset, offset, length);
        assert!(offset >= alloc_offset, "allocated regions overlap");
        if offset > alloc_offset {
            if offset - alloc_offset >= with_descriptor {
                // there's a hole in the list, insert the element here
                break;
            }
        }
        alloc_offset = offset + length;
    }
    if alloc_offset + with_descriptor <= END_OFFSET {
        allocs.insert(alloc_offset, with_descriptor);
        Some(alloc_offset)
    } else {
        None
    }
}
#[allow(dead_code)]
pub(crate) fn dealloc_inner(allocs: &mut BTreeMap<u32, u32>, offset: u32) -> bool {
    allocs.remove(&offset).is_some()
}

// run with `cargo test -- --nocapture --test-threads=1`:
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_alloc() {
        use rand_chacha::rand_core::RngCore;
        use rand_chacha::rand_core::SeedableRng;
        use rand_chacha::ChaCha8Rng;
        let mut rng = ChaCha8Rng::seed_from_u64(0);

        let mut allocs = BTreeMap::<u32, u32>::new();
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET));
        assert_eq!(alloc_inner(&mut allocs, 64), Some(START_OFFSET + 128));
        assert_eq!(alloc_inner(&mut allocs, 256), Some(START_OFFSET + 128 + 64));
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 256));
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 256 + 128));
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 256 + 128 + 128));
        assert_eq!(alloc_inner(&mut allocs, 0xFF00), None);

        // create two holes and fill first hole, interleaved
        assert_eq!(dealloc_inner(&mut allocs, START_OFFSET + 128 + 64), true);
        let mut last_alloc = 0;
        // consistency check and print out
        for (&offset, &len) in allocs.iter() {
            assert!(offset >= last_alloc, "new offset is inside last allocation!");
            println!("{}-{}", offset, offset + len);
            last_alloc = offset + len;
        }

        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64));
        assert_eq!(dealloc_inner(&mut allocs, START_OFFSET + 128 + 64 + 256 + 128), true);
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 128));

        // alloc something that doesn't fit at all
        assert_eq!(alloc_inner(&mut allocs, 256), Some(START_OFFSET + 128 + 64 + 256 + 128 + 128 + 128));

        // fill second hole
        assert_eq!(alloc_inner(&mut allocs, 128), Some(START_OFFSET + 128 + 64 + 256 + 128));

        // final tail alloc
        assert_eq!(alloc_inner(&mut allocs, 64), Some(START_OFFSET + 128 + 64 + 256 + 128 + 128 + 128 + 256));

        println!("after structured test:");
        let mut last_alloc = 0;
        // consistency check and print out
        for (&offset, &len) in allocs.iter() {
            assert!(offset >= last_alloc, "new offset is inside last allocation!");
            println!("{}-{}({})", offset, offset + len, len);
            last_alloc = offset + len;
        }

        // random alloc/dealloc and check for overlapping regions
        let mut tracker = Vec::<u32>::new();
        for _ in 0..10240 {
            if rng.next_u32() % 2 == 0 {
                if tracker.len() > 0 {
                    //println!("tracker: {:?}", tracker);
                    let index = tracker.remove((rng.next_u32() % tracker.len() as u32) as usize);
                    //println!("removing: {} of {}", index, tracker.len());
                    assert_eq!(dealloc_inner(&mut allocs, index), true);
                }
            } else {
                let req = rng.next_u32() % 256;
                if let Some(offset) = alloc_inner(&mut allocs, req) {
                    //println!("tracker: {:?}", tracker);
                    //println!("alloc: {}+{}", offset, req);
                    tracker.push(offset);
                }
            }
        }

        let mut last_alloc = 0;
        // consistency check and print out
        println!("after random test:");
        for (&offset, &len) in allocs.iter() {
            assert!(offset >= last_alloc, "new offset is inside last allocation!");
            assert!(offset & 0xF == 0, "misaligned allocation detected");
            println!("{}-{}({})", offset, offset + len, len);
            last_alloc = offset + len;
        }
    }
}
