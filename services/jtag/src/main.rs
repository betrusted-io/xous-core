#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
mod efuse_api;
mod efuse_ecc;

use api::*;
use efuse_api::*;
use num_traits::*;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack};
use xous_ipc::Buffer;

use crate::implementation::JtagPhy;

pub enum JtagState {
    TestReset,
    RunIdle,
    Select,
    Capture,
    Shift,
    Exit1,
    Pause,
    Exit2,
    Update,
}

#[derive(Copy, Clone)]
pub enum JtagChain {
    DR,
    IR,
}

pub enum JtagEndian {
    Big,    // MSB-first shiftout
    Little, // LSB-first shiftout
}

/// option 1: make a "leg" machine that contains the shift-in/shift-out records specific to each leg
/// option 2: make a comprehensive machine that receives meta-commands to transition between states
///
/// I think we want a machine that has a Vector which holds a set of instructions that encapsulate either
/// data to send into the IR or DR. There should be a state bit that indicates if the data has been
/// executed; after execution, there is a result vector that is now valid.
#[derive(Clone)]
pub struct JtagLeg {
    /// which chain (DR or IR)
    c: JtagChain,
    /// output bit vector to device; chain length is defined by vector length
    o: Vec<bool>,
    /// input bit vector from device; length is dynamically allocated as leg traverses
    i: Vec<bool>,
    /// a tag for the leg, to be used by higher level logic to track pending/done entries
    tag: String,
}

impl JtagLeg {
    pub fn new(chain_type: JtagChain, mytag: &str) -> Self {
        JtagLeg { c: chain_type, o: Vec::new(), i: Vec::new(), tag: String::from(mytag) }
    }

    /// `push` will take data in the form of an unsigned int (either u128 or u32)
    /// and append it to the JTAG input vector in preparation for sending.
    /// "count" specifies the number of bits of the vector that are valid, and
    /// "endian" specifies if the MSB or LSB first should be pushed into the JTAG
    /// chain.
    ///
    /// In the case that "count" is less than the full data length and MSB first
    /// order is requested, `push` first discards the left-most unused bits and
    /// then starts push from the remaining MSB. e.g., to push the number
    /// `101100` into the JTAG chain MSB first, store 0x2C into "data" and specify
    /// a "count" of 6, and an "endian" of JtagEndian::Big. Do not shift
    /// data all the way to the MSB of the containing "data" parameter in this case!
    pub fn push_u128(&mut self, data: u128, count: usize, endian: JtagEndian) {
        assert!(count <= 128);
        for i in 0..count {
            match endian {
                JtagEndian::Big => {
                    if (data & (1 << i)) == 0 {
                        self.i.push(false)
                    } else {
                        self.i.push(true)
                    }
                }
                JtagEndian::Little => {
                    if (data & (1 << (count - 1 - i))) == 0 {
                        self.i.push(false)
                    } else {
                        self.i.push(true)
                    }
                }
            }
        }
    }

    pub fn push_u32(&mut self, data: u32, count: usize, endian: JtagEndian) {
        assert!(count <= 32);
        for i in 0..count {
            match endian {
                JtagEndian::Big => {
                    if (data & (1 << i)) == 0 {
                        self.i.push(false)
                    } else {
                        self.i.push(true)
                    }
                }
                JtagEndian::Little => {
                    if (data & (1 << (count - 1 - i))) == 0 {
                        self.i.push(false)
                    } else {
                        self.i.push(true)
                    }
                }
            }
        }
    }

    pub fn push_u8(&mut self, data: u8, count: usize, endian: JtagEndian) {
        assert!(count <= 8);
        for i in 0..count {
            match endian {
                JtagEndian::Big => {
                    if (data & (1 << i)) == 0 {
                        self.i.push(false)
                    } else {
                        self.i.push(true)
                    }
                }
                JtagEndian::Little => {
                    if (data & (1 << (count - 1 - i))) == 0 {
                        self.i.push(false)
                    } else {
                        self.i.push(true)
                    }
                }
            }
        }
    }

    pub fn pop_u32(&mut self, count: usize, endian: JtagEndian) -> Option<u32> {
        if self.o.len() < count {
            // error out before trying to touch the vector, so that in case
            // of a parameter error we can try again without having lost our data
            // in general, "count" should be very well specified in this protocol.
            return None;
        }

        let mut data: u32 = 0;
        for _ in 0..count {
            match endian {
                JtagEndian::Little => {
                    data <<= 1;
                    if self.o.pop().unwrap() {
                        data |= 0x1;
                    }
                }
                JtagEndian::Big => {
                    data >>= 1;
                    if self.o.pop().unwrap() {
                        data |= 0x8000_0000;
                    }
                }
            }
        }

        Some(data)
    }

