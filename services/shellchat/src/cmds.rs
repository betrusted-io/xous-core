use core::fmt::Write;
use std::collections::HashMap;

#[cfg(feature = "shellperf")]
use utralib::generated::*;
use xous::MessageEnvelope;
use String;
/////////////////////////// Common items to all commands
pub trait ShellCmdApi<'a> {
    // user implemented:
    // called to process the command with the remainder of the string attached
    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error>;
    // called to process incoming messages that may have been origniated by the most recently issued command
    fn callback(
        &mut self,
        msg: &MessageEnvelope,
        _env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        log::info!("received unhandled message {:?}", msg);
        Ok(None)
    }

    // created with cmd_api! macro
    // checks if the command matches the current verb in question
    fn matches(&self, verb: &str) -> bool;
    // returns my verb
    fn verb(&self) -> &'static str;
}
// the argument to this macro is the command verb
macro_rules! cmd_api {
    ($verb:expr) => {
        fn verb(&self) -> &'static str { stringify!($verb) }
        fn matches(&self, verb: &str) -> bool { if verb == stringify!($verb) { true } else { false } }
    };
}

use trng::*;
/////////////////////////// Command shell integration
pub struct CommonEnv {
    llio: llio::Llio,
    com: com::Com,
    ticktimer: ticktimer_server::Ticktimer,
    gam: gam::Gam,
    cb_registrations: HashMap<u32, String>,
    trng: Trng,
    netmgr: net::NetManager,
    #[allow(dead_code)]
    xns: xous_names::XousNames,
    boot_instant: std::time::Instant,
    /// make this communal so any number of commands can trigger or reset the performance counter, and/or
    /// perform logging
    #[cfg(feature = "shellperf")]
    perf_csr: AtomicCsr<u32>,
    #[cfg(feature = "shellperf")]
    event_csr: AtomicCsr<u32>,
}
impl CommonEnv {
    pub fn register_handler(&mut self, verb: String) -> u32 {
        let mut key: u32;
        loop {
            key = self.trng.get_u32().unwrap();
            // reserve the bottom 1000 IDs for the main loop enums.
            if !self.cb_registrations.contains_key(&key) && (key > 1000) {
                break;
            }
        }
        self.cb_registrations.insert(key, verb);
        key
    }
}

/*
    To add a new command:
        0. ensure that the command implements the ShellCmdApi (above)
        1. mod/use the new command
        2. create an entry for the command's storage in the CmdEnv structure
        3. initialize the persistant storage here
        4. add it to the "commands" array in the dispatch() routine below

    Side note: if your command doesn't require persistent storage, you could,
    technically, generate the command dynamically every time it's called. Echo
    demonstrates this.
*/

///// 1. add your module here, and pull its namespace into the local crate
mod echo;
use echo::*;
mod sleep;
use sleep::*;
mod sensors;
use sensors::*;
// mod callback; use callback::*;
mod rtc_cmd;
use rtc_cmd::*;
mod vibe;
use vibe::*;
mod ssid;
use ssid::*;
mod ver;
use ver::*;
//mod audio;    use audio::*; // this command is currently contra-indicated with PDDB, as the test audio
// currently overlaps the PDDB space. We'll fix this eventually, but for now, let's switch to PDDB mode.
mod backlight;
use backlight::*;
mod accel;
use accel::*;
#[cfg(feature = "dbg-ecupdate")]
mod ecup;
#[cfg(feature = "dbg-ecupdate")]
use ecup::*;
mod trng_cmd;
use trng_cmd::*;
mod console;
use console::*;
//mod memtest;  use memtest::*;
mod keys;
use keys::*;
mod wlan;
use wlan::*;
mod jtag_cmd;
use jtag_cmd::*;
mod net_cmd;
use net_cmd::*;
mod pddb_cmd;
use pddb_cmd::*;
mod usb;
use usb::*;

#[cfg(not(feature = "no-codec"))]
mod test;
#[cfg(not(feature = "no-codec"))]
use test::*;

#[cfg(feature = "tts")]
mod tts;
#[cfg(feature = "tts")]
use tts::*;

#[cfg(feature = "benchmarks")]
mod engine;
#[cfg(feature = "benchmarks")]
use engine::*;
#[cfg(feature = "hashtest")]
mod sha;
#[cfg(feature = "hashtest")]
use sha::*;
#[cfg(feature = "aestests")]
mod aes_cmd;
#[cfg(feature = "aestests")]
use aes_cmd::*;
//mod fcc;      use fcc::*;
//mod pds; // dependency of the FCC file

pub struct CmdEnv {
    common_env: CommonEnv,
    lastverb: String,
    ///// 2. declare storage for your command here.
    sleep_cmd: Sleep,
    sensors_cmd: Sensors,
    //callback_cmd: CallBack,
    rtc_cmd: RtcCmd,
    vibe_cmd: Vibe,
    ssid_cmd: Ssid,
    //audio_cmd: Audio,
    #[cfg(feature = "dbg-ecupdate")]
    ecup_cmd: EcUpdate,
    trng_cmd: TrngCmd,
    //memtest_cmd: Memtest,
    keys_cmd: Keys,
    jtag_cmd: JtagCmd,
    net_cmd: NetCmd,
    pddb_cmd: PddbCmd,
    wlan_cmd: Wlan,
    usb_cmd: Usb,

