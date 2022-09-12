use crate::{ShellCmdApi, CommonEnv};
#[cfg(all(feature="pddbtest", feature="autobasis"))]
use pddb::PDDB_A_LEN;
use xous_ipc::String;
#[allow(unused_imports)]
use std::io::{Write, Read, Seek, SeekFrom};
use core::fmt::Write as FmtWrite;

pub struct PddbCmd {
    pddb: pddb::Pddb,
}
impl PddbCmd {
    pub fn new(_xns: &xous_names::XousNames) -> PddbCmd {
        PddbCmd {
            pddb: pddb::Pddb::new(),
        }
    }
}

impl<'a> ShellCmdApi<'a> for PddbCmd {
    cmd_api!(pddb); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        #[cfg(not(feature="pddbtest"))]
        let helpstring = "pddb [basislist] [basiscreate] [basisunlock] [basislock] [basisdelete] [default]\n[dictlist] [keylist] [query] [dictdelete] [keydelete] [churn] [flush] [sync]";
        #[cfg(feature="pddbtest")]
        let helpstring = "pddb [basislist] [basiscreate] [basisunlock] [basislock] [basisdelete] [default]\n[dictlist] [keylist] [query] [dictdelete] [keydelete] [churn] [flush] [sync]\n[test]";

        let mut tokens = args.as_str().unwrap().split(' ');
        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "basislist" => {
                    let bases = self.pddb.list_basis();
                    for basis in bases {
                        write!(ret, "{}\n", basis).unwrap();
                    }
                    /* // example of using .get with a callback
                    self.pddb.get("foo", "bar", None, false, false,
                        Some({
                            let cid = cid.clone();
                            let counter = self.counter.clone();
                            move || {
                            xous::send_message(cid, xous::Message::new_scalar(0, counter as usize, 0, 0, 0)).expect("couldn't send");
                        }})
                    ).unwrap();*/
                }
                "default" => {
                    match self.pddb.latest_basis() {
                        Some(latest) => write!(ret, "The current default basis is: {}", latest).unwrap(),
                        None => write!(ret, "No open basis detected").unwrap(),
                    }
                }
                "basiscreate" => {
                    if let Some(bname) = tokens.next() {
                        match self.pddb.create_basis(bname) {
                            Ok(_) => write!(ret, "basis {} created successfully", bname).unwrap(),
                            Err(e) => write!(ret, "basis {} could not be created: {:?}", bname, e).unwrap(),
                        }
                    } else {
                        write!(ret, "usage: pddb basiscreate [basis name]").unwrap()
                    }
                }
                "basisunlock" => {
                    if let Some(bname) = tokens.next() {
                        match self.pddb.unlock_basis(bname, None) {
                            Ok(_) => write!(ret, "basis {} unlocked successfully", bname).unwrap(),
                            Err(e) => write!(ret, "basis {} could not be unlocked: {:?}", bname, e).unwrap(),
                        }
                    } else {
                        write!(ret, "usage: pddb basisunlock [basis name]").unwrap()
                    }
                }
                "basislock" => {
                    if let Some(bname) = tokens.next() {
                        match self.pddb.lock_basis(bname) {
                            Ok(_) => write!(ret, "basis {} locked successfully", bname).unwrap(),
                            Err(e) => write!(ret, "basis {} could not be locked: {:?}", bname, e).unwrap(),
                        }
                    } else {
                        write!(ret, "usage: pddb basisunlock [basis name]").unwrap()
                    }
                }
                "basisdelete" => {
                    if let Some(bname) = tokens.next() {
                        match self.pddb.delete_basis(bname) {
                            Ok(_) => write!(ret, "basis {} deleted successfully", bname).unwrap(),
                            Err(e) => write!(ret, "basis {} could not be deleted: {:?}", bname, e).unwrap(),
                        }
                    } else {
                        write!(ret, "usage: pddb basisdelete [basis name]").unwrap()
                    }
                }
                "query" => {
                    if let Some(descriptor) = tokens.next() {
                        if let Some((dict, keyname)) = descriptor.split_once(':') {
                            match self.pddb.get(dict, keyname, None,
                                false, false, None, None::<fn()>) {
                                Ok(mut key) => {
                                    let mut readbuf = [0u8; 512]; // up to the first 512 chars of the key
                                    match key.read(&mut readbuf) {
                                        Ok(len) => {
                                            match std::string::String::from_utf8(readbuf[..len].to_vec()) {
                                                Ok(s) => {
                                                    write!(ret, "{}", s).unwrap();
                                                }
                                                _ => {
                                                    for &b in readbuf[..len].iter() {
                                                        match write!(ret, "{:02x} ", b) {
                                                            Ok(_) => (),
                                                            Err(_) => break, // we can overflow our return buffer returning hex chars
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        _ => write!(ret, "Error encountered reading {}:{}", dict, keyname).unwrap()
                                    }
                                }
                                _ => write!(ret, "{}:{} not found or other error", dict, keyname).unwrap()
                            }
                        } else {
                            write!(ret, "Query is of form 'dict:key'").unwrap();
                        }
                    } else {
                        write!(ret, "Missing query of form 'dict:key'").unwrap();
                    }
                }
                "write" => {
                    if let Some(descriptor) = tokens.next() {
                        if let Some((dict, keyname)) = descriptor.split_once(':') {
                            match self.pddb.get(dict, keyname, None,
                                true, true, Some(256), None::<fn()>) {
                                Ok(mut key) => {
                                    let mut val = String::<1024>::new();
                                    join_tokens(&mut val, &mut tokens);
                                    if val.len() > 0 {
                                        match key.write(&val.as_bytes()[..val.len()]) {
                                            Ok(len) => {
                                                self.pddb.sync().ok();
                                                write!(ret, "Wrote {} bytes to {}:{}", len, dict, keyname).ok();
                                            }
                                            Err(e) => {
                                                write!(ret, "Error writing {}:{}: {:?}", dict, keyname, e).ok();
                                            }
                                        }
                                    } else {
                                        write!(ret, "Created an empty key {}:{}", dict, keyname).ok();
                                    }
                                }
                                _ => write!(ret, "{}:{} not found or other error", dict, keyname).unwrap()
                            }
                        } else {
                            write!(ret, "Query is of form 'dict:key'").unwrap();
                        }
                    } else {
                        write!(ret, "Missing query of form 'dict:key'").unwrap();
                    }
                }
                "keydelete" => {
                    if let Some(descriptor) = tokens.next() {
                        if let Some((dict, keyname)) = descriptor.split_once(':') {
                            match self.pddb.delete_key(dict, keyname, None) {
                                Ok(_) => {
                                    write!(ret, "Deleted {}:{}\n", dict, keyname).unwrap();
                                    // you must call sync after all deletions are done
                                    write!(ret, "Sync: {}",
                                        self.pddb.sync()
                                        .map_or_else(|e| e.to_string(), |_| "Ok".to_string())
                                    ).unwrap();
                                }
                                Err(e) => write!(ret, "{}:{} not found or other error: {:?}", dict, keyname, e).unwrap(),
                            }
                        } else {
                            write!(ret, "Specify key with form 'dict:key'").unwrap();
                        }
                    } else {
                        write!(ret, "Missing spec of form 'dict:key'").unwrap();
                    }
                }
                "dictdelete" => {
                    if let Some(dict) = tokens.next() {
                        match self.pddb.delete_dict(dict, None) {
                            Ok(_) => {
                                write!(ret, "Deleted dictionary {}\n", dict).unwrap();
                                // you must call sync after all deletions are done
                                write!(ret, "Sync: {}",
                                    self.pddb.sync()
                                    .map_or_else(|e| e.to_string(), |_| "Ok".to_string())
                                ).unwrap();
                            }
                            Err(e) => write!(ret, "{} not found or other error: {:?}", dict, e).unwrap()
                        }
                    } else {
                        write!(ret, "Missing dictionary name").unwrap();
                    }
                }
                "keylist" => {
                    if let Some(dict) = tokens.next() {
                        match self.pddb.list_keys(dict, None) {
                            Ok(list) => {
                                let checked_len = if list.len() > 10 {
                                    write!(ret, "First 10 keys of {}:", list.len()).unwrap();
                                    10
                                } else {
                                    list.len()
                                };
                                for i in 0..checked_len {
                                    let sep = if i != checked_len - 1 {
                                        ",\n"
                                    } else {
                                        ""
                                    };
                                    match write!(ret, "{}{}", list[i], sep) {
                                        Ok(_) => (),
                                        Err(_) => break, // overflowed return buffer
                                    }
                                }
                            }
                            Err(_) => write!(ret, "{} does not exist or other error", dict).ok().unwrap_or(()),
                        }
                    } else {
                        write!(ret, "Missing dictionary name").unwrap();
                    }
                }
                "dictlist" => {
                    match self.pddb.list_dict(None) {
                        Ok(list) => {
                            let checked_len = if list.len() > 10 {
                                write!(ret, "First 10 dicts of {}:", list.len()).unwrap();
                                10
                            } else {
                                list.len()
                            };
                            for i in 0..checked_len {
                                let sep = if i != checked_len - 1 {
                                    ",\n"
                                } else {
                                    ""
                                };
                                match write!(ret, "{}{}", list[i], sep) {
                                    Ok(_) => (),
                                    Err(_) => break, // overflowed return buffer
                                }
                            }
                        }
                        Err(e) => write!(ret, "Error encountered listing dictionaries: {:?}", e).ok().unwrap_or(()),
                    }
                }
                "churn" => {
                    write!(ret, "Sync result code: {:?}\n", self.pddb.sync()).ok();
                    write!(ret, "Churn result code: {:?}", self.pddb.rekey_pddb(pddb::PddbRekeyOp::Churn)).ok();
                }
                "flush" => {
                    write!(ret, "Sync result code: {:?}\n", self.pddb.sync()).ok();
                    write!(ret, "Flush result code: {:?}", self.pddb.flush_space_update()).ok();
                }
                "sync" => {
                    write!(ret, "Sync result code: {:?}\n", self.pddb.sync()).ok();
                    log::info!("{}PDDB.SYNCDONE,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                }
                #[cfg(feature="test-rekey")]
                "rekey" => {
                    let old_dna = if let Some(dna_str) = tokens.next() {
                        dna_str.parse::<u64>().unwrap_or(0)
                    } else {
                        0
                    };
                    log::info!("rekey result: {:?}", self.pddb.rekey_pddb(pddb::PddbRekeyOp::FromDnaFast(old_dna)));
                }
                #[cfg(feature="pddbtest")]
                "dump" => {
                    self.pddb.dbg_dump("full").unwrap();
                }
                #[cfg(feature="pddbtest")]
                "largetest" => {
                    // fill memory with junk
                    // 128k chunks of junk
                    const JUNK_CHUNK: usize = 131072;
                    log::info!("fill junk");
                    for index in 0..28 { // write ~3 megs of junk, should trigger FSCB unlock at least once...
                        let mut junk = Vec::<u8>::new();
                        // 128k chunk of junk
                        for i in 0..JUNK_CHUNK {
                            junk.push((i + index) as u8);
                        }
                        let junkname = format!("junk{}", index);
                        match self.pddb.get(
                            "junk",
                            &junkname,
                            None, true, true,
                            Some(JUNK_CHUNK),
                            None::<fn()>
                        ) {
                            Ok(mut junk_key) => {
                                match junk_key.write_all(&junk) {
                                    Ok(_) => {
                                        log::info!("wrote {} of len {}", junkname, JUNK_CHUNK);
                                    }
                                    Err(e) => {
                                        log::error!("couldn't write {}: {:?}", junkname, e);
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("couldn't allocate junk key {}: {:?}", junkname, e);
                            }
                        }
                    }
                    log::info!("check junk");
                    let mut pass = true;
                    for index in 0..28 { // write ~3 megs of junk, should trigger FSCB unlock at least once...
                        let mut junk = Vec::<u8>::new();
                        // 128k chunk of junk
                        for i in 0..JUNK_CHUNK {
                            junk.push((i + index) as u8);
                        }
                        let junkname = format!("junk{}", index);
                        match self.pddb.get(
                            "junk",
                            &junkname,
                            None, false, false,
                            None,
                            None::<fn()>
                        ) {
                            Ok(mut junk_key) => {
                                let mut checkbuf = Vec::new();
                                match junk_key.read_to_end(&mut checkbuf) {
                                    Ok(len) => {
                                        log::info!("read back {} bytes", len);
                                        let mut matched = true;
                                        let mut errcount = 0;
                                        for (index, (&a, &b)) in checkbuf.iter().zip(junk.iter()).enumerate() {
                                            if a != b {
                                                matched = false;
                                                if errcount < 16 {
                                                    log::info!("match failure at {}: a:{}, b:{}", index, a, b);
                                                }
                                                errcount += 1;
                                            }
                                        }
                                        if !matched || len != JUNK_CHUNK {
                                            pass = false;
                                            log::error!("failed to verify {}", junkname);
                                        } else {
                                            log::info!("no errors in {}", junkname);
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("couldn't read {}: {:?}", junkname, e);
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("couldn't access junk key {}: {:?}", junkname, e);
                            }
                        }
                    }
                    if pass {
                        write!(ret, "largetest passed").ok();
                        log::info!("largetest passed");
                    } else {
                        write!(ret, "largetest failed").ok();
                        log::info!("largetest failed");
                    }
                }
                #[cfg(all(feature="pddbtest", feature="autobasis"))]
                "btest" => {
                    // This test will:
                    //   - generate bases iteratively:
                    //     - populate each with generated keys (at least 2 of each) specified as follows:
                    //       - small with an explicit basis
                    //       - large with an implicit basis (tests default basis determination)
                    //     - close the even basis as we create them (this exercises default basis determination and sets up for the next test)
                    //     - shove ~128k of "junk" data into the System basis
                    //   This proceeds until about 75% of the capacity of the disk is used up.
                    //
                    // The test will then unlock all the generated Basis, and confirm their contents are intact.

                    // generate Basis until we've either exhausted the limit of our config vector (32 entries),
                    // or we've filled up "enough" of the space to exercise the FSCB mechanism.
                    let mut used = 0;
                    let mut b = 0;
                    while b < 32 && used < (PDDB_A_LEN as f32 * 0.75) as usize {
                        let bname = format!("test{}", b);
                        // create & mount the test basis: this is a condensed function that
                        // will do either a create/open op or close op on any of 32 bases specified as an array to the argument.

                        // as-coded, this will incrementally open each secret basis and try to write specifically to each newly
                        // created basis "by name".
                        let mut config = [None::<bool>; 32];
                        config[b as usize] = Some(true);
                        self.pddb.basis_testing(&config);
                        log::info!("created test basis {}", b);

                        for sub in 0..2 {
                            let small_key = make_vector(b, VectorType::Small(b + sub));
                            used += small_key.len();
                            let sname = format!("small{}", sub);
                            match self.pddb.get(
                                "btest",
                                &sname,
                                // this uses an explicit specifier
                                Some(&bname), true, true,
                                None,
                                None::<fn()>
                            ) {
                                Ok(mut k) => {
                                    match k.write_all(&small_key) {
                                        Ok(_) => (),
                                        Err(e) => log::error!("small key fill on basis {} failed: {:?}", bname, e),
                                    }
                                }
                                Err(e) => log::error!("small key fill on basis {} failed: {:?}", bname, e),
                            }

                            let large_key = make_vector(b, VectorType::Large(b + sub));
                            used += large_key.len();
                            let lname = format!("large{}", sub);
                            match self.pddb.get(
                                "btest",
                                &lname,
                                // this uses the "none" specifier -- but should write to the same basis, implicitly as the small one
                                None, true, true,
                                None,
                                None::<fn()>
                            ) {
                                Ok(mut k) => {
                                    match k.write_all(&large_key) {
                                        Ok(_) => (),
                                        Err(e) => log::error!("small key fill on basis {} failed: {:?}", bname, e),
                                    }
                                }
                                Err(e) => log::error!("small key fill on basis {} failed: {:?}", bname, e),
                            }
                        }
                        if b % 2 == 0 {
                            // unmount every other basis as we create them, just to make things interesting
                            config[b] = Some(false);
                            self.pddb.basis_testing(&config);
                        }
                        let junk = make_vector(b, VectorType::Junk);
                        match self.pddb.get(
                            "junk",
                            &b.to_string(),
                            // this uses an explicit specifier
                            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), true, true,
                            None,
                            None::<fn()>
                        ) {
                            Ok(mut k) => {
                                match k.write_all(&junk) {
                                    Ok(_) => (),
                                    Err(e) => log::error!("junk key fill on basis {} failed: {:?}", bname, e),
                                }
                            }
                            Err(e) => log::error!("junk key fill on basis {} failed: {:?}", bname, e),
                        }
                        used += junk.len();

                        self.pddb.dbg_dump(&format!("btest{}", b)).unwrap();
                        let blist = self.pddb.list_basis();
                        log::info!("Iter {} / Currently open Bases: {:?}", b, blist);
                        log::info!("Currently used: {} bytes", used);
                        b += 1;
                    }
                    log::info!("-------------- generation complete, now verifying -----------------");
                    self.pddb.dbg_dump("btest_final").unwrap(); // this will also export all the extra basis keys in this mode
                    self.pddb.dbg_remount().unwrap();

                    let mut checked = 0;
                    // this unlocks all the Bases
                    let mut config = [None::<bool>; 32];
                    for i in 0..b {
                        config[i] = Some(true);
                    }
                    self.pddb.basis_testing(&config);
                    let max_b = b;

                    // now iterate through and check the Bases
                    let mut pass = true;
                    let mut errcount = 0;
                    const ERRTHRESH: usize = 32;
                    for b in 0..max_b {
                        let bname = format!("test{}", b);
                        // create & mount the test basis: this is a condensed function that
                        // will do either a create/open op or close op on any of 32 bases specified as an array to the argument.

                        // as-coded, this will incrementally open each secret basis and try to write specifically to each newly
                        // created basis "by name".
                        log::info!("checking basis {}", b);

                        for sub in 0..2 {
                            let small_key = make_vector(b, VectorType::Small(b + sub));
                            used += small_key.len();
                            let sname = format!("small{}", sub);
                            match self.pddb.get(
                                "btest",
                                &sname,
                                Some(&bname), true, true,
                                None,
                                None::<fn()>
                            ) {
                                Ok(mut k) => {
                                    let mut check = Vec::<u8>::new();
                                    match k.read_to_end(&mut check) {
                                        Ok(len) => {
                                            checked += len;
                                            if check.len() != small_key.len() {
                                                pass = false;
                                                log::error!("small key size mismatch {}:{}:{} - {}->{}",
                                                    bname, "btest", sname, small_key.len(), check.len()
                                                );
                                            } else {
                                                for (index, (&a, &b)) in check.iter().zip(small_key.iter()).enumerate() {
                                                    if a != b {
                                                        pass = false;
                                                        errcount += 1;
                                                        if errcount < ERRTHRESH {
                                                            log::error!("small key data mismatch {}:{}:{} @ {} 0x{:x}->0x{:x}",
                                                                bname, "btest", sname, index, a, b
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        Err(e) => log::error!("small key check on basis {} failed: {:?}", bname, e),
                                    }
                                }
                                Err(e) => log::error!("small key check on basis {} failed: {:?}", bname, e),
                            }

                            let large_key = make_vector(b, VectorType::Large(b + sub));
                            used += large_key.len();
                            let lname = format!("large{}", sub);
                            match self.pddb.get(
                                "btest",
                                &lname,
                                Some(&bname), true, true,
                                None,
                                None::<fn()>
                            ) {
                                Ok(mut k) => {
                                    let mut check = Vec::<u8>::new();
                                    match k.read_to_end(&mut check) {
                                        Ok(len) => {
                                            checked += len;
                                            if check.len() != large_key.len() {
                                                pass = false;
                                                log::error!("large key size mismatch {}:{}:{} - {}->{}",
                                                    bname, "btest", sname, large_key.len(), check.len()
                                                );
                                            } else {
                                                for (index, (&a, &b)) in check.iter().zip(large_key.iter()).enumerate() {
                                                    if a != b {
                                                        pass = false;
                                                        errcount += 1;
                                                        if errcount < ERRTHRESH {
                                                            log::error!("large key data mismatch {}:{}:{} @ {} 0x{:x}->0x{:x}",
                                                                bname, "btest", sname, index, a, b
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        Err(e) => log::error!("large key check on basis {} failed: {:?}", bname, e),
                                    }
                                }
                                Err(e) => log::error!("large key fill on basis {} failed: {:?}", bname, e),
                            }
                        }
                        let junk = make_vector(b, VectorType::Junk);
                        match self.pddb.get(
                            "junk",
                            &b.to_string(),
                            // this uses an explicit specifier
                            Some(pddb::PDDB_DEFAULT_SYSTEM_BASIS), true, true,
                            None,
                            None::<fn()>
                        ) {
                            Ok(mut k) => {
                                let mut check = Vec::<u8>::new();
                                match k.read_to_end(&mut check) {
                                    Ok(len) => {
                                        checked += len;
                                        if check.len() != junk.len() {
                                            pass = false;
                                            log::error!("junk key size mismatch {}:{}:{} - {}->{}",
                                                pddb::PDDB_DEFAULT_SYSTEM_BASIS, "junk", &b.to_string(), junk.len(), check.len()
                                            );
                                        } else {
                                            for (index, (&a, &b)) in check.iter().zip(junk.iter()).enumerate() {
                                                if a != b {
                                                    pass = false;
                                                    errcount += 1;
                                                    if errcount < ERRTHRESH {
                                                        log::error!("junk key data mismatch {}:{}:{} @ {} 0x{:x}->0x{:x}",
                                                            pddb::PDDB_DEFAULT_SYSTEM_BASIS, "junk", &b.to_string(), index, a, b
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    Err(e) => log::error!("junk key check on basis {} failed: {:?}", bname, e),
                                }                            }
                            Err(e) => log::error!("junk key fill on basis {} failed: {:?}", bname, e),
                        }
                        log::info!("Iter {} of checking", b);
                        log::info!("Currently checked: {} bytes", checked);
                    }
                    if pass {
                        log::info!("basis stress test passed");
                        write!(ret, "basis stress test passed").ok();
                        log::info!("CI done");
                    } else {
                        log::info!("basis stress test failed: {} errors", errcount);
                        write!(ret, "basis stress test failed: {} errors", errcount).ok();
                        log::info!("CI done");
                    }
                }
                #[cfg(feature="pddbtest")]
                "fscbtest" => {
                    let mut checkval = Vec::new();
                    for index in 0..17_000 {
                        checkval.push(index as u8);
                    }
                    // create a secret basis, put a test key in it
                    log::info!("create basis");
                    self.pddb.create_basis("fscbtest").ok();
                    self.pddb.unlock_basis("fscbtest", None).ok();
                    log::info!("write test key");
                    let mut persistence_test = self.pddb.get(
                        "persistent",
                        "key1",
                        None, true, true,
                        None,
                        None::<fn()>
                    ).unwrap();
                    persistence_test.write_all(&checkval).unwrap();
                    self.pddb.sync().ok();
                    self.pddb.dbg_dump("fscb_test1").unwrap();
                    // unmount the test basis
                    log::info!("unmount basis");
                    self.pddb.lock_basis("fscbtest").ok();

                    // fill memory with junk
                    // 128k chunks of junk
                    const JUNK_CHUNK: usize = 131072;
                    log::info!("fill junk");
                    for index in 0..17 { // write ~2 megs of junk, should trigger FSCB unlock at least once...
                        let mut junk = Vec::<u8>::new();
                        // 128k chunk of junk
                        for i in 0..JUNK_CHUNK {
                            junk.push((i + index) as u8);
                        }
                        let junkname = format!("junk{}", index);
                        match self.pddb.get(
                            "junk",
                            &junkname,
                            None, true, true,
                            Some(JUNK_CHUNK),
                            None::<fn()>
                        ) {
                            Ok(mut junk_key) => {
                                match junk_key.write_all(&junk) {
                                    Ok(_) => {
                                        log::info!("wrote {} of len {}", junkname, JUNK_CHUNK);
                                    }
                                    Err(e) => {
                                        log::error!("couldn't write {}: {:?}", junkname, e);
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("couldn't allocate junk key {}: {:?}", junkname, e);
                            }
                        }

                    }
                    // check that secret basis is still there
                    log::info!("confirm test basis");
                    self.pddb.unlock_basis("fscbtest", None).ok();
                    self.pddb.dbg_dump("fscb_test2").unwrap();
                    let mut persistence_test = self.pddb.get(
                        "persistent",
                        "key1",
                        None, true, true,
                        Some(64),
                        None::<fn()>
                    ).unwrap();
                    let mut readback = Vec::<u8>::new();
                    persistence_test.read_to_end(&mut readback).unwrap();
                    let mut passing = true;
                    if readback.len() != checkval.len() {
                        passing = false;
                        log::error!("readback length is different: {:x?}, {:x?}", readback, checkval);
                    } else {
                        log::info!("readback len: 0x{:x}", readback.len());
                    }
                    let mut failures = 0;
                    for (index, (&a, &b)) in checkval.iter().zip(readback.iter()).enumerate() {
                        if a != b {
                            passing = false;
                            if failures < 64 {
                                log::error!("readback data corruption at {}: {} vs {}", index, a, b);
                            }
                            failures += 1;
                        }
                    }
                    if passing {
                        log::info!("fscb test passed");
                        write!(ret, "fscb test passed").ok();
                    } else {
                        log::info!("fscb test failed");
                        write!(ret, "fscb test failed").ok();
                    }
                }
                // note that this feature only works in hosted mode
                #[cfg(feature="pddbtest")]
                "test" => {
                    let bname = tokens.next();
                    // zero-length key test
                    let test_handle = pddb::Pddb::new();
                    // build a key, but don't write to it.
                    let _ = test_handle.get(
                        "test",
                        "zerolength",
                        None, true, true,
                        Some(8),
                        None::<fn()>,
                    ).expect("couldn't build empty key");
                    self.pddb.sync().unwrap();
                    if let Some(name) = bname {
                        match self.pddb.lock_basis(name) {
                            Ok(_) => log::info!("basis {} lock successful", name),
                            Err(e) => log::info!("basis {} could not be unmounted: {:?}", name, e),
                        }
                    }
                    self.pddb.dbg_remount().unwrap();
                    if let Some(name) = bname {
                        match self.pddb.unlock_basis(name, None) {
                            Ok(_) => log::info!("basis {} unlocked successfully", name),
                            Err(e) => log::info!("basis {} could not be unlocked: {:?}", name, e),
                        }
                    }
                    self.pddb.dbg_dump("std_test1").unwrap();
                    write!(ret, "dumped std_test1\n").unwrap();
                    log::info!("finished zero-length alloc");

                    // delete this dictionary with a zero-length key.
                    self.pddb.delete_dict("test", None).expect("couldn't delete test dictionary");
                    self.pddb.sync().unwrap();
                    self.pddb.dbg_dump("std_test2").unwrap();
                    write!(ret, "dumped std_test2\n").unwrap();
                    log::info!("finished dict delete with zero-length key");

                    // seek test - a bunch of terrible, handcrafted test cases to exercise Start, Current, End cases of seeking.
                    let test_handle = pddb::Pddb::new();
                    // build a key, but don't write to it.
                    let mut seekwrite = test_handle.get(
                        "test",
                        "seekwrite",
                        None, true, true,
                        Some(64),
                        Some(|| {
                            log::info!("test:seekwrite key was unmounted");
                        })
                    ).expect("couldn't build empty key");
                    // 1, 1, 1, 1
                    log::info!("wrote {} bytes at offset 0",
                        seekwrite.write(&[1, 1, 1, 1]).unwrap()
                    );
                    log::info!("seek to {}",
                        seekwrite.seek(SeekFrom::Current(-2)).unwrap()
                    );
                    // 1, 1, 2, 2, 2, 2
                    log::info!("wrote {} bytes at offset 2",
                        seekwrite.write(&[2, 2, 2, 2]).unwrap()
                    );
                    if let Some(name) = bname {
                        match self.pddb.lock_basis(name) {
                            Ok(_) => log::info!("basis {} lock successful", name),
                            Err(e) => log::info!("basis {} could not be unmounted: {:?}", name, e),
                        }
                    }
                    if let Some(name) = bname {
                        match self.pddb.unlock_basis(name, None) {
                            Ok(_) => log::info!("basis {} unlocked successfully", name),
                            Err(e) => log::info!("basis {} could not be unlocked: {:?}", name, e),
                        }
                    }
                    // 1, 1, 2, 2, 2, 2, 0, 0, 3, 3
                    log::info!("seek to {}",
                        seekwrite.seek(SeekFrom::Start(8)).unwrap()
                    );
                    log::info!("wrote {} bytes at offset 8",
                        seekwrite.write(&[3, 3]).unwrap()
                    );
                    // 1, 1, 2, 2, 2, 2, 0, 10, 3, 3
                    log::info!("seek to {}",
                        seekwrite.seek(SeekFrom::End(-3)).unwrap()
                    );
                    log::info!("wrote {} bytes at offset 8",
                        seekwrite.write(&[10]).unwrap()
                    );
                    let mut readout = [0u8; 64];
                    let check = [1u8, 1u8, 2u8, 2u8, 2u8, 2u8, 0u8, 10u8, 3u8, 3u8];
                    seekwrite.seek(SeekFrom::Start(0)).unwrap();
                    log::info!("read {} bytes from 0", seekwrite.read(&mut readout).unwrap());
                    let mut pass = true;
                    for (i, (&src, &dst)) in readout.iter().zip(check.iter()).enumerate() {
                        if src != dst {
                            log::info!("mismatch at {}: read {}, check {}", i, src, dst);
                            pass = false;
                        }
                    }
                    if pass {
                        log::info!("check 1 PASSED");
                    } else {
                        log::info!("check 1 FAILED");
                    }
                    seekwrite.seek(SeekFrom::Start(7)).unwrap();
                    let mut readout2 = [0u8];
                    log::info!("read {} bytes from 7", seekwrite.read(&mut readout2).unwrap());
                    log::info!("readout2: {}, should be 10", readout2[0]);

                    self.pddb.sync().unwrap();
                    self.pddb.dbg_remount().unwrap();
                    self.pddb.dbg_dump("std_test3").unwrap();
                    write!(ret, "dumped std_test3\n").unwrap();

                    // creeping extend test
                    self.pddb.delete_key("wlan.networks", "testkey", None).ok();
                    let mut testdata = "".to_string();
                    let mut len = 0;
                    for i in 0..20 {
                        let mut testkey = self.pddb.get("wlan.networks", "testkey", None,
                        false, true, None, None::<fn()>).expect("couldn't make test key");
                        testdata.push_str(&i.to_string());
                        // testkey.seek(SeekFrom::Start(0)).ok();
                        len = testkey.write(testdata.as_bytes()).expect("couldn't write");
                        // self.pddb.sync().ok();
                        self.pddb.dbg_remount().unwrap();
                    }
                    let mut testkey_rbk = self.pddb.get("wlan.networks", "testkey", None,
                    false, true, None, None::<fn()>).expect("couldn't make test key");
                    let mut rbkdata = Vec::<u8>::new();
                    let rlen = testkey_rbk.read_to_end(&mut rbkdata).expect("couldn't read back");
                    if len != rlen {
                        log::info!("failed: written length and read back length of extended key does not match {} vs {}", len, rlen);
                        log::info!("written: {:x?}", testdata.as_bytes());
                        log::info!("readback: {:x?}", &rbkdata);
                    } else {
                        let mut passed = true;
                        let wcheck = testdata.as_bytes();
                        for (&a, &b) in wcheck.iter().zip(rbkdata.iter()) {
                            if a != b {
                                log::info!("error: a: {}, b: {}", a, b);
                                passed = false;
                            }
                        }
                        if passed {
                            log::info!("extension test passed");
                        } else {
                            log::info!("extension test failed");
                        }
                    }
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }

        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}

#[cfg(all(feature="pddbtest", feature="autobasis"))]
enum VectorType {
    Small(usize),
    Large(usize),
    Junk,
}
#[cfg(all(feature="pddbtest", feature="autobasis"))]
const SMALL_SIZE: usize = 2011;
#[cfg(all(feature="pddbtest", feature="autobasis"))]
const LARGE_SIZE: usize = 28813;
#[cfg(all(feature="pddbtest", feature="autobasis"))]
const JUNK_SIZE: usize = 128 * 1024 - 2;
#[cfg(all(feature="pddbtest", feature="autobasis"))]
fn make_vector(basis_number: usize, vtype: VectorType) -> Vec::<u8> {
    use rand::prelude::*;
    use rand_chacha::ChaCha8Rng;

    let mut vector = Vec::<u8>::new();
    // seed format:
    // bottom 0xFFFF is reserved for the basis_number
    // next 0xFFF is resrved for the vector number
    // 0x8000_0000 when set means small, not set means large
    let typemod = match vtype {
        VectorType::Small(n) => 0x1_0000 * (n as u64) + 0x8000_0000,
        VectorType::Large(n) => 0x1_0000 * (n as u64) + 0x0000_0000,
        VectorType::Junk => 0x1_0000_000,
    };
    let mut rng = ChaCha8Rng::seed_from_u64(basis_number as u64 + typemod);
    match vtype {
        VectorType::Small(n) => {
            // multiply the vector number by some odd value so the vectors are not same-sized
            for _ in 0..SMALL_SIZE + 7 * n + basis_number {
                vector.push(rng.gen());
            }
        }
        VectorType::Large(n) => {
            // multiply the vector number by some odd value so the vectors are not same-sized
            for _ in 0..LARGE_SIZE + 1117 * n + basis_number {
                vector.push(rng.gen());
            }
        }
        VectorType::Junk => {
            for _ in 0..JUNK_SIZE + basis_number {
                vector.push(rng.gen());
            }
        }
    }
    vector
}

fn join_tokens<'a>(buf: &mut String<1024>, tokens: impl Iterator<Item = &'a str>) {
    for (i, tok) in tokens.enumerate() {
        if i == 0 {
            write!(buf, "{}", tok).unwrap();
        } else {
            write!(buf, " {}", tok).unwrap();
        }
    }
}
