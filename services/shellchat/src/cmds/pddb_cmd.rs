use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;
#[allow(unused_imports)]
use std::io::{Write, Read, Seek, SeekFrom};

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
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        #[cfg(not(feature="pddbtest"))]
        let helpstring = "pddb [basislist] [basiscreate] [basisunlock] [basislock] [basisdelete] [default]\n[dictlist] [keylist] [query] [dictdelete] [keydelete]";
        #[cfg(feature="pddbtest")]
        let helpstring = "pddb [basislist] [basiscreate] [basisunlock] [basislock] [basisdelete] [default]\n[dictlist] [keylist] [query] [dictdelete] [keydelete]\n[test]";

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
                            let checked_len = if list.len() > 6 {
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
                        Err(_) => write!(ret, "Error encountered listing dictionaries").ok().unwrap_or(()),
                    }
                }
                #[cfg(feature="pddbtest")]
                "fscbtest" => {
                    let checkval = [0u8, 1u8, 2u8, 3u8];
                    // create a secret basis, put a test key in it
                    log::info!("create basis");
                    self.pddb.create_basis("fscbtest").ok();
                    self.pddb.unlock_basis("fscbtest", None).ok();
                    log::info!("write test key");
                    let mut persistence_test = self.pddb.get(
                        "persistent",
                        "key1",
                        None, true, true,
                        Some(64),
                        None::<fn()>
                    ).unwrap();
                    persistence_test.write(&checkval).unwrap();
                    self.pddb.sync().ok();
                    // unmount the test basis
                    log::info!("unmount basis");
                    self.pddb.lock_basis("fscbtest").ok();

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
                    // check that secret basis is still there
                    log::info!("confirm test basis");
                    self.pddb.unlock_basis("fscbtest", None).ok();
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
                    }
                    for (&a, &b) in checkval.iter().zip(readback.iter()) {
                        if a != b {
                            passing = false;
                            log::error!("readback data corruption: {} vs {}", a, b);
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
