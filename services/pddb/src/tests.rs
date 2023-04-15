use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;
use rand_chacha::rand_core::SeedableRng;
use crate::*;
use core::sync::atomic::{AtomicU64, Ordering};
use std::collections::{BTreeSet, HashSet};
use std::io::Result;

const UPPER_BOUND: usize = 9000;
const LOWER_BOUND: usize = 12; // needs to be big enough to compute murmur3 hash + hold checksum

static RNG_LOCAL_STATE: AtomicU64 = AtomicU64::new(3);

fn gen_key(dictname: &str, keynum: usize, lower_size_bound: usize, upper_size_bound: usize) -> (String, Vec::<u8>) {
    let mut rng = ChaCha8Rng::seed_from_u64(RNG_LOCAL_STATE.load(Ordering::SeqCst) + xous::TESTING_RNG_SEED.load(core::sync::atomic::Ordering::SeqCst));
    // we want roughly half our keys to be in the small bin, and half in the large bin
    let keylen = if rng.next_u32() < (u32::MAX / 2) {
        ((rng.next_u64() as usize) % (VPAGE_SIZE - lower_size_bound)) + lower_size_bound
        //rng.gen_range(lower_size_bound..VPAGE_SIZE) // older API used to minimize crate count
    } else {
        ((rng.next_u64() as usize) % (upper_size_bound - VPAGE_SIZE)) + VPAGE_SIZE
        //rng.gen_range(VPAGE_SIZE..upper_size_bound) // older API used to minimize crate count
    };
    // record the owning dictionary name & length with the key. This isn't mandatory for a key name,
    // but it helps the test checking program check things.
    let keyname = format!("sanitycheck|{}|key{}|len{}", dictname, keynum, keylen);
    let mut keydata = Vec::<u8>::new();
    for i in 0..keylen-4 {
        // the data starts with a number equal to the key number, and increments from there
        keydata.push((keynum + i) as u8);
    }
    // a checksum is appended to each run of data
    // copy the stored data, and pad it out to a multiple of word lengths with 0's so we can compute the checksum
    let mut checkdata = Vec::<u8>::new();
    for &b in &keydata {
        checkdata.push(b);
    }
    while checkdata.len() % 4 != 0 {
        checkdata.push(0);
    }
    let checksum = murmur3_32(&checkdata, 0);
    keydata.append(&mut checksum.to_le_bytes().to_vec());
    RNG_LOCAL_STATE.store(rng.next_u64(), Ordering::SeqCst);
    (keyname, keydata)
}

pub(crate) fn create_basis_testcase(hw: &mut PddbOs, basis_cache: &mut BasisCache,
    maybe_num_dicts: Option<usize>, maybe_num_keys: Option<usize>, maybe_key_sizes: Option<(usize, usize)>,
    maybe_extra_reserved: Option<usize>,
) -> Result<()> {

    hw.pddb_format(false, None).unwrap();
    let sys_basis = hw.pddb_mount().expect("couldn't mount system basis");
        basis_cache.basis_add(sys_basis);

    let num_dicts = maybe_num_dicts.unwrap_or(4);
    let num_keys = maybe_num_keys.unwrap_or(34);
    let (key_lower_bound, key_upper_bound) = maybe_key_sizes.unwrap_or((LOWER_BOUND, UPPER_BOUND - 4)); // 4 bytes for the CI checksum
    let extra_reserved = maybe_extra_reserved.unwrap_or(0);

    // make all the directories first
    for dictnum in 1..=num_dicts {
        let dictname = format!("dict{}", dictnum);
        basis_cache.dict_add(hw, &dictname, None).unwrap();
    }

    // now add keys of various sizes, striping through each dictionary. Adding the keys cross-dictionary
    // exercises the large pool allocator, as it is shared across multiple dictionaries.
    for keynum in 1..=num_keys {
        for dictnum in 1..=num_dicts {
            let dictname = format!("dict{}", dictnum);
            let (keyname, keydata) = gen_key(&dictname, keynum, key_lower_bound, key_upper_bound);
            // now we're ready to write it out
            if maybe_extra_reserved.is_none() {
                // we do this slightly funky way instead of just passing Some(0) because we want to test the "None" path explicitly
                basis_cache.key_update(hw, &dictname, &keyname, &keydata, None,
                    None,
                    None, false)?;
            } else {
                basis_cache.key_update(hw, &dictname, &keyname, &keydata, None,
                    Some(keydata.len() + extra_reserved),
                    None, false)?;
            }
        }
    }
    Ok(())
}