    /// pop_u128 does a "Best effort" to return up to count_req elements, will return what is
    /// available if less is available
    pub fn pop_u128(&mut self, count_req: usize, endian: JtagEndian) -> Option<u128> {
        let mut count: usize = count_req;
        if self.o.len() == 0 {
            return None;
        } else if self.o.len() < count_req {
            count = self.o.len();
        }

        let mut data: u128 = 0;
        for _ in 0..count {
            match endian {
                JtagEndian::Little => {
                    data <<= 1;
                    if self.o.pop().unwrap() {
                        data |= 0x1;
                    }
                }
                JtagEndian::Big => {
                    data >>= 1;
                    if self.o.pop().unwrap() {
                        data |= 0x8000_0000_0000_0000_0000_0000_0000_0000;
                    }
                }
            }
        }

        Some(data)
    }

    pub fn pop_u8(&mut self, count: usize, endian: JtagEndian) -> Option<u8> {
        if self.o.len() < count {
            // error out before trying to touch the vector, so that in case
            // of a parameter error we can try again without having lost our data
            // in general, "count" should be very well specified in this protocol.
            return None;
        }

        let mut data: u8 = 0;
        for _ in 0..count {
            match endian {
                JtagEndian::Little => {
                    data <<= 1;
                    if self.o.pop().unwrap() {
                        data |= 0x1;
                    }
                }
                JtagEndian::Big => {
                    data >>= 1;
                    if self.o.pop().unwrap() {
                        data |= 0x80;
                    }
                }
            }
        }

        Some(data)
    }

    pub fn tag(&self) -> String { self.tag.clone() }

    pub fn dbg_i_len(&self) -> usize { self.i.len() }

    pub fn dbg_o_len(&self) -> usize { self.o.len() }
}

pub struct JtagMach {
    /// current state (could be in one of two generics, or in DR/IR chain; check top of Vector for current
    /// chain)
    s: JtagState,
    /// a vector of legs to traverse. An entry stays in pending until the traversal is complete. Aborted
    /// traversals leave the leg in place
    pending: Vec<JtagLeg>,
    /// a vector of legs traversed. An entry is only put into the done vector once its traversal is
    /// completed.
    done: Vec<JtagLeg>,
    /// the current leg being processed
    current: Option<JtagLeg>,
    /// an integer for debug help
    debug: u32,
    /// the jtag phy
    phy: JtagPhy,
    ticktimer: ticktimer_server::Ticktimer,
}

impl JtagMach {
    pub fn new() -> Self {
        JtagMach {
            s: JtagState::TestReset,
            pending: Vec::new(),
            done: Vec::new(),
            current: None,
            debug: 0,
            phy: implementation::JtagPhy::new(),
            ticktimer: ticktimer_server::Ticktimer::new().unwrap(),
        }
    }

    /// pause for a given number of microseconds.
    pub fn pause(&mut self, us: u32) {
        let mut delay: u32 = us / 1000;
        if delay == 0 {
            delay = 1;
        }
        self.ticktimer.sleep_ms(delay as usize).expect("couldn't sleep");
    }

    /// add() -- add a leg to the pending queue
    pub fn add(&mut self, leg: JtagLeg) { self.pending.push(leg); }

    /// get() -- get the oldest result in the done queue. Returns an option.
    pub fn get(&mut self) -> Option<JtagLeg> {
        if self.done.len() > 0 { Some(self.done.remove(0)) } else { None }
    }

    /// has_pending() -- tells if the jtag machine has a pending leg to traverse. Returns the tag of the
    /// pending item, or None.
    pub fn has_pending(&self) -> bool { if self.pending.len() > 0 { true } else { false } }

    /// has_done() -- tells if the jtag machine has any legs that are done to read out. Returns the tag of the
    /// done item, or None.
    #[allow(dead_code)]
    pub fn has_done(&self) -> bool { if self.done.len() > 0 { true } else { false } }

    /// for debug
    #[allow(dead_code)]
    pub fn pending_len(&self) -> usize { self.pending.len() }

    /// for debug
    #[allow(dead_code)]
    pub fn done_len(&self) -> usize { self.done.len() }

