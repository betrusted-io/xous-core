use rand::Rng;
use crate::*;

pub(crate) fn create_testcase(hw: &mut PddbOs,
    maybe_num_dicts: Option<usize>, maybe_num_keys: Option<usize>, maybe_key_sizes: Option<(usize, usize)>) {
    let mut rng = rand::thread_rng();

    hw.pddb_format().unwrap();
    let mut basis_cache = BasisCache::new();
    let sys_basis = hw.pddb_mount().expect("couldn't mount system basis");
    basis_cache.basis_add(sys_basis);

    let num_dicts = maybe_num_dicts.unwrap_or(4);
    let num_keys = maybe_num_keys.unwrap_or(34);
    let (key_lower_bound, key_upper_bound) = maybe_key_sizes.unwrap_or((1000, 9000));

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
            let keylen = rng.gen_range(key_lower_bound..key_upper_bound);
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
            // now we're ready to write it out
            basis_cache.key_update(hw, &dictname, &keyname, &keydata, None, None, None, false).unwrap();
        }
    }
}