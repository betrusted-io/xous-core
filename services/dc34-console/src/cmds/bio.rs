// bio.rs - REPL command that receives a BIO program over serial and runs it.

use core::fmt::Write;
use std::io::Read;
use std::io::Write as FsWrite;
use std::time::Duration;

use bao1x_api::bio::*;
use bao1x_api::bio_resources::*;
use bao1x_hal::bio::{Bio, CoreCsr};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use dc34_api::{DC34_BIO, DC34_BIO_CLK, DC34_BIO_PINS, DC34_DICT};
use pddb::Pddb;

use crate::{CommonEnv, ShellCmdApi};

// -- Constants ----------------------------------------------------------------

const CHUNK_DATA_SIZE: usize = 64;
const CHUNK_INDEX_BYTES: usize = 2;
const CHUNK_CRC_BYTES: usize = 4;
/// Total decoded wire size per chunk.
const CHUNK_WIRE_SIZE: usize = CHUNK_INDEX_BYTES + CHUNK_DATA_SIZE + CHUNK_CRC_BYTES; // 70
/// Size of BIO memory in 32-bit words
const BIO_MEM_BYTES: usize = 0xf00;
const NUM_CHUNKS: usize = BIO_MEM_BYTES / CHUNK_DATA_SIZE;
const ALLOWED_PINS: [u8; 4] = [21, 22, 30, 31];

// -- Command state -------------------------------------------------------------
fn check_pins(pins: &mut Vec<u8>) {
    pins.retain(|p| ALLOWED_PINS.contains(p));
    pins.sort();
    pins.dedup();
}

pub struct BioLoader {
    /// Raw 64-byte payloads indexed by chunk number.
    /// `None` means that chunk has not yet been received.
    chunks: Vec<Option<[u8; CHUNK_DATA_SIZE]>>,

    /// How many distinct chunk slots are currently filled.
    received_count: usize,

    /// BIO clock config
    target_freq: u32,

    /// Pins used by this BIO
    pin_spec: Vec<u8>,

    /// Actual code
    code: Option<[u8; BIO_MEM_BYTES]>,

    pddb: Pddb,

    bio_ss: Bio,
    _fifo_handle: CoreHandle,
    fifo: CoreCsr,
    resource_grant: ResourceGrant,

    core_init: bool,
    vault_conn: xous::CID,
}