    pub fn dbg_reset(&mut self) { self.debug = 0; }

    pub fn dbg_get(&self) -> u32 { self.debug }

    /// step() -- move state machine by one cycle
    /// if there is nothing in the pending queue, stay in idle
    /// if something in the pending queue, traverse to execute it
    pub fn step(&mut self) {
        self.s = match self.s {
            JtagState::TestReset => {
                self.phy.sync(false, false);
                JtagState::RunIdle
            }
            JtagState::RunIdle => {
                // we have a current item, traverse to the correct tree based on the type
                if let Some(ref mut cur) = self.current {
                    match cur.c {
                        JtagChain::DR => {
                            self.debug = 2;
                            self.phy.sync(false, true);
                        }
                        JtagChain::IR => {
                            self.debug = 3;
                            // must be IR -- do two TMS high pulses to get to the IR leg
                            self.phy.sync(false, true);
                            self.phy.sync(false, true);
                        }
                    }
                    JtagState::Select
                } else {
                    if self.pending.len() > 0 {
                        // nothing current, but has pending --> assign a current
                        // don't pop the entry, though, until we are finished traversing the leg,
                        // hence we make a clone of the entry
                        self.current = Some(self.pending[0].clone());
                    } else {
                        // nothing pending, nothing current
                        // stay in the current state
                        self.phy.sync(false, false);
                    }
                    JtagState::RunIdle
                }
            }
            JtagState::Select => {
                self.phy.sync(false, false);
                JtagState::Capture
            }
            JtagState::Capture => {
                // always move to shift, because leg structures always have data
                self.phy.sync(false, false);
                JtagState::Shift
            }
            JtagState::Shift => {
                // shift data until the input vector is exhausted
                if let Some(ref mut cur) = self.current {
                    if let Some(tdi) = cur.i.pop() {
                        if cur.i.len() > 0 {
                            let tdo: bool = self.phy.sync(tdi, false);
                            cur.o.push(tdo);
                            self.current = Some(cur.clone());
                            JtagState::Shift
                        } else {
                            // last element should leave the state
                            let tdo: bool = self.phy.sync(tdi, true);
                            cur.o.push(tdo);
                            self.current = Some(cur.clone());
                            JtagState::Exit1
                        }
                    } else {
                        // Shouldn't happen: no "i", but move on gracefully
                        JtagState::Exit1
                    }
                } else {
                    // Shouldn't happen: No "Current", but move on gracefully
                    JtagState::Exit1
                }
            }
            JtagState::Exit1 => {
                self.phy.sync(false, true);
                JtagState::Update
            }
            JtagState::Pause => {
                self.phy.sync(false, true);
                JtagState::Exit2
            }
            JtagState::Exit2 => {
                self.phy.sync(false, true);
                JtagState::Update
            }
            JtagState::Update => {
                self.phy.sync(false, false);

                self.pending.remove(0); // remove the oldest entry
                if let Some(next) = self.current.take() {
                    self.done.push(next);
                }
                JtagState::RunIdle
            }
        }
    }

    /// reset() -- bring the state machine back to the TEST_RESET state
    pub fn reset(&mut self) {
        // regardless of what state we are in, 5 cycles of TMS=1 will bring us to RESET
        for _ in 0..5 {
            self.phy.sync(false, true);
        }
        self.s = JtagState::TestReset;
    }

    /// next() -- advance until a RUN_IDLE state. If currently RUN_IDLE, traverse the next available leg, if
    /// one exists
    pub fn next(&mut self) {
        match self.s {
            JtagState::RunIdle | JtagState::TestReset => {
                if self.has_pending() {
                    // if pending, step until we're into a leg
                    loop {
                        match self.s {
                            JtagState::RunIdle | JtagState::TestReset => self.step(),
                            _ => break,
                        }
                    }
                    // then step until we're out of the leg
                    loop {
                        match self.s {
                            JtagState::RunIdle | JtagState::TestReset => break,
                            _ => self.step(),
                        }
                    }
                } else {
                    self.step(); // this should be a single step with no state change
                }
            }
            _ => {
                // in the case that we're not already in idle or reset, run the machine until we get to idle
                // or reset
                loop {
                    match self.s {
                        JtagState::RunIdle | JtagState::TestReset => break,
                        _ => self.step(),
                    }
                }
            }
        }
    }
}