/// Delete & add dictionary consistency check
pub(crate) fn delete_add_dict_consistency(hw: &mut PddbOs, basis_cache: &mut BasisCache,
    maybe_num_dicts: Option<usize>, maybe_num_keys: Option<usize>, maybe_key_sizes: Option<(usize, usize)>,
    maybe_extra_reserved: Option<usize>, basis_name: Option<&str>
) -> Result<()> {
    let evict_count = maybe_num_dicts.unwrap_or(1);
    let num_keys = maybe_num_keys.unwrap_or(36);
    let (key_lower_bound, key_upper_bound) = maybe_key_sizes.unwrap_or((LOWER_BOUND, UPPER_BOUND - 4));
    let extra_reserved = maybe_extra_reserved.unwrap_or(0);

    #[cfg(feature = "deterministic")]
    let mut dict_list = BTreeSet::<String>::new();
    #[cfg(feature = "deterministic")]
    {
        let dict_list_unord = basis_cache.dict_list(hw, None);
        for s in dict_list_unord {
            dict_list.insert(s);
        }
    }
    #[cfg(not(feature = "deterministic"))]
    let dict_list = basis_cache.dict_list(hw, None);

    let dict_start_index = dict_list.len();
    for (evicted, evict_dict) in dict_list.iter().enumerate() {
        if evicted < evict_count {
            log::debug!("evicting dict {}", evict_dict);
            match basis_cache.dict_remove(hw, evict_dict, basis_name, false) {
                Ok(_) => {},
                Err(e) => log::error!("Error evicting dictionary {}: {:?}", evict_dict, e),
            }
        } else {
            break;
        }
    }

    for keynum in 1..=num_keys {
        for dictnum in 1..=evict_count {
            let dictname = format!("dict{}", dictnum + dict_start_index);
            let (keyname, keydata) = gen_key(&dictname, keynum, key_lower_bound, key_upper_bound);
            log::debug!("adding {}:{}", dictname, keyname);
            // now we're ready to write it out
            if maybe_extra_reserved.is_none() {
                // we do this slightly funky way instead of just passing Some(0) because we want to test the "None" path explicitly
                basis_cache.key_update(hw, &dictname, &keyname, &keydata, None,
                    None,
                    basis_name, false)?;
            } else {
                basis_cache.key_update(hw, &dictname, &keyname, &keydata, None,
                    Some(keydata.len() + extra_reserved),
                    basis_name, false)?;
            }
        }
    }
    Ok(())
}

