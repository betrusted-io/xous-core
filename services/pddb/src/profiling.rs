#[allow(unused_imports)]
use std::io::{Error, ErrorKind, Read, Write};
use std::thread;

const PROFILING_DICT: &'static str = "pdict";
const NUMKEYS: usize = 200;
const PROFILE_ITERS: usize = 200;

fn gen_guid(trng: &mut trng::Trng) -> String {
    let mut guid = [0u8; 16];
    trng.fill_bytes(&mut guid);
    hex::encode(guid)
}

/// A test that repeatedly queries a dictionary with many entries
pub fn do_query_work() {
    let _ = thread::spawn(|| {
        let xns = xous_names::XousNames::new().unwrap();
        let pddb = pddb::Pddb::new();
        let mut trng = trng::Trng::new(&xns).unwrap();

        let key_count = match pddb.list_keys(PROFILING_DICT, None) {
            Ok(kl) => kl.len(),
            Err(_e) => 0,
        };

        // populate the dictionary if we're short the number of keys we want to have
        log::info!("test setup");
        if key_count < NUMKEYS {
            for _ in 0..NUMKEYS - key_count {
                let guid = gen_guid(&mut trng);
                match pddb.get(PROFILING_DICT, &guid, None, true, true, Some(256), None::<fn()>) {
                    Ok(mut key) => {
                        let mut rdata = [0u8; 192];
                        trng.fill_bytes(&mut rdata);
                        key.write_all(&rdata).ok();
                    }
                    Err(e) => log::error!("couldn't create test key {:?}", e),
                }
            }
        }
        log::info!("starting main loop");
        let modals = modals::Modals::new(&xns).unwrap();
        modals.show_notification("press any key to start profiling run.", None).ok();

        modals.dynamic_notification(Some("running"), None).ok();
        for i in 0..PROFILE_ITERS {
            let klist = pddb.list_keys(PROFILING_DICT, None).unwrap();
            log::info!("iter {}, klist: {}", i, klist.len());
        }
        modals.dynamic_notification_close().ok();
        modals.show_notification("done", None).ok();

        log::info!("quitting");
    });
}
