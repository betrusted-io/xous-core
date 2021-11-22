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

pub(crate) fn manual_testcase(hw: &mut PddbOs) {
    log::info!("Initializing disk...");
    hw.pddb_format().unwrap();
    log::info!("Done initializing disk");

    // it's a vector because order is important: by default access to keys/dicts go into the latest entry first, and then recurse to the earliest
    let mut basis_cache = BasisCache::new();

    log::info!("Attempting to mount the PDDB");
    if let Some(sys_basis) = hw.pddb_mount() {
        log::info!("PDDB mount operation finished successfully");
        basis_cache.basis_add(sys_basis);
    } else {
        log::info!("PDDB did not mount; did you remember to format the PDDB region?");
    }
    log::info!("size of vpage: {}", VPAGE_SIZE);

    // add a "system settings" dictionary to the default basis
    log::info!("adding 'system settings' dictionary");
    basis_cache.dict_add(hw, "system settings", None).expect("couldn't add system settings dictionary");
    basis_cache.key_update(hw, "system settings", "wifi/wpa_keys/Kosagi", "my_wpa_key_here".as_bytes(), None, None, None, false).expect("couldn't add a key");
    let mut readback = [0u8; 15];
    match basis_cache.key_read(hw, "system settings", "wifi/wpa_keys/Kosagi", &mut readback, None, None) {
        Ok(readsize) => {
            log::info!("read back {} bytes", readsize);
            log::info!("read data: {}", String::from_utf8_lossy(&readback));
        },
        Err(e) => {
            log::info!("couldn't read data: {:?}", e);
        }
    }
    basis_cache.key_update(hw, "system settings", "wifi/wpa_keys/e4200", "12345678".as_bytes(), None, None, None, false).expect("couldn't add a key");

    // add a "big" key
    let mut bigdata = [0u8; 5000];
    for (i, d) in bigdata.iter_mut().enumerate() {
        *d = i as u8;
    }
    basis_cache.key_update(hw, "system settings", "big_pool1", &bigdata, None, None, None, false).expect("couldn't add a key");

    basis_cache.dict_add(hw, "test_dict_2", None).expect("couldn't add test dictionary 2");
    basis_cache.key_update(hw, "test_dict_2", "test key in dict 2", "some data".as_bytes(), None, Some(128), None, false).expect("couldn't add a key to second dict");

    basis_cache.key_update(hw, "system settings", "wifi/wpa_keys/e4200", "ABC".as_bytes(), Some(2), None, None, false).expect("couldn't update e4200 key");

    log::info!("test readback of wifi/wpa_keys/e4200");
    match basis_cache.key_read(hw, "system settings", "wifi/wpa_keys/e4200", &mut readback, None, None) {
        Ok(readsize) => {
            log::info!("read back {} bytes", readsize);
            log::info!("read data: {}", String::from_utf8_lossy(&readback));
        },
        Err(e) => {
            log::info!("couldn't read data: {:?}", e);
        }
    }
}