#[cfg(any(feature = "precursor", feature = "renode"))]
mod implementation {
    use utralib::generated::*;

    #[allow(dead_code)]
    pub(crate) struct JtagPhy {
        csr: utralib::CSR<u32>,
    }

    impl JtagPhy {
        pub fn new() -> JtagPhy {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::jtag::HW_JTAG_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map JTAG CSR range");

            let jtag = JtagPhy { csr: CSR::new(csr.as_mut_ptr() as *mut u32) };

            jtag
        }

        /// given a tdi and tms value, pulse the clock, and then return the tdo that comes out
        pub fn sync(&mut self, tdi: bool, tms: bool) -> bool {
            self.csr.wo(
                utra::jtag::NEXT,
                self.csr.ms(utra::jtag::NEXT_TDI, if tdi { 1 } else { 0 })
                    | self.csr.ms(utra::jtag::NEXT_TMS, if tms { 1 } else { 0 }),
            );

            while self.csr.rf(utra::jtag::TDO_READY) == 0 {} // make sure we are in a ready/tdo valid state
            if self.csr.rf(utra::jtag::TDO_TDO) == 0 {
                // this is the TDO value from /prior/ to the TCK rise
                false
            } else {
                true
            }
            // note: the hardware already guarantees TDO sample timing relative to TCK edge: in other words,
            // TDO is sampled before the TCK edge is allowed to rise
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "xous"))]
mod implementation {
    pub struct JtagPhy {}

    impl JtagPhy {
        pub fn new() -> JtagPhy { JtagPhy {} }

        pub fn sync(&mut self, _tdi: bool, _tms: bool) -> bool { false }
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // expect:
    //   - one connection from the key server
    //   - one connection from shellchat for command line
    //   - another connection from shellchat for oqc testing
    #[cfg(all(any(feature = "precursor", feature = "renode"), not(feature = "dvt")))]
    let jtag_sid = xns.register_name(api::SERVER_NAME_JTAG, Some(3)).expect("can't register server");
    #[cfg(all(any(feature = "precursor", feature = "renode"), feature = "dvt"))] // dvt build has less in it
    let jtag_sid = xns.register_name(api::SERVER_NAME_JTAG, Some(2)).expect("can't register server");
    #[cfg(not(target_os = "xous"))]
    let jtag_sid = xns.register_name(api::SERVER_NAME_JTAG, Some(2)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", jtag_sid);

    let mut jtag = JtagMach::new();
    let mut efuse = EfuseApi::new();

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let mut susres = susres::Susres::new_without_hook(&xns).expect("couldn't create suspend/resume object");

    loop {
        let mut msg = xous::receive_message(jtag_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::GetId) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                jtag.reset();
                let mut id_leg: JtagLeg = JtagLeg::new(JtagChain::IR, "idcode");
                id_leg.push_u32(0b001001, 6, JtagEndian::Little);
                jtag.add(id_leg);
                jtag.next();
                // NOW: - check the return data on .get() before using it
                if jtag.get().is_none() {
                    // discard ID code but check that there's something
                    log::error!("ID instruction not in get queue!");
                    xous::return_scalar(msg.sender, 0xFFFF_FFFF).unwrap();
                    continue;
                }

                let mut data_leg: JtagLeg = JtagLeg::new(JtagChain::DR, "iddata");
                data_leg.push_u32(0, 32, JtagEndian::Little);
                jtag.add(data_leg);
                jtag.dbg_reset();
                jtag.next();
                let d: u32 = jtag.dbg_get();
                if let Some(mut iddata) = jtag.get() {
                    // this contains the actual idcode data
                    let id = iddata.pop_u32(32, JtagEndian::Little).unwrap();
                    log::trace!("tag: {}, code: 0x{:08x}, d:{}", iddata.tag(), id, d);
                    xous::return_scalar(msg.sender, id as usize).unwrap();
                } else {
                    log::trace!("ID data not in get queue!");
                }
            }),
            Some(Opcode::GetDna) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                jtag.reset();
                let mut ir_leg: JtagLeg = JtagLeg::new(JtagChain::IR, "cmd");
                ir_leg.push_u32(0b110010, 6, JtagEndian::Little);
                jtag.add(ir_leg);
                jtag.next();
                if jtag.get().is_none() {
                    // discard ID code but check that there's something
                    log::error!("cmd instruction not in get queue!");
                    xous::return_scalar2(msg.sender, 0xFFFF_FFFF, 0xFFFF).unwrap();
                }

