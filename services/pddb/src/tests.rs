use rand::Rng;
use crate::*;

fn gen_key(dictname: &str, keynum: usize, lower_size_bound: usize, upper_size_bound: usize) -> (String, Vec::<u8>) {
    let mut rng = rand::thread_rng();
    // we want roughly half our keys to be in the small bin, and half in the large bin
    let keylen = if rng.gen_bool(0.5) {
        rng.gen_range(lower_size_bound..VPAGE_SIZE)
    } else {
        rng.gen_range(VPAGE_SIZE..upper_size_bound)
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
    (keyname, keydata)
}

pub(crate) fn create_basis_testcase(hw: &mut PddbOs, basis_cache: &mut BasisCache,
    maybe_num_dicts: Option<usize>, maybe_num_keys: Option<usize>, maybe_key_sizes: Option<(usize, usize)>) {

    hw.pddb_format(false).unwrap();
    let sys_basis = hw.pddb_mount().expect("couldn't mount system basis");
    basis_cache.basis_add(sys_basis);

    let num_dicts = maybe_num_dicts.unwrap_or(4);
    let num_keys = maybe_num_keys.unwrap_or(34);
    let (key_lower_bound, key_upper_bound) = maybe_key_sizes.unwrap_or((1, 9000));

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
            basis_cache.key_update(hw, &dictname, &keyname, &keydata, None, None, None, false).unwrap();
        }
    }
}

/// Delete & add dictionary consistency check
pub(crate) fn delete_add_dict_consistency(hw: &mut PddbOs, basis_cache: &mut BasisCache,
    maybe_num_dicts: Option<usize>, maybe_num_keys: Option<usize>, maybe_key_sizes: Option<(usize, usize)>) {
    let evict_count = maybe_num_dicts.unwrap_or(1);
    let num_keys = maybe_num_keys.unwrap_or(36);
    let (key_lower_bound, key_upper_bound) = maybe_key_sizes.unwrap_or((1, 9000));

    let dict_list = basis_cache.dict_list(hw);
    let dict_start_index = dict_list.len();
    for (evicted, evict_dict) in dict_list.iter().enumerate() {
        if evicted < evict_count {
            match basis_cache.dict_remove(hw, evict_dict, None, false) {
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
            // now we're ready to write it out
            basis_cache.key_update(hw, &dictname, &keyname, &keydata, None, None, None, false).unwrap();
        }
    }
}

/// patch data check
pub(crate) fn patch_test(hw: &mut PddbOs, basis_cache: &mut BasisCache,
    maybe_patch_offset: Option<usize>, maybe_patch_data: Option<String>) {

    let patch_offset = maybe_patch_offset.unwrap_or(5);
    let patch_data = maybe_patch_data.unwrap_or("patched!".to_string());

    let dict_list = basis_cache.dict_list(hw);
    for dict in dict_list.iter() {
        if let Ok(key_list) = basis_cache.key_list(hw, dict) {
            for key in key_list.iter() {
                // this actually does something a bit more complicated on a multi-basis system than you'd think:
                // it will get the union of all key names, and then patch the *latest open basis* only with new data.
                // note: if the key didn't already exist in the latest open basis, it's added, with just that patch data in it.
                // to override this behavior, you'd want to specify a _specific_ basis, but for testing purposes, we only have one
                // so we're doing this way that would lead to surprising results if it were copied as template code elsewhere.
                basis_cache.key_update(hw, dict, key, patch_data.as_bytes(), Some(patch_offset), None, None, false).unwrap();
            }
        }

    }
}