impl BioLoader {
    pub fn new() -> Self {
        let pddb = Pddb::new();
        // this will block until the Vault calls mount
        pddb.is_mounted_blocking();

        #[cfg(feature = "factory-mismatch")]
        {
            use std::io::Write;

            use dc34_api::{BadgeType, DC34_BADGE, DC34_TOUR, FACTORY_ONE_WAY};

            let xns = xous_names::XousNames::new().unwrap();
            let keystore = keystore::Keystore::new(&xns);
            if keystore.get_owc(FACTORY_ONE_WAY).unwrap_or(1) == 0 {
                let pddb = pddb::Pddb::new();
                log::info!("Resetting state...");
                let mut key = pddb
                    .get(DC34_DICT, DC34_TOUR, None, true, true, Some(1), None::<fn()>)
                    .expect("couldn't get PDDB key");
                key.write(&[0]).ok();
                let mut badge_key = pddb
                    .get(DC34_DICT, DC34_BADGE, None, true, true, Some(1), None::<fn()>)
                    .expect("couldn't get PDDB key");
                badge_key.write(&[BadgeType::None as u8]).ok();
                pddb.sync().ok();
                log::info!("...done!");
                // safety: FACTORY_ONE_WAY is checked to be at the correct offset
                unsafe {
                    keystore.inc_owc(FACTORY_ONE_WAY).ok();
                }
                log::info!("Further factory ops disabled.");
            }
        }

        #[cfg(feature = "factory-wipe")]
        {
            use dc34_api::FACTORY_ONE_WAY;

            let xns = xous_names::XousNames::new().unwrap();
            let keystore = keystore::Keystore::new(&xns);
            if keystore.get_owc(FACTORY_ONE_WAY).unwrap_or(1) == 0 {
                let pddb = pddb::Pddb::new();
                pddb.delete_dict(DC34_DICT, None).ok();
                pddb.sync_cleanup().ok();
                log::info!("...done!");
                // safety: FACTORY_ONE_WAY is checked to be at the correct offset
                unsafe {
                    keystore.inc_owc(FACTORY_ONE_WAY).ok();
                }
                log::info!("Further factory ops disabled.");
            }
        }

        // this now happens fairly late in the init due to the gate above
        let mut bio_ss = Bio::new();
        // claim core resource and initialize it
        let resource_grant =
            bio_ss.claim_resources(&Self::resource_spec()).expect("Couldn't claim BIO resources");

        log::info!("BIO loader reserving core: {:?}", resource_grant.cores[0]);

        // safety: fifo is stored in this object so they aren't Drop'd before the object is
        // destroyed
        let fifo_handle = unsafe { bio_ss.get_core_handle(Fifo::Fifo3) }
            .expect("Didn't get FIFO3 handle")
            .expect("Didn't get FIFO3 handle");
        let fifo = CoreCsr::from_handle(&fifo_handle);

        bio_ss
            .setup_fifo_event_triggers(FifoEventConfig {
                which: Fifo::Fifo3,
                trigger_slot: TriggerSlot::new_with_raw_value(1),
                level: FifoLevel::new_with_raw_value(1),
                trigger_less_than: false,
                trigger_greater_than: true,
                trigger_equal_to: true,
            })
            .expect("couldn't set FIFO trigger configuration");
        bio_ss
            .setup_fifo_event_triggers(FifoEventConfig {
                which: Fifo::Fifo3,
                trigger_slot: TriggerSlot::new_with_raw_value(0),
                level: FifoLevel::new_with_raw_value(0),
                trigger_less_than: false,
                trigger_greater_than: false,
                trigger_equal_to: true,
            })
            .expect("couldn't set FIFO trigger configuration");

        let xns = xous_names::XousNames::new().unwrap();
        // this is used to indicate BIO activity on the UI
        let vault_conn = xns.request_connection_blocking("_Vault2_").unwrap();

        let mut loader = BioLoader {
            chunks: vec![None; NUM_CHUNKS],
            received_count: 0,
            target_freq: 350_000_000,
            pin_spec: vec![],
            code: None,
            pddb,
            bio_ss,
            _fifo_handle: fifo_handle,
            fifo,
            resource_grant,
            core_init: false,
            vault_conn,
        };
        match loader.reload() {
            Err(s) => {
                log::error!("Couldn't setup BIO: {:?}", s)
            }
            _ => (),
        }

        loader
    }