                let mut data_leg: JtagLeg = JtagLeg::new(JtagChain::DR, "dna");
                data_leg.push_u128(0, 64, JtagEndian::Little);
                jtag.add(data_leg);
                jtag.next();
                if let Some(mut data) = jtag.get() {
                    let dna: u128 = data.pop_u128(64, JtagEndian::Little).unwrap();
                    xous::return_scalar2(msg.sender, (dna >> 32) as usize, dna as usize).unwrap();
                    log::info!("{}/0x{:16x}", data.tag(), dna);
                } else {
                    log::error!("cmd instruction not in get queue!");
                    xous::return_scalar2(msg.sender, 0xFFFF_FFFF, 0xFFFF).unwrap();
                }
            }),
            Some(Opcode::EfuseFetch) => {
                efuse.fetch(&mut jtag);
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut efuse_rec = buffer.to_original::<EfuseRecord, _>().unwrap();

                efuse_rec.user = efuse.phy_user();
                efuse_rec.cntl = efuse.phy_cntl();
                efuse_rec.key.copy_from_slice(&efuse.phy_key());
                buffer.replace(efuse_rec).unwrap();
            }
            Some(Opcode::WriteIr) => msg_scalar_unpack!(msg, ir, _, _, _, {
                jtag.reset();
                let mut ir_leg: JtagLeg = JtagLeg::new(JtagChain::IR, "cmd");
                ir_leg.push_u32(ir as u32, 6, JtagEndian::Little); // ISC_ENABLE
                jtag.add(ir_leg);
                jtag.next();
                if jtag.get().is_none() {
                    // discard ID code but check that there's something
                    log::error!("cmd instruction not in get queue!");
                }
            }),
            Some(Opcode::EfuseKeyBurn) => {
                susres.set_suspendable(false).unwrap();
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let key = buffer.to_original::<[u8; 32], _>().unwrap();
                // refresh the efuse cache
                efuse.fetch(&mut jtag);
                #[cfg(feature = "hazardous-debug")]
                log::info!("attempting to burn key: {:x?}", key); // this is easier for me to read as a human
                #[cfg(feature = "hazardous-debug")]
                log::info!("attempting to burn key: {:?}", key); // this is easier for import to python
                efuse.set_key(key);
                if efuse.is_valid() {
                    log::info!("efuse key is valid to burn, proceeding. There is no return...");
                    if cfg!(feature = "dry-run") {
                        log::info!(
                            "Dry run selected, key NOT BURNED. Device will be in an inconsistent state."
                        );
                    } else {
                        efuse.burn(&mut jtag);
                    }
                    buffer.replace(EfuseResult::Success).unwrap();
                } else {
                    log::error!("efuses already burned, new key is unpatchable. Refusing to burn!");
                    buffer.replace(EfuseResult::Failure).unwrap();
                }
                susres.set_suspendable(true).unwrap();
            }
            Some(Opcode::EfuseCtlBurn) => msg_blocking_scalar_unpack!(msg, ctl, _, _, _, {
                susres.set_suspendable(false).unwrap();
                efuse.fetch(&mut jtag);
                efuse.set_cntl(ctl as u8);
                if efuse.is_valid() {
                    log::info!("control is valid to burn, proceeding. There is no return...");
                    if cfg!(feature = "dry-run") {
                        log::info!(
                            "Dry run selected, control fuses NOT BURNED. Device will be in an inconsistent state."
                        );
                    } else {
                        efuse.burn(&mut jtag);
                    }
                    susres.set_suspendable(true).unwrap();
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    susres.set_suspendable(true).unwrap();
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::EfuseUserBurn) => msg_blocking_scalar_unpack!(msg, user, _, _, _, {
                susres.set_suspendable(false).unwrap();
                efuse.fetch(&mut jtag);
                efuse.set_user(user as u32);
                if efuse.is_valid() {
                    log::info!("user fuses are valid to burn, proceeding. There is no return...");
                    if cfg!(feature = "dry-run") {
                        log::info!(
                            "Dry run selected, user fuse NOT BURNED. Device will be in an inconsistent state."
                        );
                    } else {
                        efuse.burn(&mut jtag);
                    }
                    susres.set_suspendable(true).unwrap();
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    susres.set_suspendable(true).unwrap();
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            None => {
                log::error!("couldn't convert opcode");
                break;
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(jtag_sid).unwrap();
    xous::destroy_server(jtag_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