    #[cfg(not(feature = "no-codec"))]
    test_cmd: Test,

    #[cfg(feature = "tts")]
    tts_cmd: Tts,

    #[cfg(feature = "hashtest")]
    sha_cmd: Sha,
    #[cfg(feature = "aestests")]
    aes_cmd: Aes,
    #[cfg(feature = "benchmarks")]
    engine_cmd: Engine,
    //fcc_cmd: Fcc,
}
impl CmdEnv {
    pub fn new(xns: &xous_names::XousNames) -> CmdEnv {
        let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
        #[cfg(feature = "shellperf")]
        let perf_csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::perfcounter::HW_PERFCOUNTER_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map perfcounter CSR range");
        #[cfg(feature = "shellperf")]
        let event1_csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::event_source1::HW_EVENT_SOURCE1_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map event1 CSR range");

        // _ prefix allows us to leave the `mut` here without creating a warning.
        // the `mut` option is needed for some features.
        let mut _common = CommonEnv {
            llio: llio::Llio::new(&xns),
            com: com::Com::new(&xns).expect("could't connect to COM"),
            ticktimer,
            gam: gam::Gam::new(&xns).expect("couldn't connect to GAM"),
            cb_registrations: HashMap::new(),
            trng: Trng::new(&xns).unwrap(),
            xns: xous_names::XousNames::new().unwrap(),
            netmgr: net::NetManager::new(),
            boot_instant: std::time::Instant::now(),
            #[cfg(feature = "shellperf")]
            perf_csr: AtomicCsr::new(perf_csr.as_mut_ptr() as *mut u32),
            #[cfg(feature = "shellperf")]
            event_csr: AtomicCsr::new(event1_csr.as_mut_ptr() as *mut u32),
        };
        //let fcc = Fcc::new(&mut common);
        #[cfg(feature = "hashtest")]
        let sha = Sha::new(&xns, &mut _common);
        #[cfg(feature = "aestests")]
        let aes = Aes::new(&xns, &mut _common);
        #[cfg(feature = "benchmarks")]
        let engine = Engine::new(&xns, &mut _common);
        //let memtest = Memtest::new(&xns, &mut common);

        // print our version info
        let soc_ver = _common.llio.soc_gitrev().unwrap();
        log::info!("SoC git rev {}", soc_ver.to_string());
        log::info!(
            "{}PDDB.DNA,{:x},{}",
            xous::BOOKEND_START,
            _common.llio.soc_dna().unwrap(),
            xous::BOOKEND_END
        );
        let (rev, dirty) = _common.com.get_ec_git_rev().unwrap();
        let dirtystr = if dirty { "dirty" } else { "clean" };
        log::info!("EC gateware git commit: {:x}, {}", rev, dirtystr);
        let ec_ver = _common.com.get_ec_sw_tag().unwrap();
        log::info!("EC sw tag: {}", ec_ver.to_string());
        let wf_ver = _common.com.get_wf200_fw_rev().unwrap();
        log::info!("WF200 fw rev {}.{}.{}", wf_ver.maj, wf_ver.min, wf_ver.rev);

        CmdEnv {
            common_env: _common,
            lastverb: String::new(),
            ///// 3. initialize your storage, by calling new()
            sleep_cmd: {
                log::debug!("sleep");
                Sleep::new(&xns)
            },
            sensors_cmd: {
                log::debug!("sensors");
                Sensors::new()
            },
            //callback_cmd: CallBack::new(),
            rtc_cmd: {
                log::debug!("rtc");
                RtcCmd::new(&xns)
            },
            vibe_cmd: {
                log::debug!("vibe");
                Vibe::new()
            },
            ssid_cmd: {
                log::debug!("ssid");
                Ssid::new()
            },
            //audio_cmd: Audio::new(&xns),
            #[cfg(feature = "dbg-ecupdate")]
            ecup_cmd: EcUpdate::new(),
            trng_cmd: {
                log::debug!("trng");
                TrngCmd::new()
            },
            //memtest_cmd: memtest,
            keys_cmd: {
                log::debug!("keys");
                Keys::new(&xns)
            },
            jtag_cmd: {
                log::debug!("jtag");
                JtagCmd::new(&xns)
            },
            net_cmd: {
                log::debug!("net");
                NetCmd::new(&xns)
            },
            pddb_cmd: {
                log::debug!("pddb");
                PddbCmd::new(&xns)
            },
            wlan_cmd: {
                log::debug!("wlan");
                Wlan::new()
            },
            usb_cmd: {
                log::debug!("usb");
                Usb::new()
            },

            #[cfg(not(feature = "no-codec"))]
            test_cmd: {
                log::debug!("test");
                Test::new(&xns)
            },

            #[cfg(feature = "tts")]
            tts_cmd: Tts::new(&xns),

            #[cfg(feature = "hashtest")]
            sha_cmd: sha,
            #[cfg(feature = "aestests")]
            aes_cmd: aes,
            #[cfg(feature = "benchmarks")]
            engine_cmd: engine,
            //fcc_cmd: fcc,
        }
    }