    pub fn reload(&mut self) -> Result<(), String> {
        let config = {
            let mut clk_buf = [0u8; 4];
            let mut key = self
                .pddb
                .get(DC34_DICT, DC34_BIO_CLK, None, true, true, Some(4), None::<fn()>)
                .map_err(|_| "couldn't get PDDB key".to_string())?;
            let attrs = key.attributes().map_err(|e| format!("couldn't get pins attrs: {:?}", e))?;
            let clk_len = if attrs.len > 0 {
                key.read(&mut clk_buf).map_err(|_| "couldn't read key".to_string())?
            } else {
                log::info!("No clock spec");
                0
            };
            self.target_freq = 350_000_000;
            if clk_len == 4 && clk_buf != [0u8; 4] {
                let target_freq = u32::from_le_bytes(clk_buf);
                if target_freq > 0 && target_freq <= 350_000_000 {
                    log::info!("target_freq {}", target_freq);
                    self.target_freq = target_freq;
                    CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqInt(target_freq) }
                } else {
                    CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqInt(350_000_000) }
                }
            } else {
                CoreConfig { clock_mode: bao1x_api::bio::ClockMode::TargetFreqInt(350_000_000) }
            }
        };
        // release any claimed pins
        let mut io_config = IoConfig::default();
        for pin in &self.pin_spec {
            self.bio_ss.release_dynamic_pin(*pin, &BioLoader::resource_spec().claimer).ok();
            io_config.mapped |= 1 << (*pin as u32);
        }
        if io_config.mapped != 0 {
            io_config.mode = IoConfigMode::ClearOnly;
            io_config.mapped = !io_config.mapped;
            self.bio_ss.setup_io_config(io_config).ok();
        }
        // now init a new set of pins
        let mut pin_spec = vec![0u8, 0u8, 0u8, 0u8];
        let mut key = self
            .pddb
            .get(DC34_DICT, DC34_BIO_PINS, None, true, true, Some(4), None::<fn()>)
            .map_err(|_| "couldn't get PDDB key".to_string())?;
        let attrs = key.attributes().map_err(|e| format!("couldn't get pins attrs: {:?}", e))?;
        if attrs.len > 0 {
            key.read(&mut pin_spec).map_err(|_| "couldn't read key".to_string())?;
            log::info!("Read pin spec {:?}", pin_spec);
        } else {
            log::info!("No pin spec");
        }

        let mut code_buf = [0u8; 0xf00];
        let mut key = self
            .pddb
            .get(DC34_DICT, DC34_BIO, None, true, true, Some(0xf00), None::<fn()>)
            .map_err(|_| "couldn't get PDDB key".to_string())?;
        let attrs = key.attributes().map_err(|e| format!("couldn't get code attrs: {:?}", e))?;
        // attrs.len check works around PDDB bug which seems to only trigger on this key
        // TODO: fix the PDDB bug.
        let code_len = if attrs.len > 0 {
            key.read(&mut code_buf).map_err(|_| "couldn't read key".to_string())?
        } else {
            0
        };
        let code_found;
        if code_len > 0 && code_buf.iter().any(|&b| b != 0) {
            log::info!("code: {:x?}", &code_buf[..16]);
            if self.core_init {
                self.bio_ss.de_init_core(self.resource_grant.cores[0]).ok();
                self.core_init = false;
            }
            let actual_freq = self
                .bio_ss
                .init_core(self.resource_grant.cores[0], (&code_buf, None), config)
                .map_err(|e| format!("Couldn't init core: {:?}", e))?;
            if let Some(actual) = actual_freq {
                log::info!("Actual quantum frequency: {}Hz", actual);
            }
            self.core_init = true;
            code_found = true;
        } else {
            log::info!("No code to load");
            code_found = false;
        }
        self.code = Some(code_buf);

        check_pins(&mut pin_spec);
        log::info!("Finalized pin spec {:?}", pin_spec);
        let mut io_config = IoConfig::default();
        for pin in &pin_spec {
            log::info!("Claiming pin {}", *pin);
            // claim pin resource - this only claims the resource, it does not configure it
            self.bio_ss
                .claim_dynamic_pin(*pin, &BioLoader::resource_spec().claimer)
                .map_err(|_| "can't claim pin".to_string())?;
            // now configure the claimed resource
            io_config.mapped |= 1 << (*pin as u32);
        }
        if io_config.mapped != 0 {
            io_config.mode = IoConfigMode::SetOnly;
            log::info!("Map pin mask {:x}", io_config.mapped);
            self.bio_ss.setup_io_config(io_config).ok();
        }
        self.pin_spec = pin_spec;

        if code_found {
            log::info!("Set core to run {:?}", self.resource_grant);
            self.bio_ss.set_core_run_state(&self.resource_grant, true);
            // indicate loading to the UI - activates "BIO ACTIVE" notice on the idle screen
            xous::send_message(self.vault_conn, xous::Message::new_scalar(1027, 1, 0, 0, 0)).ok();
        } else {
            log::info!("Set core to halt {:?}", self.resource_grant);
            self.bio_ss.set_core_run_state(&self.resource_grant, false);
        }
        Ok(())
    }

    /// Return true when every chunk slot is filled.
    pub fn is_complete(&self) -> bool { self.received_count == NUM_CHUNKS }

    /// Assemble all received chunks into a `[u8; 0xf00]` code chunk.
    /// Panics if `is_complete()` is false - caller should check first.
    pub fn to_code(&self) -> [u8; BIO_MEM_BYTES] {
        assert!(self.is_complete(), "code not yet complete");
        let mut code = [0u8; BIO_MEM_BYTES];
        for (chunk_idx, slot) in self.chunks.iter().enumerate() {
            let data = slot.as_ref().unwrap();
            let word_base = chunk_idx * CHUNK_DATA_SIZE;
            code[word_base..word_base + CHUNK_DATA_SIZE].copy_from_slice(data);
        }
        code
    }

    /// Reset all state in the holding buffer (not to be confused with clearing the PDDB stored data)
    pub fn clear(&mut self) {
        for slot in self.chunks.iter_mut() {
            *slot = None;
        }
        self.received_count = 0;
    }

    /// Persist the assembled code to PDDB, clear in-memory state, and signal
    /// a reload. Called both from the normal last-chunk path and from `pad`.
    fn commit(&mut self) -> &'static str {
        {
            let mut code_key = self
                .pddb
                .get(DC34_DICT, DC34_BIO, None, true, true, Some(0xf00), None::<fn()>)
                .expect("couldn't get PDDB key");
            let bytes = self.to_code();
            code_key.write_all(&bytes).ok();
        }
        self.clear(); // clears the holding buffer for incoming code
        self.pddb.sync().ok();
        "SUCCESS"
    }
}