/// patch data check
pub(crate) fn patch_test(hw: &mut PddbOs, basis_cache: &mut BasisCache,
    maybe_patch_offset: Option<usize>, maybe_patch_data: Option<String>, extend: bool) -> Result<()> {

    let patch_offset = maybe_patch_offset.unwrap_or(5);
    let patch_data = maybe_patch_data.unwrap_or("patched!".to_string());

    #[cfg(feature = "deterministic")]
    let mut dict_list = BTreeSet::<String>::new();
    #[cfg(feature = "deterministic")]
    {
        let dict_list_unord = basis_cache.dict_list(hw, None);
        for s in dict_list_unord {
            dict_list.insert(s);
        }
    }
    #[cfg(not(feature = "deterministic"))]
    let dict_list = basis_cache.dict_list(hw, None);
    for dict in dict_list.iter() {
        if let Ok((key_list_unord, _, _)) = basis_cache.key_list(hw, dict, None) {
            #[cfg(feature = "deterministic")]
            let mut key_list = BTreeSet::<String>::new();
            #[cfg(feature = "deterministic")]
            for s in key_list_unord {
                key_list.insert(s);
            }
            #[cfg(not(feature = "deterministic"))]
            let key_list = key_list_unord;
            for key in key_list.iter() {
                log::debug!("updating {}:{}", dict, key);
                // this actually does something a bit more complicated on a multi-basis system than you'd think:
                // it will get the union of all key names, and then patch the *latest open basis* only with new data.
                // note: if the key didn't already exist in the latest open basis, it's added, with just that patch data in it.
                // to override this behavior, you'd want to specify a _specific_ basis, but for testing purposes, we only have one
                // so we're doing this way that would lead to surprising results if it were copied as template code elsewhere.
                match basis_cache.key_update(hw, dict, key, patch_data.as_bytes(), Some(patch_offset), None, None, false) {
                    Ok(_) => (),
                    Err(e) => {
                        log::error!("couldn't update key {}:{} with {} bytes data offset {}: {:?}", dict, key, patch_data.as_bytes().len(), patch_offset, e);
                        panic!("error updating patch key");
                    }
                }

                // now fix the CI checksum. structured as two separate patches. not because it's efficient,
                // but because it exercises the code harder.
                let mut patchbuf = [0u8; UPPER_BOUND];
                let readlen = basis_cache.key_read(hw, dict, key, &mut patchbuf, Some(0), None).unwrap();
                if !extend {
                    // now re-compute the checksum
                    let mut checkdata = Vec::<u8>::new();
                    for &b in &patchbuf[..readlen-4] {
                        checkdata.push(b);
                    }
                    log::trace!("checkdata len: {}", checkdata.len());
                    while checkdata.len() % 4 != 0 {
                        checkdata.push(0);
                    }
                    let checksum = murmur3_32(&checkdata, 0);
                    basis_cache.key_update(hw, dict, key, &checksum.to_le_bytes(), Some(readlen-4), None, None, false)?;
                } else {
                    // this gloms a new checksum onto the existing record, adding 4 new bytes.
                    let mut checkdata = Vec::<u8>::new();
                    for &b in &patchbuf[..readlen] {
                        checkdata.push(b);
                    }
                    log::trace!("checkdata len: {}", checkdata.len());
                    while checkdata.len() % 4 != 0 {
                        checkdata.push(0);
                    }
                    let checksum = murmur3_32(&checkdata, 0);
                    basis_cache.key_update(hw, dict, key, &checksum.to_le_bytes(), Some(readlen), None, None, false)?;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn delete_pattern(hw: &mut PddbOs, basis_cache: &mut BasisCache,
    maybe_num_dicts: Option<usize>, maybe_num_keys: Option<usize>, maybe_key_sizes: Option<(usize, usize)>,
    maybe_extra_reserved: Option<usize>,
) -> Result<()> {
    let evict_count = maybe_num_dicts.unwrap_or(2);
    let num_keys = maybe_num_keys.unwrap_or(36);
    let (key_lower_bound, key_upper_bound) = maybe_key_sizes.unwrap_or((LOWER_BOUND, UPPER_BOUND - 4));
    let extra_reserved = maybe_extra_reserved.unwrap_or(0);

    let mut evicted_dicts = Vec::<String>::new();
    #[cfg(feature = "deterministic")]
    let mut dict_list = BTreeSet::<String>::new();
    #[cfg(feature = "deterministic")]
    {
        let dict_list_unord = basis_cache.dict_list(hw, None);
        for s in dict_list_unord {
            dict_list.insert(s);
        }
    }
    #[cfg(not(feature = "deterministic"))]
    let dict_list = basis_cache.dict_list(hw, None);
    let mut evict_iter = 0;
    for (evicted, evict_dict) in dict_list.iter().enumerate() {
        if evicted < evict_count {
            let (key_list_unord, _, _) = basis_cache.key_list(hw, evict_dict, None).unwrap();
            let mut key_list = BTreeSet::<String>::new();
            for s in key_list_unord {
                key_list.insert(s);
            }
            for (index, key) in key_list.iter().enumerate() {
                if index % 2 == (evict_iter % 2) { // exercise odd/even patterns
                    log::info!("deleting {}:{}", evict_dict, key);
                    basis_cache.key_remove(hw, evict_dict, key,None, false).unwrap();
                    let da = basis_cache.dict_attributes(hw, evict_dict, None).unwrap();
                    log::info!("{:?}", da);
                }
            }
        } else {
            break;
        }
        evicted_dicts.push(evict_dict.to_string());
        evict_iter += 1;
    }
    #[cfg(feature = "deterministic")]
    let mut dict_list = BTreeSet::<String>::new();
    #[cfg(feature = "deterministic")]
    {
        let dict_list_unord = basis_cache.dict_list(hw, None);
        for s in dict_list_unord {
            dict_list.insert(s);
        }
    }
    #[cfg(not(feature = "deterministic"))]
    let dict_list = basis_cache.dict_list(hw, None);
    for dict in dict_list.iter() {
        let da = basis_cache.dict_attributes(hw, dict, None).unwrap();
        log::debug!("{:?}", da);
    }

    for keynum in 1..=num_keys {
        for dictname in evicted_dicts.iter() {
            let (keyname, keydata) = gen_key(dictname, keynum, key_lower_bound, key_upper_bound);
            // now we're ready to write it out
            match basis_cache.key_attributes(hw, dictname, &keyname, None) {
                Ok(_ka) => {
                    log::warn!("rng collision on keygen: {}, deleting key", keyname);
                    basis_cache.key_remove(hw, dictname, &keyname, None, false)?;
                }
                _ => {
                }
            }
            if maybe_extra_reserved.is_none() {
                // we do this slightly funky way instead of just passing Some(0) because we want to test the "None" path explicitly
                basis_cache.key_update(hw, dictname, &keyname, &keydata, None,
                    None,
                    None, false)?;
            } else {
                basis_cache.key_update(hw, dictname, &keyname, &keydata, None,
                    Some(keydata.len() + extra_reserved),
                    None, false)?;
            }
        }
    }
    #[cfg(feature = "deterministic")]
    let mut dict_list = BTreeSet::<String>::new();
    #[cfg(feature = "deterministic")]
    {
        let dict_list_unord = basis_cache.dict_list(hw, None);
        for s in dict_list_unord {
            dict_list.insert(s);
        }
    }
    #[cfg(not(feature = "deterministic"))]
    let dict_list = basis_cache.dict_list(hw, None);
    for dict in dict_list.iter() {
        let da = basis_cache.dict_attributes(hw, dict, None).unwrap();
        log::debug!("{:?}", da);
    }

    log::info!("doing all-basis sync");
    basis_cache.sync(hw, None, false).unwrap();
    log::info!("all-basis sync done");

    list_all(hw, basis_cache);
    #[cfg(feature = "deterministic")]
    let mut dict_list = BTreeSet::<String>::new();
    #[cfg(feature = "deterministic")]
    {
        let dict_list_unord = basis_cache.dict_list(hw, None);
        for s in dict_list_unord {
            dict_list.insert(s);
        }
    }
    #[cfg(not(feature = "deterministic"))]
    let dict_list = basis_cache.dict_list(hw, None);
    for dict in dict_list.iter() {
        let da = basis_cache.dict_attributes(hw, dict, None).unwrap();
        log::info!("{:?}", da);
    }
    Ok(())
}

pub(crate) fn list_all(hw: &mut PddbOs, basis_cache: &mut BasisCache) {
    #[cfg(feature = "deterministic")]
    let mut dict_list = BTreeSet::<String>::new();
    #[cfg(feature = "deterministic")]
    {
        let dict_list_unord = basis_cache.dict_list(hw, None);
        for s in dict_list_unord {
            dict_list.insert(s);
        }
    }
    #[cfg(not(feature = "deterministic"))]
    let dict_list = basis_cache.dict_list(hw, None);

    // now list all the attributes of the basis
    for dict in dict_list.iter() {
        let da = match basis_cache.dict_attributes(hw, &dict, None) {
            Ok(da) => da,
            Err(e) => {
                log::warn!("dictionary {} not in sub-basis (this is normal for multi-basis systems, {:?}", dict, e);
                continue;
            }
        };
        log::debug!("{:?}", da);
        let mut sanity_count = 0;
        let (key_list, _, _) = basis_cache.key_list(hw, dict, None).unwrap();
        for key in key_list.iter() {
            let attrs = match basis_cache.key_attributes(hw, dict, key, None) {
                Ok(a) => a,
                Err(e) => {log::debug!("key not in basis, searching another basis, {:?}", e);  continue},
            };
            log::debug!("{}:{}=>{:?}", dict, key, attrs);
            sanity_count += 1;
        }
        log::info!("sanity check count: {}", sanity_count);
    }
}

/* list of test cases:
    - [done] genenral integrity: allocate 4 dictionaries, each with 34 keys of various sizes ranging from 1k-9k.
    - [done] delete/add consistency: general integrity, delete a dictionary, then add a dictionary.
    - [done] in-place update consistency: general integrity then patch all keys with a new test pattern
    - [done] extend update consistency: general integrity then patch all keys with a longer test pattern
    - [done] key deletion torture test: delete every other key in a dictionary, then regenerate some of them with new data.
    - [done] fast space exhaustion test: allocate and delete a bunch of stuff. trigger a fast-space regenerate.
        note: for faster stress-testing, we dialed the FSCB_PAGES to 4 and the FASTSPACE_PAGES to 1.
    - [done] basis search: create basis A, populate with general integrity. create basis B, add test entries.
        hide basis B, confirm original A; mount basis B, confirm B overlay.
*/

#[allow(dead_code)]
pub(crate) fn ci_tests(pddb_os: &mut PddbOs) -> Result<()> {
    {
        const EXTRA_BASIS: &'static str = "Basis2";
        const EXTRA_BASIS_PW: &'static str = "some password blah blah";

        log::set_max_level(log::LevelFilter::Info);
        log::info!("Seed for this run: {}", xous::TESTING_RNG_SEED.load(core::sync::atomic::Ordering::SeqCst));

        pddb_os.test_reset();
        log::info!("Creating `basecase1e`");
        log::set_max_level(log::LevelFilter::Info);
        let mut basis_cache = BasisCache::new();
        create_basis_testcase(pddb_os, &mut basis_cache, None,
            None, None, Some(32))?;
        log::info!("Saving `basecase1e` to local host");
        pddb_os.dbg_dump(Some("basecase1e".to_string()), None);
        let extra_basis_key = pddb_os.basis_derive_key(EXTRA_BASIS, EXTRA_BASIS_PW);
        let mut name = [0 as u8; 64];
        for (&src, dst) in EXTRA_BASIS.as_bytes().iter().zip(name.iter_mut()) {
            *dst = src;
        }
        let extra_export = KeyExport {
            basis_name: name,
            key: extra_basis_key.data,
            pt_key: extra_basis_key.pt,
        };
        let mut export: Vec::<KeyExport> = Vec::new();
        export.push(extra_export);

        log::info!("Building a second basis");
        basis_cache.basis_create(pddb_os,
            EXTRA_BASIS, EXTRA_BASIS_PW).expect("couldn't build test basis");

        log::info!("heap usage: {}", heap_usage());
        test_prune(pddb_os, &mut basis_cache);
        log::info!("Doing delete/add consistency with data extension");
        delete_add_dict_consistency(pddb_os, &mut basis_cache, None,
            None, None, None, None)?;
        test_prune(pddb_os, &mut basis_cache);
        log::info!("Saving `dachecke` to local host");
        pddb_os.dbg_dump(Some("dachecke".to_string()), None);

        log::info!("Doing patch test");
        patch_test(pddb_os, &mut basis_cache, None, None, true)?;
        pddb_os.dbg_dump(Some("patche".to_string()), None);

        log::info!("Doing delete pattern test");
        delete_pattern(pddb_os, &mut basis_cache, None, None, None, None)?;
        pddb_os.dbg_dump(Some("patterne".to_string()), None);

        // extended tests.
        // allocation space curtailed to force resource exhaustion faster.
        // note to self: FSCB_PAGES revert to 16 (hw.rs), FASTSPACE_PAGES revert to 2 (fastspace.rs)
        log::info!("Doing patch test 2");
        patch_test(pddb_os, &mut basis_cache, None, None, true)?;
        pddb_os.dbg_dump(Some("patche2".to_string()), None);

        log::info!("Doing delete pattern test 2");
        delete_pattern(pddb_os, &mut basis_cache, None, None, None, None)?;
        pddb_os.dbg_dump(Some("patterne2".to_string()), None);

        log::info!("Doing delete/add consistency with data extension 2");
        delete_add_dict_consistency(pddb_os, &mut basis_cache, Some(3),
            Some(50), None, None, None)?;
        log::info!("Saving `dachecke2` to local host");
        pddb_os.dbg_dump(Some("dachecke2".to_string()), None);
        test_prune(pddb_os, &mut basis_cache);

        log::info!("Doing delete/add consistency with data extension 3");
        delete_add_dict_consistency(pddb_os, &mut basis_cache, Some(3),
            Some(50), None, None, None)?;
        log::info!("Saving `dachecke3` to local host");
        pddb_os.dbg_dump(Some("dachecke3".to_string()), None);
        test_prune(pddb_os, &mut basis_cache);

        log::info!("Doing delete/add consistency with data extension 4");
        delete_add_dict_consistency(pddb_os, &mut basis_cache, Some(6),
            Some(50), None, None, None)?;
        log::info!("Saving `dachecke4` to local host");
        pddb_os.dbg_dump(Some("dachecke4".to_string()), None);
        test_prune(pddb_os, &mut basis_cache);

        let mut pre_list = HashSet::<String>::new();
        for dict in basis_cache.dict_list(pddb_os, None).iter() {
            let (key_list, _, _) = basis_cache.key_list(pddb_os, dict, None).unwrap();
            for key in key_list.iter() {
                pre_list.insert(key.to_string());
            }
        }

        log::info!("Doing remount disk test");
        let mut basis_cache = BasisCache::new();
        if let Some(sys_basis) = pddb_os.pddb_mount() {
            basis_cache.basis_add(sys_basis);
            list_all(pddb_os, &mut basis_cache);
            pddb_os.dbg_dump(Some("remounte".to_string()), Some(&export));
        }

        log::info!("Mounting the second basis");
        if let Some(basis2) = basis_cache.basis_unlock(pddb_os,
            EXTRA_BASIS, EXTRA_BASIS_PW, BasisRetentionPolicy::Persist) {
            basis_cache.basis_add(basis2);
        }
        log::set_max_level(log::LevelFilter::Info);
        log::info!("Adding keys to Basis2");
        delete_add_dict_consistency(pddb_os, &mut basis_cache, Some(3),
            Some(15), None, None, Some(EXTRA_BASIS))?;
        log::info!("Saving `basis2` to local host");
        test_prune(pddb_os, &mut basis_cache);
        pddb_os.dbg_dump(Some("basis2".to_string()), Some(&export));
        log::set_max_level(log::LevelFilter::Info);

        let mut merge_list = HashSet::<String>::new();
        for dict in basis_cache.dict_list(pddb_os, None).iter() {
            let (key_list, _, _) = basis_cache.key_list(pddb_os, dict, None).unwrap();
            for key in key_list.iter() {
                merge_list.insert(key.to_string());
            }
        }
        assert!(pre_list.is_subset(&merge_list), "pre-merge list is not a subset of the merged basis");

        let mut b2_list = HashSet::<String>::new();
        for dict in basis_cache.dict_list(pddb_os, Some(EXTRA_BASIS)).iter() {
            let (key_list, _, _) = basis_cache.key_list(pddb_os, dict, Some(EXTRA_BASIS)).unwrap();
            for key in key_list.iter() {
                b2_list.insert(key.to_string());
            }
        }
        assert!(b2_list.is_subset(&merge_list), "basis 2 is not a subset of the merged lists");

        basis_cache.basis_unmount(pddb_os, EXTRA_BASIS).unwrap();
        let mut post_list = HashSet::<String>::new();
        for dict in basis_cache.dict_list(pddb_os, None).iter() {
            let (key_list, _, _) = basis_cache.key_list(pddb_os, dict, None).unwrap();
            for key in key_list.iter() {
                post_list.insert(key.to_string());
            }
        }

        log::info!("Doing remount disk test part 2");
        let mut basis_cache = BasisCache::new();
        if let Some(sys_basis) = pddb_os.pddb_mount() {
            basis_cache.basis_add(sys_basis);
        }
        let mut remount_list = HashSet::<String>::new();
        for dict in basis_cache.dict_list(pddb_os, None).iter() {
            let (key_list, _, _) = basis_cache.key_list(pddb_os, dict, None).unwrap();
            for key in key_list.iter() {
                remount_list.insert(key.to_string());
            }
        }
        assert!(remount_list.difference(&pre_list).count() == 0, "remounted list is not identical to the original list");

        log::info!("Mounting the second basis again");
        if let Some(basis2) = basis_cache.basis_unlock(pddb_os,
            EXTRA_BASIS, EXTRA_BASIS_PW, BasisRetentionPolicy::Persist) {
            basis_cache.basis_add(basis2);
        }
        let mut merge2_list = HashSet::<String>::new();
        for dict in basis_cache.dict_list(pddb_os, None).iter() {
            let (key_list, _, _) = basis_cache.key_list(pddb_os, dict, None).unwrap();
            for key in key_list.iter() {
                merge2_list.insert(key.to_string());
            }
        }
        assert!(merge2_list.difference(&merge_list).count() == 0, "merged list is different from the original list after remount");
        list_all(pddb_os, &mut basis_cache);

        log::info!("CI done");
        xous::rsyscall(xous::SysCall::Shutdown).unwrap();
        Ok(())
    }
}

fn test_prune(hw: &mut PddbOs, basis_cache: &mut BasisCache) {
    const TARGET_SIZE: usize = 150*1024;
    let cache_size = basis_cache.cache_size();
    if cache_size > TARGET_SIZE {
        log::info!("size is {}, requesting prune of {}", cache_size, cache_size - TARGET_SIZE);
        log::info!("pruned {}", basis_cache.cache_prune(hw, cache_size - TARGET_SIZE));
    }
    log::info!("size is now {}", basis_cache.cache_size());
}