    pub fn dispatch(
        &mut self,
        maybe_cmdline: Option<&mut String>,
        maybe_callback: Option<&MessageEnvelope>,
    ) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();

        let mut echo_cmd = Echo {}; // this command has no persistent storage, so we can "create" it every time we call dispatch (but it's a zero-cost absraction so this doesn't actually create any instructions)
        let mut ver_cmd = Ver {};
        let mut backlight_cmd = Backlight {};
        let mut accel_cmd = Accel {};
        let mut console_cmd = Console {};
        let commands: &mut [&mut dyn ShellCmdApi] = &mut [
            ///// 4. add your command to this array, so that it can be looked up and dispatched
            &mut echo_cmd,
            &mut self.sleep_cmd,
            &mut self.sensors_cmd,
            //&mut self.callback_cmd,
            &mut self.rtc_cmd,
            &mut self.vibe_cmd,
            &mut self.ssid_cmd,
            &mut ver_cmd,
            //&mut self.audio_cmd,
            &mut backlight_cmd,
            &mut accel_cmd,
            #[cfg(feature = "dbg-ecupdate")]
            &mut self.ecup_cmd,
            &mut self.trng_cmd,
            &mut console_cmd,
            // &mut self.memtest_cmd,
            &mut self.keys_cmd,
            &mut self.wlan_cmd,
            &mut self.jtag_cmd,
            &mut self.net_cmd,
            &mut self.pddb_cmd,
            &mut self.usb_cmd,
            #[cfg(not(feature = "no-codec"))]
            &mut self.test_cmd,
            #[cfg(feature = "tts")]
            &mut self.tts_cmd,
            #[cfg(feature = "hashtest")]
            &mut self.sha_cmd,
            #[cfg(feature = "aestests")]
            &mut self.aes_cmd,
            #[cfg(feature = "benchmarks")]
            &mut self.engine_cmd,
            //&mut self.fcc_cmd,
        ];

        if let Some(cmdline) = maybe_cmdline {
            let maybe_verb = tokenize(cmdline);

            let mut cmd_ret: Result<Option<String>, xous::Error> = Ok(None);
            if let Some(verb_string) = maybe_verb {
                let verb = verb_string.to_str();

                // search through the list of commands linearly until one matches,
                // then run it.
                let mut match_found = false;
                for cmd in commands.iter_mut() {
                    if cmd.matches(verb) {
                        match_found = true;
                        cmd_ret = cmd.process(*cmdline, &mut self.common_env);
                        self.lastverb.clear();
                        write!(self.lastverb, "{}", verb).expect("SHCH: couldn't record last verb");
                    };
                }

                // if none match, create a list of available commands
                if !match_found {
                    let mut first = true;
                    write!(ret, "Commands: ").unwrap();
                    for cmd in commands.iter() {
                        if !first {
                            ret.push_str(", ");
                        }
                        ret.append(cmd.verb())?;
                        first = false;
                    }
                    Ok(Some(ret))
                } else {
                    cmd_ret
                }
            } else {
                Ok(None)
            }
        } else if let Some(callback) = maybe_callback {
            let mut cmd_ret: Result<Option<String>, xous::Error> = Ok(None);
            // first check and see if we have a callback registration; if not, just map to the last verb
            let verb = match self.common_env.cb_registrations.get(&(callback.body.id() as u32)) {
                Some(verb) => verb.to_str(),
                None => self.lastverb.to_str(),
            };
            // now dispatch
            let mut verbfound = false;
            for cmd in commands.iter_mut() {
                if cmd.matches(verb) {
                    cmd_ret = cmd.callback(callback, &mut self.common_env);
                    verbfound = true;
                    break;
                };
            }
            if verbfound { cmd_ret } else { Ok(None) }
        } else {
            Ok(None)
        }
    }
}

/// extract the first token, as delimited by spaces
/// modifies the incoming line by removing the token and returning the remainder
/// returns the found token
/// note: we don't have split() because of nostd
pub fn tokenize(line: &mut String) -> Option<String> {
    let mut token = String::new();
    let mut retline = String::new();

    let lineiter = line.as_str().chars();
    let mut foundspace = false;
    let mut foundrest = false;
    for ch in lineiter {
        if ch != ' ' && !foundspace {
            token.push(ch).unwrap();
        } else if foundspace && foundrest {
            retline.push(ch).unwrap();
        } else if foundspace && ch != ' ' {
            // handle case of multiple spaces in a row
            foundrest = true;
            retline.push(ch).unwrap();
        } else {
            foundspace = true;
            // consume the space
        }
    }
    line.clear();
    write!(line, "{}", retline.as_str()).unwrap();
    if token.len() > 0 { Some(token) } else { None }
}