impl Resources for BioLoader {
    fn resource_spec() -> ResourceSpec {
        ResourceSpec {
            claimer: "BIO loader".to_string(),
            cores: vec![CoreRequirement::Any],
            fifos: vec![Fifo::Fifo3],
            static_pins: vec![],
            dynamic_pin_count: 4,
        }
    }
}

impl Drop for BioLoader {
    fn drop(&mut self) {
        for &core in self.resource_grant.cores.iter() {
            self.bio_ss.de_init_core(core).unwrap();
        }
        self.core_init = false;
        let mut io_config = IoConfig::default();
        for pin in &self.pin_spec {
            self.bio_ss.release_dynamic_pin(*pin, &BioLoader::resource_spec().claimer).ok();
            io_config.mapped |= 1 << (*pin as u32);
        }
        if io_config.mapped != 0 {
            io_config.mode = IoConfigMode::ClearOnly;
            io_config.mapped = !io_config.mapped;
            self.bio_ss.setup_io_config(io_config).unwrap();
        }
        self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();
    }
}

fn clear_key(pddb: &Pddb, name: &str, len: usize) {
    // There's a PDDB bug that makes the PDDB unstable with deleting and then remaking
    // keys over and over again. This is a work-around that just writes an all-0 value
    // over the existing key.
    let mut key =
        pddb.get(DC34_DICT, name, None, true, true, Some(len), None::<fn()>).expect("couldn't get PDDB key");
    key.write_all(&vec![0u8; len]).unwrap();
}
// -- ShellCmdApi implementation ------------------------------------------------

impl<'a> ShellCmdApi<'a> for BioLoader {
    cmd_api!(bio);

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();

        if args == "ready" {
            write!(ret, "OK").unwrap();
            return Ok(Some(ret));
        }

        if args == "clear" {
            self.clear();
            clear_key(&self.pddb, DC34_BIO, 0xf00);
            clear_key(&self.pddb, DC34_BIO_CLK, 4);
            clear_key(&self.pddb, DC34_BIO_PINS, 4);
            self.pddb.sync().ok();

            for &core in self.resource_grant.cores.iter() {
                self.bio_ss.de_init_core(core).unwrap();
            }
            self.core_init = false;
            let mut io_config = IoConfig::default();
            for pin in &self.pin_spec {
                self.bio_ss.release_dynamic_pin(*pin, &BioLoader::resource_spec().claimer).ok();
                io_config.mapped |= 1 << (*pin as u32);
            }
            if io_config.mapped != 0 {
                io_config.mode = IoConfigMode::ClearOnly;
                io_config.mapped = !io_config.mapped;
                self.bio_ss.setup_io_config(io_config).unwrap();
            }
            self.pin_spec = vec![];
            // don't release this - we still "own" it just in case we get new code uploaded
            // self.bio_ss.release_resources(self.resource_grant.grant_id).unwrap();

            write!(ret, "CLEAR").unwrap();
            // monkey patch to reset the light pattern and remove the "BIO ACTIVE" notice
            let conn = _env.xns.request_connection_blocking("_Vault2_").unwrap();
            xous::send_message(conn, xous::Message::new_scalar(1027, 0, 0, 0, 0)).ok();
            return Ok(Some(ret));
        }

        if args.starts_with("pin") {
            let arg_list: Vec<&str> = args.split_whitespace().collect();
            let mut pins = Vec::<u8>::new();
            if arg_list.len() >= 2 {
                for pin_str in &arg_list[1..] {
                    if let Ok(pin) = u8::from_str_radix(pin_str, 10) {
                        pins.push(pin);
                    }
                }
                check_pins(&mut pins);
                {
                    let mut pin_list = [0u8; 4];
                    for (i, p) in pins.iter().enumerate() {
                        pin_list[i] = *p;
                    }
                    let mut pin_key = self
                        .pddb
                        .get(DC34_DICT, DC34_BIO_PINS, None, true, true, Some(4), None::<fn()>)
                        .expect("couldn't get PDDB key");
                    pin_key.write_all(&pin_list).ok();
                    log::info!("Set pin spec of {:x?}", pin_list);
                    self.pddb.sync().ok();
                }
            }
            write!(ret, "OK").unwrap();
            return Ok(Some(ret));
        }

        if args.starts_with("clk") {
            let arg_list: Vec<&str> = args.split_whitespace().collect();
            if arg_list.len() >= 2 {
                let mut clk_key = self
                    .pddb
                    .get(DC34_DICT, DC34_BIO_CLK, None, true, true, Some(4), None::<fn()>)
                    .expect("couldn't get PDDB key");
                clk_key.write_all(&[0, 0, 0, 0]).ok();
                if let Ok(clk) = u32::from_str_radix(arg_list[1], 10) {
                    let mut clk_key = self
                        .pddb
                        .get(DC34_DICT, DC34_BIO_CLK, None, true, true, Some(4), None::<fn()>)
                        .expect("couldn't get PDDB key");
                    clk_key.write_all(&clk.to_le_bytes()).ok();
                }
            }
            write!(ret, "OK").unwrap();
            return Ok(Some(ret));
        }

        if args.starts_with("reload") {
            match self.reload() {
                Ok(_) => {
                    write!(ret, "BIO load successful").unwrap();
                    return Ok(Some(ret));
                }
                Err(e) => {
                    write!(ret, "ERR {:?}", e).unwrap();
                    return Ok(Some(ret));
                }
            }
        }

        if args.starts_with("rx") {
            self.bio_ss.debug(self.resource_grant.cores[0]);
            let arg_list: Vec<&str> = args.split_whitespace().collect();
            let iters =
                if arg_list.len() >= 2 { usize::from_str_radix(arg_list[1], 10).unwrap_or(1) } else { 1 };
            let timeout = if arg_list.len() >= 3 {
                Duration::from_secs(u64::from_str_radix(arg_list[2], 10).unwrap_or(1))
            } else {
                Duration::from_secs(1)
            };
            for _ in 0..iters {
                let start = std::time::Instant::now();
                while self.fifo.csr.rf(utralib::utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3) == 0 {
                    // wait for entry
                    if std::time::Instant::now().duration_since(start) > timeout {
                        log::info!("timeout");
                        break;
                    }
                }
                let dat = self.fifo.csr.r(utralib::utra::bio_bdma::SFR_RXF3);
                log::info!("{:x?}", dat);
            }
            write!(ret, "OK").unwrap();
            return Ok(Some(ret));
        }

        if args.starts_with("tx") {
            let arg_list: Vec<&str> = args.split_whitespace().collect();
            if arg_list.len() >= 2 {
                let s = arg_list[1].trim();
                let val = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                    u32::from_str_radix(hex, 16).map_err(|e| e.to_string())
                } else {
                    s.parse::<u32>().map_err(|e| e.to_string())
                };
                let iters =
                    if arg_list.len() == 3 { usize::from_str_radix(arg_list[2], 10).unwrap_or(1) } else { 1 };
                let mut timeout = false;
                if let Ok(val) = val {
                    for _ in 0..iters {
                        let start = std::time::Instant::now();
                        while self.fifo.csr.rf(utralib::utra::bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3) > 7 {
                            // don't overflow the fifo
                            // wait for entry
                            if std::time::Instant::now().duration_since(start) > Duration::from_secs(5) {
                                log::info!("timeout");
                                timeout = true;
                                break;
                            }
                        }
                        if timeout {
                            break;
                        }
                        self.fifo.csr.wo(utralib::utra::bio_bdma::SFR_TXF3, val);
                        log::info!("{:x?}", val);
                    }
                    write!(ret, "OK").unwrap();
                } else {
                    write!(ret, "ERR bad argument").unwrap();
                }
            } else {
                write!(ret, "ERR tx <val> [repeat]").unwrap();
            }
            return Ok(Some(ret));
        }

        if args == "pad" {
            // Zero-fill every slot not yet received, then commit.
            // This allows the host to send only the chunks that carry real data
            // and let the device pad the remainder of the 0xf00-byte buffer.

            for slot in self.chunks.iter_mut() {
                if slot.is_none() {
                    *slot = Some([0u8; CHUNK_DATA_SIZE]);

                    self.received_count += 1;
                }
            }

            write!(ret, "{}", self.commit()).unwrap();

            return Ok(Some(ret));
        }

        // -- Decode base64 argument --------------------------------------------
        let b64 = args.trim();
        if b64.is_empty() {
            write!(ret, "ERR").unwrap();
            return Ok(Some(ret));
        }

        let decoded = match B64.decode(b64) {
            Ok(d) => d,
            Err(_) => {
                write!(ret, "ERR").unwrap();
                return Ok(Some(ret));
            }
        };

        if decoded.len() != CHUNK_WIRE_SIZE {
            write!(ret, "ERR").unwrap();
            return Ok(Some(ret));
        }

        // -- Parse fields ------------------------------------------------------
        let index = u16::from_be_bytes([decoded[0], decoded[1]]) as usize;
        // data lives at decoded[2..66]
        let received_crc = u32::from_be_bytes([decoded[66], decoded[67], decoded[68], decoded[69]]);

        // -- Verify CRC over (index bytes || data) -----------------------------
        let computed_crc = crc32fast::hash(&decoded[..CHUNK_INDEX_BYTES + CHUNK_DATA_SIZE]);
        if computed_crc != received_crc {
            write!(ret, "ERR").unwrap();
            return Ok(Some(ret));
        }

        // -- Bounds-check index ------------------------------------------------
        if index >= NUM_CHUNKS {
            write!(ret, "ERR").unwrap();
            return Ok(Some(ret));
        }

        // -- Store chunk (silent overwrite on duplicate) -----------------------
        let mut data_arr = [0u8; CHUNK_DATA_SIZE];
        data_arr.copy_from_slice(&decoded[CHUNK_INDEX_BYTES..CHUNK_INDEX_BYTES + CHUNK_DATA_SIZE]);

        let was_empty = self.chunks[index].is_none();
        self.chunks[index] = Some(data_arr);
        if was_empty {
            self.received_count += 1;
        }

        // -- Reply -------------------------------------------------------------
        if self.is_complete() {
            write!(ret, "{}", self.commit()).unwrap();
        } else {
            write!(ret, "OK").unwrap();
        }

        Ok(Some(ret))
    }
}
