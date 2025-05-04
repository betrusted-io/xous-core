use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use std::cmp::Reverse;
#[cfg(feature = "deterministic")]
use std::collections::BTreeSet;
use std::collections::BinaryHeap;
use std::collections::HashMap;
#[cfg(any(all(feature = "pddbtest", feature = "autobasis"), not(feature = "deterministic")))]
use std::collections::HashSet;
use std::convert::TryInto;
use std::io::{Error, ErrorKind, Result};

use aes::cipher::{BlockDecrypt, BlockEncrypt, generic_array::GenericArray};
use aes::{Aes256, BLOCK_SIZE, Block};
use aes_gcm_siv::aead::{Aead, Payload};
use aes_gcm_siv::{Aes256GcmSiv, Nonce, Tag};
use backend::bcrypt::*;
#[cfg(not(any(feature = "hosted-baosec", feature = "board-baosec")))]
use gam::modal::*;
use modals::Modals;
#[cfg(feature = "gen1")]
use root_keys::api::AesRootkeyType;
#[cfg(feature = "gen1")]
use root_keys::api::KeywrapError;
use sha2::{Digest, Sha512_256Hw, Sha512_256Sw};
use spinor::SPINOR_BULK_ERASE_SIZE;
use subtle::ConstantTimeEq;
#[cfg(any(feature = "hosted-baosec", feature = "board-baosec"))]
use ux_api::widgets::*;

use crate::*;

/*
    Refactor notes --

    What to do about rootkeys? The features used by this crate are:
      - selection of an AES key at an index
      - password entry to unlock key at index (this is handled inside rootkeys)
      - key wrapping using the indexed key
      - block decrypt/encrypt with the indexed key
      - also queries are made with regards to the key box health and initialization status

    What to do about modals?
      - The work flow for many of the crates assumes we can have a blocking user I/O
        via the modals crate
      - I think that the modals crate should probably be the "right" layer for things
        to talk to the UI.
    Question:
      - Can we adapt modals to a small (128x128) pixel display?
      - Can we adapt modals to a console-like text interface?

    Suggestion:
      - Stop the refactor on PDDB at this moment and dig into modals/rootkeys
        - Rootkeys can place the keys in RRAM at the "final" location according to ACRAM lock abilities
          even if the lifecycle stuff is broken, the ability to read/write those keys are configured
          "out of band"
        - Modals should include an implementation directly inside the modals crate that pulls in
          the mini-gfx library, so that there are no other dependencies to get user I/O going.

    Observation: I could check in this partial, broken code right now into a branch, and come back
    to it later. As long as I don't select the PDDB crate as a cramium target, it doesn't break
    anything else in the code base?

    Probably better just to branch this for now - let's make a branch, stick it in xous-core,
    and put some notes around it in a WIP pull request. In fact let's put all this data in that
    PR so we have some publicly trackable information about how this is progressing.

    Xous-swapper refactor notes:

    SPINOR shares the same interface as the swap SRAM, and therefore, the Spinor interface
    has to live inside xous-swapper.

    Xous also needs to be extended to implement Virtual Addresses that are not backed by
    physical addresses, but are on-demand allocated by copying SPINOR contents into pages that
    are allocated in SRAM. The general idea is something like this:
    - A new flag is added that can be passed to xous::MapMemory(). This flag is the `Swapped` flag,
        and it indicates to the kernel that the physical address should be `null` and marked as `swapped`
        for the mapping in its initial page table map in the given process.
    - A "special" range of virtual addresses needs to be carved out which is where the memory-mapped
        FLASH memory would go to. Thus, a process would gain access to FLASH by simply calling MapMemory
        with the FLASH memory VA range as the virtual address, `None` as the PA, and the Swapped flag
        as one of the args in Memory Arguments
    - When the virtual address is referenced, it will page fault; the page fault handler will pass
        this to Xous-Swapper in the userspace, which will then add a check for the virtual address.
        If the virtual address is in the magic range for SPINOR, the contents are fetched using the
        SPINOR interface based on the linear mapping of the lower bits of the address onto the SPINOR
        memory space.
    - WHnever a write happens inside Xous-Swapper to a location in FLASH, the page table entry for
        the mapped FLASH location needs to be marked as invalid and returned to the free pool. This
        uses the swap system to keep the read-view of FLASH in sync with hte write-view. This also
        means that Xous-Swapper's SPINOR map has to keep a scoreboard of what pages are mapped to
        what processes, so that we can search for the mapping and clear it whenever a write comes in.
    - Development of this can probably happen separately from the emu-layer version. The basic thing
        would be to drill down into the kernel with the kernel and a simple test server that just
        tries to map the SPINOR and read some contents out, and perhaps update a sector. This will
        exercise the path in isolation and allow us to test this routine as a separate primitive
        from the PDDB.
*/

#[cfg(not(feature = "deterministic"))]
type FspaceSet = HashSet<PhysPage>;
#[cfg(feature = "deterministic")]
type FspaceSet = BTreeSet<PhysPage>;

#[cfg(feature = "perfcounter")]
use perflib::*;
#[cfg(feature = "perfcounter")]
use utralib::AtomicCsr;
use zeroize::Zeroize;

#[cfg(feature = "migration1")]
use crate::backend::migration1to2::*;

/// Implementation-specific PDDB structures: for Precursor/Xous OS pair
pub(crate) const MBBB_PAGES: usize = 10;
pub(crate) const FSCB_PAGES: usize = 16;

/// size of a physical page
pub const PAGE_SIZE: usize = spinor::SPINOR_ERASE_SIZE as usize;
/// size of a virtual page -- after the AES encryption and journaling overhead is subtracted
pub const VPAGE_SIZE: usize = PAGE_SIZE - size_of::<Nonce>() - size_of::<Tag>() - size_of::<JournalType>();

/// length of the ciphertext in an AES-GCM-SIV page with key commitments
/// equal to the total plaintext to be encrypted, including the journal number
/// does not include the MAC overhead
pub const KCOM_CT_LEN: usize = 4004;

pub(crate) const WRAPPED_AES_KEYSIZE: usize = AES_KEYSIZE + 8;
const SCD_VERSION: u32 = 2;

#[cfg(all(feature = "pddbtest", feature = "autobasis"))]
pub const BASIS_TEST_ROOTNAME: &'static str = "test";

#[derive(Zeroize)]
#[zeroize(drop)]
#[repr(C)] // this can map directly into Flash
pub(crate) struct StaticCryptoData {
    /// a version number for the block
    pub(crate) version: u32,
    /// aes-256 key of the system basis page table, encrypted with the User0 root key, and wrapped using NIST
    /// SP800-38F
    pub(crate) system_key_pt: [u8; WRAPPED_AES_KEYSIZE],
    /// aes-256 key of the system basis, encrypted with the User0 root key, and wrapped using NIST SP800-38F
    pub(crate) system_key: [u8; WRAPPED_AES_KEYSIZE],
    /// a pool of fixed data used for salting. The first 32 bytes are further subdivided for use in the HKDF.
    pub(crate) salt_base: [u8; 4096 - WRAPPED_AES_KEYSIZE * 2 - size_of::<u32>()],
}
impl StaticCryptoData {
    pub fn default() -> StaticCryptoData {
        StaticCryptoData {
            version: SCD_VERSION,
            system_key_pt: [0u8; WRAPPED_AES_KEYSIZE],
            system_key: [0u8; WRAPPED_AES_KEYSIZE],
            salt_base: [0u8; 4096 - WRAPPED_AES_KEYSIZE * 2 - size_of::<u32>()],
        }
    }
}
impl Deref for StaticCryptoData {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const StaticCryptoData as *const u8,
                size_of::<StaticCryptoData>(),
            ) as &[u8]
        }
    }
}
impl DerefMut for StaticCryptoData {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self as *mut StaticCryptoData as *mut u8,
                size_of::<StaticCryptoData>(),
            ) as &mut [u8]
        }
    }
}
#[derive(Eq, PartialEq)]
enum DnaMode {
    Normal,
    Churn,
    Migration,
}
#[derive(Zeroize)]
#[zeroize(drop)]
pub(crate) struct BasisKeys {
    pub(crate) pt: [u8; AES_KEYSIZE],
    pub(crate) data: [u8; AES_KEYSIZE],
}

struct MigrationCiphers {
    pt_ecb: Aes256,
    data_gcm_siv: Aes256GcmSiv,
    data_key: [u8; 32],
    aad_incoming: Vec<u8>,
    aad_local: Vec<u8>,
}

// emulated
#[cfg(not(target_os = "xous"))]
type EmuMemoryRange = EmuStorage;
#[cfg(not(target_os = "xous"))]
type EmuSpinor = HostedSpinor;

// native hardware
#[cfg(any(feature = "precursor", feature = "renode"))]
type EmuMemoryRange = xous::MemoryRange;
#[cfg(any(feature = "precursor", feature = "renode"))]
type EmuSpinor = spinor::Spinor;

#[cfg(all(feature = "board-baosec", feature = "gen2"))]
type EmuMemoryRange = xous::MemoryRange;
#[cfg(all(feature = "board-baosec", feature = "gen2"))]
type EmuSpinor = xous_swapper::Spinor;

pub(crate) struct PddbOs {
    spinor: EmuSpinor,
    #[cfg(feature = "gen1")]
    rootkeys: root_keys::RootKeys,
    tt: ticktimer_server::Ticktimer,
    pddb_mr: EmuMemoryRange,
    /// page table base -- location in FLASH, offset from physical bottom of pddb_mr
    pt_phys_base: PageAlignedPa,
    /// local key store -- one page, to store exactly one key, used for the system basis.
    /// the rest of the keys are generated on the fly entirely from the user password + a salt also stored in
    /// this page
    key_phys_base: PageAlignedPa,
    /// make before break buffer base -- location in FLASH, offset from physical bottom of pddb_mr
    mbbb_phys_base: PageAlignedPa,
    /// free space circular buffer base -- location in FLASH, offset from physical bottom of pddb_mr
    fscb_phys_base: PageAlignedPa,
    data_phys_base: PageAlignedPa,
    /// We keep a copy of the raw key around because we have to combine this with the AAD of a block to
    /// derive the AES-GCM-SIV cipher.
    system_basis_key: Option<BasisKeys>,
    /// derived cipher for handling fastspace -- cache it, so we can save the time cost of constructing the
    /// cipher key schedule
    cipher_ecb: Option<Aes256>,
    /// fast space cache
    fspace_cache: FspaceSet,
    /// memoize the location of the fscb log pages
    fspace_log_addrs: Vec<PageAlignedPa>,
    /// memoize the current target offset for the next log entry
    fspace_log_next_addr: Option<PhysAddr>,
    /// track roughly how big the log has gotten, so we can pre-emptively garbage collect it before we get
    /// too full.
    fspace_log_len: usize,
    /// a cached copy of the FPGA's DNA ID, used in the AAA records.
    dna: u64,
    /// DNA for migrations from restored backups coming from different devices
    migration_dna: u64,
    /// set which DNA to use
    dna_mode: DnaMode,
    /// reference to a TrngPool object that's shared among all the hardware functions
    entropy: Rc<RefCell<TrngPool>>,
    /// connection to the password request manager
    pw_cid: xous::CID,
    /// Number of consecutive failed login attempts
    failed_logins: u64,
    #[cfg(all(feature = "pddbtest", feature = "autobasis"))]
    testnames: HashSet<String>,
    /// Performance counter elements
    #[cfg(feature = "perfcounter")]
    perfclient: PerfClient,
    #[cfg(feature = "perfcounter")]
    pc_id: u32,
    #[cfg(feature = "perfcounter")]
    /// used to toggle performance profiling on or off
    use_perf: bool,
}

impl PddbOs {
    pub fn new(trngpool: Rc<RefCell<TrngPool>>, pw_cid: xous::CID) -> PddbOs {
        let xns = xous_names::XousNames::new().unwrap();
        #[cfg(any(feature = "precursor", feature = "renode"))]
        let pddb = xous::syscall::map_memory(
            xous::MemoryAddress::new(xous::PDDB_LOC as usize + xous::FLASH_PHYS_BASE as usize),
            None,
            PDDB_A_LEN as usize,
            xous::MemoryFlags::R | xous::MemoryFlags::RESERVE,
        )
        .expect("Couldn't map the PDDB memory range");
        #[cfg(any(feature = "precursor", feature = "renode"))]
        // Safety: all u8 values are valid
        log::info!(
            "pddb slice len: {}, PDDB_A_LEN: {}, raw len: {}",
            unsafe { pddb.as_slice::<u8>().len() },
            PDDB_A_LEN,
            pddb.len()
        ); // sanity check the PDDB size on init

        // the mbbb is located one page off from the Page Table
        let key_phys_base = PageAlignedPa::from(size_of::<PageTableInFlash>());
        log::debug!("key_phys_base: {:x?}", key_phys_base);
        let mbbb_phys_base = key_phys_base + PageAlignedPa::from(PAGE_SIZE);
        log::debug!("mbbb_phys_base: {:x?}", mbbb_phys_base);
        let fscb_phys_base =
            PageAlignedPa::from(mbbb_phys_base.as_u32() + MBBB_PAGES as u32 * PAGE_SIZE as u32);
        log::debug!("fscb_phys_base: {:x?}", fscb_phys_base);

        let llio = llio::Llio::new(&xns);
        let dna = llio.soc_dna().unwrap();

        // performance counter infrastructure, if selected
        #[cfg(feature = "perfcounter")]
        let event2_csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::generated::utra::event_source2::HW_EVENT_SOURCE2_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map event2 CSR range");
        #[cfg(feature = "perfcounter")]
        let perfclient = PerfClient::new(AtomicCsr::new(event2_csr.as_mut_ptr() as *mut u32));
        #[cfg(feature = "perfcounter")]
        let pc_id = xous::process::id() as u32;

        // native hardware
        #[cfg(any(feature = "precursor", feature = "renode"))]
        let ret = PddbOs {
            spinor: spinor::Spinor::new(&xns).unwrap(),
            rootkeys: root_keys::RootKeys::new(&xns, Some(AesRootkeyType::User0))
                .expect("FATAL: couldn't access RootKeys!"),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            pddb_mr: pddb,
            pt_phys_base: PageAlignedPa::from(0 as u32),
            key_phys_base,
            mbbb_phys_base,
            fscb_phys_base,
            data_phys_base: PageAlignedPa::from(
                fscb_phys_base.as_u32() + FSCB_PAGES as u32 * PAGE_SIZE as u32,
            ),
            system_basis_key: None,
            cipher_ecb: None,
            fspace_cache: FspaceSet::new(),
            fspace_log_addrs: Vec::<PageAlignedPa>::new(),
            fspace_log_next_addr: None,
            fspace_log_len: 0,
            dna,
            // default to our own DNA in this case
            migration_dna: dna,
            dna_mode: DnaMode::Normal,
            entropy: trngpool,
            pw_cid,
            failed_logins: 0,
            #[cfg(all(feature = "pddbtest", feature = "autobasis"))]
            testnames: HashSet::new(),
            #[cfg(feature = "perfcounter")]
            perfclient,
            #[cfg(feature = "perfcounter")]
            pc_id,
            #[cfg(feature = "perfcounter")]
            use_perf: true,
        };
        #[cfg(any(feature = "cramium-soc"))]
        let ret = PddbOs {
            spinor: crate::hw::Spinor::new(&xns).unwrap(),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            pddb_mr: pddb,
            pt_phys_base: PageAlignedPa::from(0 as u32),
            key_phys_base,
            mbbb_phys_base,
            fscb_phys_base,
            data_phys_base: PageAlignedPa::from(
                fscb_phys_base.as_u32() + FSCB_PAGES as u32 * PAGE_SIZE as u32,
            ),
            system_basis_key: None,
            cipher_ecb: None,
            fspace_cache: FspaceSet::new(),
            fspace_log_addrs: Vec::<PageAlignedPa>::new(),
            fspace_log_next_addr: None,
            fspace_log_len: 0,
            dna,
            // default to our own DNA in this case
            migration_dna: dna,
            dna_mode: DnaMode::Normal,
            entropy: trngpool,
            pw_cid,
            failed_logins: 0,
            #[cfg(all(feature = "pddbtest", feature = "autobasis"))]
            testnames: HashSet::new(),
            #[cfg(feature = "perfcounter")]
            perfclient,
            #[cfg(feature = "perfcounter")]
            pc_id,
            #[cfg(feature = "perfcounter")]
            use_perf: true,
        };
        // emulated
        #[cfg(not(target_os = "xous"))]
        let ret = {
            PddbOs {
                spinor: HostedSpinor::new(),
                rootkeys: root_keys::RootKeys::new(&xns, Some(AesRootkeyType::User0))
                    .expect("FATAL: couldn't access RootKeys!"),
                tt: ticktimer_server::Ticktimer::new().unwrap(),
                pddb_mr: EmuStorage::new(),
                pt_phys_base: PageAlignedPa::from(0 as u32),
                key_phys_base,
                mbbb_phys_base,
                fscb_phys_base,
                data_phys_base: PageAlignedPa::from(
                    fscb_phys_base.as_u32() + FSCB_PAGES as u32 * PAGE_SIZE as u32,
                ),
                system_basis_key: None,
                cipher_ecb: None,
                fspace_cache: FspaceSet::new(),
                fspace_log_addrs: Vec::<PageAlignedPa>::new(),
                fspace_log_next_addr: None,
                fspace_log_len: 0,
                dna,
                // default to our own DNA in this case
                migration_dna: dna,
                dna_mode: DnaMode::Normal,
                entropy: trngpool,
                pw_cid,
                failed_logins: 0,
                #[cfg(all(feature = "pddbtest", feature = "autobasis"))]
                testnames: HashSet::new(),
            }
        };
        ret
    }

    #[cfg(not(target_os = "xous"))]
    pub fn dbg_dump(&self, name: Option<String>, extra_keys: Option<&Vec<KeyExport>>) {
        self.pddb_mr.dump_fs(&name);
        let mut export = Vec::<KeyExport>::new();
        if let Some(key) = &self.system_basis_key {
            log::info!("(hosted mode debug) written key: {:x?}, {:x?}", key.pt, key.data);
            let mut name = [0 as u8; 64];
            for (&src, dst) in PDDB_DEFAULT_SYSTEM_BASIS.as_bytes().iter().zip(name.iter_mut()) {
                *dst = src;
            }
            export.push(KeyExport { basis_name: name, key: key.data, pt_key: key.pt });
        }
        if let Some(extra) = extra_keys {
            for key in extra {
                export.push(*key);
            }
        }
        self.pddb_mr.dump_keys(&export, &name);
    }

    #[allow(dead_code)]
    #[cfg(any(feature = "precursor", feature = "renode"))]
    pub fn dbg_dump(&self, _name: Option<String>) {
        // placeholder
    }

    #[allow(dead_code)]
    #[cfg(feature = "perfcounter")]
    pub fn set_use_perf(&mut self, use_perf: bool) { self.use_perf = use_perf; }

    #[allow(dead_code)]
    #[cfg(feature = "perfcounter")]
    pub fn perf_entry(&mut self, file_id: u32, meta: u32, index: u32, line: u32) {
        if self.use_perf {
            let entry = perf_entry!(self.pc_id, file_id, meta, index, line);
            self.perfclient.log_event_unchecked(entry);
        }
    }

    #[allow(dead_code)]
    #[cfg(not(target_os = "xous"))]
    /// used to reset the hardware structure for repeated runs of testing within a single invocation
    pub fn test_reset(&mut self) {
        self.fspace_cache = FspaceSet::new();
        self.fspace_log_addrs = Vec::<PageAlignedPa>::new();
        self.system_basis_key = None;
        self.cipher_ecb = None;
        self.fspace_log_next_addr = None;
        self.pddb_mr.reset();
    }

    #[cfg(feature = "gen1")]
    pub(crate) fn is_efuse_secured(&self) -> bool {
        self.rootkeys.is_efuse_secured().expect("couldn't query efuse security state") == Some(true)
    }

    #[cfg(feature = "gen2")]
    pub(crate) fn is_efuse_secured(&self) -> bool { unimplemented!() }

    pub(crate) fn nonce_gen(&self) -> Nonce {
        let nonce_array = self.entropy.borrow_mut().get_nonce();
        *Nonce::from_slice(&nonce_array)
    }

    #[allow(dead_code)]
    pub(crate) fn dna(&self) -> u64 { self.dna }

    pub(crate) fn trng_slice(&self, slice: &mut [u8]) { self.entropy.borrow_mut().get_slice(slice); }

    pub(crate) fn trng_u32(&self) -> u32 { self.entropy.borrow_mut().get_u32() }

    pub(crate) fn trng_u8(&self) -> u8 { self.entropy.borrow_mut().get_u8() }

    pub(crate) fn timestamp_now(&self) -> u64 { self.tt.elapsed_ms() }

    /// checks if the root keys are initialized, which is a prerequisite to formatting and mounting
    #[cfg(feature = "gen1")]
    pub(crate) fn rootkeys_initialized(&self) -> bool {
        self.rootkeys.is_initialized().expect("couldn't query initialization state of the rootkeys server")
    }

    #[cfg(feature = "gen2")]
    pub(crate) fn rootkeys_initialized(&self) -> bool {
        unimplemented!();
    }

    /// patches data at an offset starting from the data physical base address, which corresponds
    /// exactly to the first entry in the page table
    pub(crate) fn patch_data(&self, data: &[u8], offset: u32) {
        log::trace!("patch offset: {:x} len: {:x}", offset, data.len());
        // log::trace!("patch bef: {:x?}", &self.pddb_mr.as_slice::<u8>()[offset as usize +
        // self.data_phys_base.as_usize()..offset as usize + self.data_phys_base.as_usize() + 48]);
        assert!(
            data.len() + offset as usize <= PDDB_A_LEN - self.data_phys_base.as_usize(),
            "attempt to store past disk boundary"
        );
        self.spinor
            .patch(
                unsafe { self.pddb_mr.as_slice() },
                xous::PDDB_LOC,
                &data,
                offset + self.data_phys_base.as_u32(),
            )
            .expect("couldn't write to data region in the PDDB");
        //log::trace!("patch aft: {:x?}", &self.pddb_mr.as_slice::<u8>()[offset as usize +
        // self.data_phys_base.as_usize()..offset as usize + self.data_phys_base.as_usize() + 48]);
        // log::trace!("patch end: {:x?}", &self.pddb_mr.as_slice::<u8>()[offset as usize +
        // self.data_phys_base.as_usize() + 4048..offset as usize + self.data_phys_base.as_usize() + 4096])
    }

    fn patch_pagetable(&self, data: &[u8], offset: u32) {
        if cfg!(feature = "mbbb") {
            assert!(
                data.len() + offset as usize <= size_of::<PageTableInFlash>(),
                "attempt to patch past page table end"
            );
            // 1. Check if there is an MBBB structure existing. If so, copy it into the empty slot in the page
            //    table, then erase the mbbb.
            if let Some(mbbb) = self.mbbb_retrieve() {
                if let Some(erased_offset) = self.pt_find_erased_slot() {
                    self.spinor
                        .patch(
                            unsafe { self.pddb_mr.as_slice() },
                            xous::PDDB_LOC,
                            &mbbb,
                            self.pt_phys_base.as_u32() + erased_offset,
                        )
                        .expect("couldn't write to page table");
                }
                // if there *isn't* an erased slot, we still want to get rid of the MBBB structure. A lack of
                // an erased slot would indicate we lost power after we copied the previous
                // MBBB structure but before we could erase it from storage, so this picks up
                // where we left off.
                self.mbbb_erase();
            }
            // 2. find the page we're patching
            let base_page = offset as usize & !(PAGE_SIZE - 1);
            // 3. copy the data to a local buffer
            let mut mbbb_page = [0u8; PAGE_SIZE];
            for (&src, dst) in unsafe {
                self.pddb_mr.as_slice()[self.pt_phys_base.as_usize() + base_page
                    ..self.pt_phys_base.as_usize() + base_page + PAGE_SIZE]
                    .iter()
            }
            .zip(mbbb_page.iter_mut())
            {
                *dst = src;
            }
            // 4. patch the local buffer copy
            let base_offset = offset as usize & (PAGE_SIZE - 1);
            for (&src, dst) in data.iter().zip(mbbb_page[base_offset..base_offset + data.len()].iter_mut()) {
                *dst = src;
            }
            // 5. pick a random offset in the MBBB for the target patch, and write the data into the target
            let offset = (self.entropy.borrow_mut().get_u8() % MBBB_PAGES as u8) as u32 * PAGE_SIZE as u32;
            self.patch_mbbb(&mbbb_page, offset);
            // 6. erase the original page area, thus making the MBBB the authorative location
            let blank = [0xffu8; PAGE_SIZE];
            self.spinor
                .patch(
                    unsafe { self.pddb_mr.as_slice() },
                    xous::PDDB_LOC,
                    &blank,
                    self.pt_phys_base.as_u32() + base_page as u32,
                )
                .expect("couldn't write to page table");
        } else {
            self.patch_pagetable_raw(data, offset);
        }
    }

    /// Direct write to the page table, without MBBB buffering.
    fn patch_pagetable_raw(&self, data: &[u8], offset: u32) {
        assert!(
            data.len() + offset as usize <= size_of::<PageTableInFlash>(),
            "attempt to patch past page table end"
        );
        self.spinor
            .patch(
                unsafe { self.pddb_mr.as_slice() },
                xous::PDDB_LOC,
                &data,
                self.pt_phys_base.as_u32() + offset,
            )
            .expect("couldn't write to page table");
    }

    fn patch_keys(&self, data: &[u8], offset: u32) {
        assert!(
            data.len() + offset as usize <= PAGE_SIZE,
            "attempt to burn key data that is outside the key region"
        );
        log::info!("patching keys area with {} bytes", data.len());
        self.spinor
            .patch(
                unsafe { self.pddb_mr.as_slice() },
                xous::PDDB_LOC,
                data,
                self.key_phys_base.as_u32() + offset,
            )
            .expect("couldn't burn keys");
    }

    fn patch_mbbb(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= PAGE_SIZE * MBBB_PAGES, "mbbb patch would go out of bounds");
        self.spinor
            .patch(
                unsafe { self.pddb_mr.as_slice() },
                xous::PDDB_LOC,
                data,
                self.mbbb_phys_base.as_u32() + offset,
            )
            .expect("couldn't burn mbbb");
    }

    /// raw patch is provided for 128-bit incremental updates to the FLASH. For FastSpace master record
    /// writes, see fast_space_write()
    fn patch_fscb(&self, data: &[u8], offset: u32) {
        assert!(data.len() + offset as usize <= PAGE_SIZE * FSCB_PAGES, "fscb patch would go out of bounds");
        self.spinor
            .patch(
                unsafe { self.pddb_mr.as_slice() },
                xous::PDDB_LOC,
                data,
                self.fscb_phys_base.as_u32() + offset,
            )
            .expect("couldn't burn fscb");
    }

    /// Public function that "does the right thing" for patching in a page table entry based on a virtual to
    /// physical mapping. Note that the `va` is an *address* (in units of bytes), and the `phy_page_num` is
    /// a *page number* (so a physical address divided by the page size). It's a slightly awkward units, but
    /// it saves a bit of math going back and forth between the native storage formats of the records.
    pub(crate) fn pt_patch_mapping(&self, va: VirtAddr, phys_page_num: u32, cipher: &Aes256) {
        let mut pte = Pte::new(va, PtFlags::CLEAN, Rc::clone(&self.entropy));
        let mut block = Block::from_mut_slice(pte.deref_mut());
        //log::info!("pte pt: {:x?}", block);
        cipher.encrypt_block(&mut block);
        //log::info!("pte ct: {:x?}", block);
        self.patch_pagetable(&block, phys_page_num * aes::BLOCK_SIZE as u32);
    }

    /// erases a page table entry by overwriting it with garbage
    pub(crate) fn pt_erase(&mut self, phys_page_num: u32) {
        let mut eraseblock = [0u8; aes::BLOCK_SIZE];
        self.trng_slice(&mut eraseblock);
        self.patch_pagetable(&eraseblock, phys_page_num * aes::BLOCK_SIZE as u32);
    }

    /// Searches the page table for an MBBB slot. This is currently an O(N) search but
    /// in practice for Precursor there are only 8 pages, so it's quite fast on average.
    /// This would want to be optimized or cached for a much larger filesystem.
    /// Returns the physical address of the erased slot as an offset from the pt_phys_base()
    fn pt_find_erased_slot(&self) -> Option<PhysAddr> {
        let pt: &[u8] = unsafe {
            &self.pddb_mr.as_slice()
                [self.pt_phys_base.as_usize()..self.pt_phys_base.as_usize() + size_of::<PageTableInFlash>()]
        };
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for (index, page) in pt.chunks(PAGE_SIZE).enumerate() {
            if page[..aes::BLOCK_SIZE] == blank {
                return Some((index * PAGE_SIZE) as PhysAddr);
            }
        }
        None
    }

    fn pt_as_slice(&self) -> &[u8] {
        unsafe {
            &self.pddb_mr.as_slice()
                [self.pt_phys_base.as_usize()..self.pt_phys_base.as_usize() + size_of::<PageTableInFlash>()]
        }
    }

    /// scans the page tables and returns all entries for a given basis
    /// basis_name is needed to decrypt pages in case of a journal conflict
    ///
    /// Here, we may encounter page table entries that are bogus for two reasons:
    ///    1. PT entries that pass the checksum, but don't map to valid data due to random collisions from the
    ///       short 32-bit checksum space
    ///    2. Conflicting entries due to an unclean power-down
    ///
    /// This routine does a "lazy" check of improper data:
    /// If the bad data happens to land on the same page mapping as valid data, a conflict resolution happens
    /// where the block is decrypted and validated.
    ///   - If there is only one valid block, the valid block is picked to populate the page table (handles
    ///     half of case 1)
    ///   - If two valid blocks are found, the one with the older journal entry is used (handles the easy
    ///     version of case 2)
    ///   - If two valid blocks with identical journal versions happen (hard version of case 2), this is an
    ///     internal consistency error. It shouldn't happen; but, if it happens, it retains the first version
    ///     encountered and prints an error to the log.
    ///
    /// This leaves the "hard version" of case 1 - there was a checksum collision, and it wasn't detected
    /// because no other valid block happened to map to it. In this case, the "imposter" entry is left to
    /// stand, under the theory that typically this represents this represents a "leak" of free space
    /// where this orphaned bock may never be allocated or used. However, because these are rare (perhaps
    /// around 1 in a billion chance?) this leakage is fine. Note that if the FSCB is generated before this
    /// page gets allocated, the imposter page is avoided in the scan, so this leaked memory is "forever".
    /// An orphaned node search could try to chase this out, if it becomes a problem.
    ///
    /// If the "imposter" entry also happens to be "allocated" down the road (that is, the FSCB happens to
    /// have an entry that also maps to the imposter), the imposter is effectively de-allocated because
    /// the FSCB report is inserted directly into the page table, overwriting the impostor entry in the
    /// paget able. On the next mount, this turns into the "trivial conflict" case, where one page will
    /// validate and the other will not.
    pub(crate) fn pt_scan_key(
        &self,
        pt_key: &[u8; AES_KEYSIZE],
        data_key: &[u8; AES_KEYSIZE],
        basis_name: &str,
    ) -> Option<HashMap<VirtAddr, PhysPage>> {
        use aes::cipher::KeyInit;
        let cipher = Aes256::new(&GenericArray::from_slice(pt_key));
        let pt = self.pt_as_slice();
        let mut map = HashMap::<VirtAddr, PhysPage>::new();
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for (page_index, pt_page) in pt.chunks(PAGE_SIZE).enumerate() {
            let clean_page = if pt_page[..aes::BLOCK_SIZE] == blank {
                if let Some(page) = self.mbbb_retrieve() {
                    page
                } else {
                    log::debug!(
                        "Blank page in PT found, but no MBBB entry exists. PT is either corrupted or not initialized!"
                    );
                    pt_page
                }
            } else {
                pt_page
            };
            for (index, candidate) in clean_page.chunks(aes::BLOCK_SIZE).enumerate() {
                // encryption is in-place, but the candidates are read-only, so we have to copy them to a new
                // location
                let mut block = Block::clone_from_slice(candidate);
                cipher.decrypt_block(&mut block);
                if let Some(pte) = Pte::try_from_slice(block.as_slice()) {
                    let mut pp = PhysPage(0);
                    pp.set_page_number(((page_index * PAGE_SIZE / aes::BLOCK_SIZE) + index) as PhysAddr);
                    // the state is clean because this entry is, by definition, synchronized with the disk
                    pp.set_clean(true);
                    pp.set_valid(true);
                    pp.set_space_state(SpaceState::Used);
                    // handle conflicting journal versions here
                    if let Some(prev_page) = map.get(&pte.vaddr()) {
                        let cipher = Aes256GcmSiv::new(data_key.into());
                        let aad = self.data_aad(basis_name);
                        if pte.vaddr() != VirtAddr::new(VPAGE_SIZE as u64).unwrap() {
                            // handle case that there is a conflict in data areas
                            let prev_data = self.data_decrypt_page(&cipher, &aad, prev_page);
                            let new_data = self.data_decrypt_page(&cipher, &aad, &pp);
                            if let Some(new_d) = new_data {
                                if let Some(prev_d) = prev_data {
                                    let prev_j = JournalType::from_le_bytes(
                                        prev_d[..size_of::<JournalType>()].try_into().unwrap(),
                                    );
                                    let new_j = JournalType::from_le_bytes(
                                        new_d[..size_of::<JournalType>()].try_into().unwrap(),
                                    );
                                    if new_j > prev_j {
                                        map.insert(pte.vaddr(), pp);
                                    } else if new_j == prev_j {
                                        log::error!(
                                            "Found duplicate blocks with same journal age, picking arbitrary block and moving on..."
                                        );
                                    }
                                } else {
                                    self.resolve_pp_journal(&mut pp);
                                    // prev data was bogus anyways, replace with the new entry
                                    map.insert(pte.vaddr(), pp);
                                }
                            } else {
                                // new data is bogus, ignore it
                                log::warn!(
                                    "conflicting PTE found, but data is bogus: v{:x?}:p{:x?}",
                                    pte.vaddr(),
                                    prev_page
                                );
                            }
                        } else {
                            // handle case that there is a conflict in root structure
                            let prev_data = self.data_decrypt_page_with_commit(data_key, &aad, prev_page);
                            let new_data = self.data_decrypt_page_with_commit(data_key, &aad, &pp);
                            if let Some(new_d) = new_data {
                                if let Some(prev_d) = prev_data {
                                    let prev_j = JournalType::from_le_bytes(
                                        prev_d[..size_of::<JournalType>()].try_into().unwrap(),
                                    );
                                    let new_j = JournalType::from_le_bytes(
                                        new_d[..size_of::<JournalType>()].try_into().unwrap(),
                                    );
                                    if new_j > prev_j {
                                        map.insert(pte.vaddr(), pp);
                                    } else if new_j == prev_j {
                                        log::error!(
                                            "Found duplicate blocks with same journal age, picking arbitrary block and moving on..."
                                        );
                                    }
                                } else {
                                    self.resolve_pp_journal(&mut pp);
                                    // prev data was bogus anyways, replace with the new entry
                                    map.insert(pte.vaddr(), pp);
                                }
                            } else {
                                // new data is bogus, ignore it
                                // new data is bogus, ignore it
                                log::warn!(
                                    "conflicting basis root found, but data is bogus: v{:x?}:p{:x?}",
                                    pte.vaddr(),
                                    prev_page
                                );
                            }
                        }
                    } else {
                        self.resolve_pp_journal(&mut pp);
                        map.insert(pte.vaddr(), pp);
                    }
                }
            }
        }
        if map.len() > 0 { Some(map) } else { None }
    }

    /// Pages drawn from disk might already have come from the FSCB. We need to make the journal number
    /// of these consistent with those in the FSCB so later on when they are retired we don't have journal
    /// conflicts.
    fn resolve_pp_journal(&self, pp: &mut PhysPage) {
        if let Some(fs_pp) = self.fspace_cache.get(pp) {
            if pp.journal() < fs_pp.journal() {
                log::debug!("bumping journal {}->{}: {:x?}", pp.journal(), fs_pp.journal(), pp);
                pp.set_journal(fs_pp.journal());
            }
        }
    }

    /// maps a StaticCryptoData structure into the key area of the PDDB.
    fn static_crypto_data_get(&self) -> &StaticCryptoData {
        let scd_ptr = unsafe {
            self.pddb_mr.as_slice::<u8>()
                [self.key_phys_base.as_usize()..self.key_phys_base.as_usize() + PAGE_SIZE]
                .as_ptr() as *const StaticCryptoData
        };
        let scd: &StaticCryptoData = unsafe { scd_ptr.as_ref().unwrap() };
        scd
    }

    #[cfg(feature = "migration1")]
    /// needs to reside in this object because it accesses the key_phys_base registers, which have good reason
    /// to be private this needs to return a full copy of the data, because the copy on disk is going
    /// away.
    fn static_crypto_data_get_v1(&self) -> StaticCryptoDataV1 {
        let mut scd = StaticCryptoDataV1::default();
        let scd_ptr = self.pddb_mr.as_slice::<u8>()
            [self.key_phys_base.as_usize()..self.key_phys_base.as_usize() + PAGE_SIZE]
            .as_ptr() as *const StaticCryptoDataV1;
        let scd_flash: &StaticCryptoDataV1 = unsafe { scd_ptr.as_ref().unwrap() };
        scd.version = scd_flash.version;
        scd.system_key = scd_flash.system_key;
        scd.salt_base = scd_flash.salt_base;
        scd
    }

    /// takes the key and writes it with zero, using hard pointer math and a compiler fence to ensure
    /// the wipe isn't optimized out.
    #[allow(dead_code)]
    fn syskey_erase(&mut self) {
        // this implements zeroize, so replacing it with None should do the trick
        self.system_basis_key = None;
        self.cipher_ecb = None;
    }

    #[cfg(feature = "gen1")]
    pub(crate) fn clear_password(&self) { self.rootkeys.clear_password(AesRootkeyType::User0); }

    #[cfg(feature = "gen2")]
    pub(crate) fn clear_password(&self) { todo!("implement password clearing") }

    pub(crate) fn try_login(&mut self) -> PasswordState {
        use aes::cipher::KeyInit;
        if self.system_basis_key.is_none() || self.cipher_ecb.is_none() {
            let scd = self.static_crypto_data_get();
            if scd.version == 0xFFFF_FFFF {
                // system is in the blank state
                return PasswordState::Uninit;
            }
            if scd.version != SCD_VERSION {
                log::error!("Version mismatch for keystore, declaring database as uninitialized");
                return PasswordState::Uninit;
            }
            // prep a migration structure, in case it is required. Bit of history:
            //
            // Turns out that the keywrap implementation we found does not work. It does not generate
            // results that match the NIST test vectors at
            // https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/mac/kwtestvectors.zip
            //
            // The `aes_kw` functions in the RustCrypto library do create matching results.
            // However, back when the PDDB was written, this crate didn't exist (the first commit
            // to `aes_kw` was Jan 2022, the PDDB was started back in October 2021, and around
            // the time I searched for an aes-kw implementation, the `aes_kw` code was just pushed
            // at 0.1.0, and not registering in Google).
            //
            // Ah well. Better we caught it now than later! This structure here prepares for
            // a potential migration in case we are told we need to do that. The main regression
            // we now suffer is that the key return structure is not zeroized on drop automatically,
            // because we are using a fancy compound enum and our version of zeroize is ancient
            // due to interpedencies with other crypto crates.
            let mut new_crypto_keys = StaticCryptoData::default();
            new_crypto_keys.version = scd.version;
            new_crypto_keys.system_key_pt.copy_from_slice(&scd.system_key_pt);
            new_crypto_keys.system_key.copy_from_slice(&scd.system_key);
            new_crypto_keys.salt_base.copy_from_slice(&scd.salt_base);

            // now try to populate our keys, and prep a migration if necessary
            let mut syskey_pt = [0u8; 32];
            let mut syskey = [0u8; 32];
            #[cfg(feature = "gen1")]
            {
                let mut keys_updated = false;
                match self.rootkeys.unwrap_key(&scd.system_key_pt, AES_KEYSIZE) {
                    Ok(skpt) => syskey_pt.copy_from_slice(&skpt),
                    Err(e) => match e {
                        KeywrapError::UpgradeToNew((key, upgrade)) => {
                            log::warn!("pt key migration required");
                            syskey_pt.copy_from_slice(&key);
                            new_crypto_keys.system_key_pt.copy_from_slice(&upgrade);
                            keys_updated = true;
                        }
                        _ => {
                            log::error!("Couldn't unwrap our system key: {:?}", e);
                            self.failed_logins = self.failed_logins.saturating_add(1);
                            return PasswordState::Incorrect(self.failed_logins);
                        }
                    },
                }
                match self.rootkeys.unwrap_key(&scd.system_key, AES_KEYSIZE) {
                    Ok(sk) => syskey.copy_from_slice(&sk),
                    Err(e) => match e {
                        KeywrapError::UpgradeToNew((key, upgrade)) => {
                            log::warn!("data key migration required");
                            syskey.copy_from_slice(&key);
                            new_crypto_keys.system_key.copy_from_slice(&upgrade);
                            keys_updated = true;
                        }
                        _ => {
                            log::error!("Couldn't unwrap our system key: {:?}", e);
                            self.failed_logins = self.failed_logins.saturating_add(1);
                            return PasswordState::Incorrect(self.failed_logins);
                        }
                    },
                }
                if keys_updated {
                    log::warn!("Migration event from incorrectly wrapped key");
                    self.patch_keys(new_crypto_keys.deref(), 0);
                }
            }
            #[cfg(feature = "gen2")]
            {
                todo!("implement password entry")
            }

            let cipher = Aes256::new(GenericArray::from_slice(&syskey_pt));
            self.cipher_ecb = Some(cipher);
            let mut system_key_pt: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
            // copy over the pt key only after we've unwrapped and decrypted the data key
            system_key_pt.copy_from_slice(&syskey_pt);
            // copy over the data key
            let mut system_key: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
            system_key.copy_from_slice(&syskey);

            self.system_basis_key = Some(BasisKeys { data: system_key, pt: system_key_pt });
            // erase the plaintext keys
            let nuke = syskey.as_mut_ptr();
            for i in 0..syskey.len() {
                unsafe { nuke.add(i).write_volatile(0) };
            }
            let nuke = syskey_pt.as_mut_ptr();
            for i in 0..syskey_pt.len() {
                unsafe { nuke.add(i).write_volatile(0) };
            }
            self.failed_logins = 0;
            PasswordState::Correct
        } else {
            self.failed_logins = 0;
            PasswordState::Correct
        }
    }

    fn syskey_ensure(&mut self) {
        while self.try_login() != PasswordState::Correct {
            self.clear_password(); // clear the bad password entry
            #[cfg(feature = "gen1")]
            {
                let xns = xous_names::XousNames::new().unwrap();
                let modals = modals::Modals::new(&xns).expect("can't connect to Modals server");
                modals
                    .show_notification(t!("pddb.badpass_infallible", locales::LANG), None)
                    .expect("notification failed");
            }
            #[cfg(feature = "gen2")]
            {
                todo!("implement feedback that syskey is not available");
            }
        }
    }

    fn mbbb_as_slice(&self) -> &[u8] {
        unsafe {
            &self.pddb_mr.as_slice()
                [self.mbbb_phys_base.as_usize()..self.mbbb_phys_base.as_usize() + MBBB_PAGES * PAGE_SIZE]
        }
    }

    fn mbbb_retrieve(&self) -> Option<&[u8]> {
        // Invariant: MBBB pages should be blank, unless a page is stashed there. So, just check the first
        // AES key size region for "blankness"
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for page in self.mbbb_as_slice().chunks(PAGE_SIZE) {
            if page[..aes::BLOCK_SIZE] == blank {
                continue;
            } else {
                return Some(page);
            }
        }
        None
    }

    fn mbbb_erase(&self) {
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for (index, page) in self.mbbb_as_slice().chunks(PAGE_SIZE).enumerate() {
            if page[..aes::BLOCK_SIZE] == blank {
                continue;
            } else {
                let blank_page = [0xffu8; PAGE_SIZE];
                self.patch_mbbb(&blank_page, (index * PAGE_SIZE) as u32);
            }
        }
    }

    /// Create fast_space AAD: name, version number, and FPGA ID.
    /// This data is "well known", and fixed for every device, but changes
    /// from device to device. It prevents records from one device from being copied
    /// and used on another, and it also makes it annoying to swap out the FPGA.
    /// This makes it a bit harder to repair, but also makes it harder for an adversary
    /// to change out the FPGA on your board without also having to patch the OS.
    /// If you are doing a repair, patch out the LLIO function that returns the DNA with
    /// the desired target DNA to effectively bypass the check (you need to, of course,
    /// know what that DNA is in the first place, so hopefully you were able to extract
    /// it before you destroyed the FPGA).
    fn fast_space_aad(&self, aad: &mut Vec<u8>) {
        aad.extend_from_slice(PDDB_FAST_SPACE_SYSTEM_BASIS.as_bytes());
        aad.extend_from_slice(&PDDB_VERSION.to_le_bytes());
        match self.dna_mode {
            DnaMode::Normal | DnaMode::Churn => aad.extend_from_slice(&self.dna.to_le_bytes()),
            DnaMode::Migration => aad.extend_from_slice(&self.migration_dna.to_le_bytes()),
        }
    }

    /// Assumes you are writing a "most recent" version of FastSpace. Thus
    /// Anytime the fscb is updated, all the partial records are nuked, as well as any existing record.
    /// Then, a _random_ location is picked to place the structure to help with wear levelling.
    fn fast_space_write(&mut self, fs: &FastSpace) {
        use aes::cipher::KeyInit;
        self.syskey_ensure();
        if let Some(system_basis_key) = &self.system_basis_key {
            let cipher = Aes256GcmSiv::new(&system_basis_key.data.into());
            let nonce_array = self.entropy.borrow_mut().get_nonce();
            let nonce = Nonce::from_slice(&nonce_array);
            let fs_ser: &[u8] = fs.deref();
            assert!(
                ((fs_ser.len() + size_of::<Nonce>() + size_of::<Tag>()) & (PAGE_SIZE - 1)) == 0,
                "FastSpace record is not page-aligned in size!"
            );
            // AAD + data => Payload
            let mut aad = Vec::<u8>::new();
            self.fast_space_aad(&mut aad);
            let payload = Payload { msg: fs_ser, aad: &aad };
            // these are useful for debug, but do a hard-comment on them because they leak secret info
            //log::info!("key: {:x?}", key);
            //log::info!("nonce: {:x?}", nonce);
            //log::info!("aad: {:x?}", aad);
            //log::info!("payload: {:x?}", fs_ser);
            let ciphertext = cipher.encrypt(nonce, payload).expect("failed to encrypt FastSpace record");
            //log::info!("ct_len: {}", ciphertext.len());
            //log::info!("mac: {:x?}", &ciphertext[ciphertext.len()-16..]);
            let ct_to_flash = ciphertext.deref();
            // determine which page we're going to write the ciphertext into
            let page_search_limit =
                FSCB_PAGES - ((PageAlignedPa::from(ciphertext.len()).as_usize() / PAGE_SIZE) - 1);
            let dest_page_start = self.entropy.borrow_mut().get_u32() % page_search_limit as u32;
            // atomicity of the FreeSpace structure is a bit of a tough topic. It's a fairly hefty structure,
            // that runs a risk of corruption as it's being written, if power is lost or the system crashes.
            // However, the guiding principle of this ordering is that it's better to have no FastSpace
            // structure (and force a re-computation of it by scanning all the open Basis), than
            // it is to have a broken FastSpace structure + stale SpaceUpdates. In particular a
            // stale SpaceUpdate would lead the system to conclude that some pages are free when
            // they aren't. Thus, we prefer to completely erase the FSCB region before committing
            // the updated version.
            let patch_data = [&nonce_array, ct_to_flash].concat();
            assert!(patch_data.len() % PAGE_SIZE == 0); // should be guaranteed by design, if not, throw an error during early testing.
            let dest_page_end = dest_page_start + (patch_data.len() / PAGE_SIZE) as u32;
            log::info!(
                "picking random pages [{}-{}) out of {} pages for fscb",
                dest_page_start,
                dest_page_end,
                FSCB_PAGES
            );
            {
                // this is where we begin the "it would be bad if we lost power about now" code region
                let blank_sector: [u8; PAGE_SIZE] = [0xff; PAGE_SIZE]; // prep a "blank" page for the loop
                for offset in 0..FSCB_PAGES as u32 {
                    if offset >= dest_page_start && offset < dest_page_end {
                        // patch the FSCB data in when we find it, and skip on later iterations that are
                        // within the page range of the FSCB
                        if offset == dest_page_start {
                            self.patch_fscb(&patch_data, dest_page_start * PAGE_SIZE as u32);

                            // Catch cache coherence issues. For now, we do a "hard panic" so that we know
                            // there is a cache coherence problem.
                            // We could "paper over" this by re-reading the data, but really, this problem
                            // should be solved by the d-cache flush in the SPINOR
                            // primitive.
                            let page_start = dest_page_start as usize * PAGE_SIZE;
                            let fscb_slice =
                                &unsafe { self.pddb_mr.as_slice() }[self.fscb_phys_base.as_usize()
                                    ..self.fscb_phys_base.as_usize() + FSCB_PAGES * PAGE_SIZE];
                            let mut fscb_buf = [0; FASTSPACE_PAGES * PAGE_SIZE - size_of::<Nonce>()];
                            // copy the encrypted data to the decryption buffer
                            fscb_buf.copy_from_slice(
                                &fscb_slice[page_start + size_of::<Nonce>()
                                    ..page_start + FASTSPACE_PAGES * PAGE_SIZE],
                            );
                            let mut errcount = 0;
                            for (index, (&buf, &flash)) in
                                fscb_buf.iter().zip(&patch_data[size_of::<Nonce>()..]).enumerate()
                            {
                                if buf != flash {
                                    if errcount < 128 {
                                        log::warn!(
                                            "Cache coherence issue at 0x{:x}: buf 0x{:x} -> rbk 0x{:x}",
                                            index,
                                            buf,
                                            flash
                                        );
                                    }
                                    errcount += 1;
                                }
                            }
                            if errcount != 0 {
                                log::warn!(
                                    "Cache coherence issue detected: total buf<->rbk errors: {}",
                                    errcount
                                );
                                panic!(
                                    "FSCB write to pages [{}-{}) failed: cache coherence failure, with {} errors",
                                    dest_page_start, dest_page_end, errcount
                                );
                            }
                            /*
                            let mut daad = Vec::<u8>::new();
                            self.fast_space_aad(&mut daad);
                            let payload = Payload {
                                msg: &fscb_buf,
                                aad: &daad,
                            };
                            match cipher.decrypt(
                                Nonce::from_slice(&fscb_slice[page_start..page_start + size_of::<Nonce>()]),
                             payload
                            ) {
                                Ok(_) => log::info!("FSCB readback successful"),
                                Err(e) => {
                                    log::warn!("FSCB update at page {}({}) did not write successfully. Error: {:?}", dest_page_start, page_start, e);
                                    log::warn!("Patch data len {}, readback len {}", patch_data.len(), FASTSPACE_PAGES * PAGE_SIZE);
                                    let mut errcount = 0;
                                    for (index, (&patch, &flash)) in
                                    patch_data.iter().zip(&fscb_slice[page_start..page_start + FASTSPACE_PAGES * PAGE_SIZE]).enumerate() {
                                        if patch != flash {
                                            if errcount < 128 {
                                                log::warn!("Mismatch at 0x{:x}: patch 0x{:x} -> rbk 0x{:x}", index, patch, flash);
                                            }
                                            errcount += 1;
                                        }
                                    }
                                    log::warn!("Total errors: {}", errcount);
                                    if errcount == 0 {
                                        match cipher.decrypt(
                                            Nonce::from_slice(&patch_data[..size_of::<Nonce>()]),
                                            Payload {
                                                msg: &patch_data[size_of::<Nonce>()..],
                                                aad: &daad,
                                            }
                                        ) {
                                            Ok(_) => {
                                                log::warn!("0 readback errors detected, and source data decrypts correctly. Suspect read error on FLASH!");
                                                let mut errcount = 0;
                                                for (index, (&buf, &flash)) in fscb_buf.iter().zip(&patch_data[size_of::<Nonce>()..]).enumerate() {
                                                    if buf != flash {
                                                        if errcount < 128 {
                                                            log::warn!("Mismatch at 0x{:x}: buf 0x{:x} -> rbk 0x{:x}", index, buf, flash);
                                                        }
                                                        errcount += 1;
                                                    }
                                                }
                                                log::warn!("Total buf<->rbk errors: {}", errcount);
                                                if errcount == 0 {
                                                    log::warn!("Suspect AAD issue: patch aad {:?}, rbk aad {:?}", aad, daad);
                                                }
                                            },
                                            Err(_e) => log::warn!("Source patch data was not encrypted correctly!"),
                                        }
                                    }
                                    panic!("FSCB write to pages [{}-{}) failed integrity check with {} errors", dest_page_start, dest_page_end, errcount);
                                }
                            }
                            */
                        } else {
                            // skip, because we already patched it in the first page we hit
                        }
                    } else {
                        // erase all the other pages in the FSCB
                        self.patch_fscb(&blank_sector, offset * PAGE_SIZE as u32);
                    }
                }
                // commit the fscb data
                log::info!("patch data len: {}", patch_data.len());
            } // end "it would be bad if we lost power now" region

        // note: this function should be followed up by a fast_space_read() to regenerate the temporary
        // bookkeeping variables that are not reset by this function.
        } else {
            panic!("invalid state!");
        }
    }

    #[cfg(feature = "hwtest")]
    pub fn stresstest_read(&mut self, pagecount: u32, iters: u32) -> u32 {
        let pagecount = if pagecount == 0 { 2 } else { pagecount };
        let iters = if iters == 0 { 16 } else { iters };
        let test_slice = &self.pddb_mr.as_slice()
            [self.data_phys_base.as_usize()..self.data_phys_base.as_usize() + pagecount as usize * PAGE_SIZE];
        let mut total_errs = 0;
        for attempt in 0..iters {
            log::info!("Attempt {} of {}", attempt + 1, iters);
            let mut dest_mem = vec![0u8; pagecount as usize * PAGE_SIZE]; // do a fresh alloc at every loop
            dest_mem.copy_from_slice(test_slice);
            let mut errcount = 0;
            for (index, (&flash, &mem)) in test_slice.iter().zip(dest_mem.iter()).enumerate() {
                if flash != mem {
                    errcount += 1;
                    if errcount < 128 {
                        log::warn!(
                            "Attempt {}: Mismatch at 0x:{:x}: flash:{:x} <-> mem {:x}",
                            attempt,
                            index,
                            flash,
                            mem
                        );
                    }
                }
            }
            total_errs += errcount;
            dest_mem.zeroize();
        }
        total_errs
    }

    /// Sweeps through the entire set of known data (as indicated in `page_heap`) and
    /// returns a subset of the total free space in a PhysPage vector that is a list of physical pages,
    /// in random order, that can be used by PDDB operations in the future without worry about
    /// accidentally overwriting Basis data that are locked.
    ///
    /// The function is coded to prioritize small peak memory footprint over speed, as it
    /// needs to run in a fairly memory-constrained environment, keeping in mind that if the PDDB
    /// structures were to be extended to run on say, an external USB drive with gigabytes of space,
    /// we cannot afford to naively allocate vectors that count every single page.
    fn fast_space_generate(&mut self, mut page_heap: BinaryHeap<Reverse<u32>>) -> Vec<PhysPage> {
        let mut free_pool = Vec::<usize>::new();
        let max_entries = FASTSPACE_PAGES * PAGE_SIZE / size_of::<PhysPage>();
        free_pool.reserve_exact(max_entries);

        // 1. check that the page_heap has enough entries
        let total_used_pages = page_heap.len();
        let total_pages = (PDDB_A_LEN - self.data_phys_base.as_usize()) / PAGE_SIZE;
        let total_free_pages = total_pages - total_used_pages;
        log::info!("page alloc: {} used; {} free; {} total", total_used_pages, total_free_pages, total_pages);
        if total_free_pages == 0 {
            log::warn!("Disk is out of space, no free pages available!");
            // return an empty free_pool vector.
            return Vec::<PhysPage>::new();
        }
        // 2. fill the free_pool, avoiding entries in the page_heap. This algorithm
        // uses a fixed amount of storage regardless of the size of the disk, but
        // it produces a free_pool that has entries biased toward the high
        // addresses.
        let mut min_used_page = page_heap.pop();
        // consider every page
        for page_candidate in 0..(PDDB_A_LEN - self.data_phys_base.as_usize()) / PAGE_SIZE {
            // if the page is used, skip it.
            if let Some(Reverse(mp)) = min_used_page {
                if page_candidate == mp as usize {
                    log::debug!("removing used page from free_pool: {}", mp);
                    min_used_page = page_heap.pop();
                    continue;
                }
            }
            // page is free. if we've space in the pool, just deposit it there
            if free_pool.len() < max_entries {
                free_pool.push(page_candidate);
            } else {
                // page is free, but we have no space in the pool.
                // pick a random page from the pool, and replace it with the current page
                let index = self.entropy.borrow_mut().get_u32() as usize % free_pool.len();
                free_pool[index] = page_candidate;
            }
        }
        // 3. shuffle the contents of free_pool. This is important in the case that the
        // amount of free space starts to approach the size of the free pool, as it will
        // essentially come out of step 2 as a sorted list.
        // this shuffle is stolen out of the rand crate directly -- it's a small function,
        // and pulling in the ENTIRE rand crate for this code seemed very unnecessary.
        // https://github.com/rust-random/rand/blob/0f4fc6b4c303696bd5f8765a375162ac7142b1df/src/seq/mod.rs#L586-L592
        for i in (1..free_pool.len()).rev() {
            // invariant: elements with index > i have been locked in place.
            free_pool.swap(i, self.entropy.borrow_mut().get_u32() as usize % (i + 1));
        }
        log::info!("free_pool initial count: {}", free_pool.len());

        // 4. ensure that the free pool stays within the defined deniability ratio + noise
        let mut noise =
            (self.entropy.borrow_mut().get_u32() as f32 / u32::MAX as f32) * FSCB_FILL_UNCERTAINTY;
        if self.entropy.borrow_mut().get_u8() > 127 {
            noise = -noise;
        }
        let deniable_free_pages = (total_free_pages as f32 * (FSCB_FILL_COEFFICIENT + noise)) as usize;
        // we're guaranteed to have at least one free page, because we errored out if the pages was 0 above.
        let deniable_free_pages = if deniable_free_pages == 0 { 1 } else { deniable_free_pages };
        free_pool.truncate(deniable_free_pages);
        log::warn!(
            "total_free: {}; free_pool after PD trim: {}; max pages allowed: {}",
            total_free_pages,
            free_pool.len(),
            deniable_free_pages
        );

        // 5. Take the free_pool and annotate it for writing to disk
        let mut page_pool = Vec::<PhysPage>::new();
        for page in free_pool {
            let mut pp = PhysPage(0);
            pp.set_journal(self.trng_u8() % FSCB_JOURNAL_RAND_RANGE);
            pp.set_page_number(page as PhysAddr);
            pp.set_space_state(SpaceState::Free);
            pp.set_valid(true);
            page_pool.push(pp);
        }
        page_pool
    }

    fn fscb_deref(&self) -> &[u8] {
        unsafe {
            &self.pddb_mr.as_slice()
                [self.fscb_phys_base.as_usize()..self.fscb_phys_base.as_usize() + FSCB_PAGES * PAGE_SIZE]
        }
    }

    /// Reads the data structures in the FSCB, if any, and stores the results in the fspace_cache HashSet.
    /// Note the following convention on the fscb: if the first 128 bits of a page are all 1's, then that
    /// sector cannot contain the master FastSpace record. Also, if a sector is to contain *any* data, the
    /// first piece of data must start at exactly 16 bytes into the page (at the 129th bit). Examples:
    ///
    /// FF = must be all 1's
    /// xx/yy/zz = AES encrypted data. Techincally AES includes the all 1's ciphertext in its set, but it's
    /// extremely unlikely.
    ///
    /// Byte #
    /// | 0  | 1  | 2  | 3  | 4  | 5  | 6  | 7  | 8  | 9  |  A |  B | C  | D  | E  | F  | 10 | 11 | 12 | 13 | 14 | ...  # byte offset
    /// ---------------------------------------------------------------------------------------------------------------
    /// | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | ...  # page must be blank
    /// | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | xx | yy | yy | yy | yy | yy | ...  # page must contain FastSpace record
    /// | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | FF | zz | zz | zz | zz | zz | ...  # page contains an arbitrary number of SpaceUpdate records
    fn fast_space_read(&mut self) {
        use aes::cipher::KeyInit;
        self.syskey_ensure();
        if let Some(system_key) = &self.system_basis_key {
            // remove the old contents, since we're about to re-read an authorative copy from disk.
            self.fspace_cache.clear();
            self.fspace_log_addrs.clear();
            self.fspace_log_next_addr = None;
            self.fspace_log_len = 0;

            // let fscb_slice = self.fscb_deref(); // can't use this line because it causse self to be
            // immutably borrowed, so we write out the equivalent below.
            let fscb_slice = unsafe {
                &self.pddb_mr.as_slice()
                    [self.fscb_phys_base.as_usize()..self.fscb_phys_base.as_usize() + FSCB_PAGES * PAGE_SIZE]
            };

            // 1. scan through the entire space, and look for the FastSpace record. It can be identified by
            //    the
            // first 16 (aes::BLOCK_SIZE) bytes not being all 1's.
            let blank: [u8; aes::BLOCK_SIZE] = [0xff; aes::BLOCK_SIZE];
            let mut fscb_pages = 0;
            let mut blank_pages = Vec::new();
            for page_start in (0..fscb_slice.len()).step_by(PAGE_SIZE) {
                if (fscb_slice[page_start..page_start + aes::BLOCK_SIZE] == blank)
                && (fscb_slice[page_start + aes::BLOCK_SIZE..page_start + aes::BLOCK_SIZE * 2] == blank) {
                //if fscb_slice[page_start..page_start + aes::BLOCK_SIZE].iter().zip(blank.iter()).all(|(&a,&b)| a==b)
                //&& fscb_slice[page_start + aes::BLOCK_SIZE..page_start + aes::BLOCK_SIZE * 2].iter().zip(blank.iter()).all(|(&a,&b)| a==b) {
                    // page has met the criteria for being blank, skip to the next page
                    blank_pages.push(page_start);
                    continue
                } else if fscb_slice[page_start..page_start+aes::BLOCK_SIZE] == blank {
                //} else if fscb_slice[page_start..page_start+aes::BLOCK_SIZE].iter().zip(blank.iter()).all(|(&a,&b)| a==b) {
                    // this page contains update records; stash it for scanning after we've read in the master record
                    self.fspace_log_addrs.push(PageAlignedPa::from(page_start));
                    continue
                } else {
                    // this page (and the ones immediately afterward) "should" contain the FastSpace encrypted record
                    if fscb_pages == 0 {
                        let mut fscb_buf = [0; FASTSPACE_PAGES * PAGE_SIZE - size_of::<Nonce>()];
                        // copy the encrypted data to the decryption buffer
                        for (&src, dst) in
                        fscb_slice[page_start + size_of::<Nonce>() .. page_start + FASTSPACE_PAGES * PAGE_SIZE]
                        .iter().zip(fscb_buf.iter_mut()) {
                            *dst = src;
                        }
                        let mut aad = Vec::<u8>::new();
                        self.fast_space_aad(&mut aad);
                        let payload = Payload {
                            msg: &fscb_buf,
                            aad: &aad,
                        };
                        let cipher = Aes256GcmSiv::new(&system_key.data.into());
                        match cipher.decrypt(Nonce::from_slice(&fscb_slice[page_start..page_start + size_of::<Nonce>()]), payload) {
                            Ok(msg) => {
                                log::info!("decrypted: {}, FastSpace size: {}", msg.len(), size_of::<FastSpace>());
                                assert!(msg.len() == size_of::<FastSpace>());
                                // payload now contains the decrypted data, and the "msg" return field is truncated to the right length
                                // we now map the FastSpace structure onto the payload using an unsafe operation.
                                let fs_ptr = (&msg).as_ptr() as *const FastSpace;
                                let fs_ref: &FastSpace = unsafe{fs_ptr.as_ref().unwrap()};
                                // iterate through the FastSpace disk image and extract the valid and free pages, and note them in the cache
                                for pp in fs_ref.free_pool.iter() {
                                    if pp.valid() && pp.space_state() == SpaceState::Free {
                                        self.fspace_cache.insert(*pp);
                                    }
                                }
                            },
                            Err(e) => {
                                log::warn!("FSCB data was found at page {}, but it did not decrypt correctly. Ignoring FSCB record. Error: {:?}", page_start, e);
                                log::info!("FSCB nonce: {:?}, aad: {:?}, len: {}, msg: {:?}", Nonce::from_slice(&fscb_slice[page_start..page_start + size_of::<Nonce>()]), &aad, fscb_buf.len(), &fscb_buf);
                            }
                        }
                    }
                    fscb_pages += 1;
                }
            }

            // 2. visit the update_page_addrs and modify the fspace_cache accordingly.
            let cipher = Aes256::new(GenericArray::from_slice(&system_key.pt));
            let mut block = Block::default();
            log::info!("space_log_addrs len: {}", self.fspace_log_addrs.len());
            for page in &self.fspace_log_addrs {
                for (index, ct_block) in fscb_slice
                    [page.as_usize() + aes::BLOCK_SIZE..page.as_usize() + PAGE_SIZE]
                    .chunks_exact(aes::BLOCK_SIZE)
                    .enumerate()
                {
                    let mut is_blank = true;
                    for &ct in ct_block {
                        if ct != 0xFF {
                            is_blank = false;
                        }
                    }
                    if is_blank {
                        // end the scan at the first blank block. Note the location.
                        self.fspace_log_next_addr =
                            Some((page.as_usize() + ((1 + index) * aes::BLOCK_SIZE)) as PhysAddr);
                        break;
                    }
                    // now try to decrypt the ciphertext block
                    for (&src, dst) in ct_block.iter().zip(block.iter_mut()) {
                        *dst = src;
                    }
                    cipher.decrypt_block(&mut block);
                    if let Some(pp) = SpaceUpdate::try_into_phys_page(block.as_slice()) {
                        log::debug!("maybe replacing fspace block: {:x?}", pp);
                        // note: pp.valid() isn't the cryptographic check, the cryptographic check of record
                        // validity is in try_into_phys_page()
                        if pp.valid() {
                            // PS: it should always be valid!
                            if let Some(prev_pp) = self.fspace_cache.get(&pp) {
                                if pp.journal() > prev_pp.journal() {
                                    self.fspace_cache.replace(pp);
                                } else if pp.journal() == prev_pp.journal() {
                                    log::error!(
                                        "got two identical journal revisions -- this shouldn't happen\n{:x?} (prev)\n{:x?}(candidate)",
                                        prev_pp,
                                        pp
                                    );
                                    log::error!(
                                        "replacing the previous version with the candidate! wish us luck."
                                    );
                                    self.fspace_cache.replace(pp);
                                }
                            } else {
                                // this happens when the FSCB is flushed, and then a page is deleted.
                                log::debug!(
                                    "Journal entry for a free space page that isn't already in our cache {:?}",
                                    pp
                                );
                                self.fspace_cache.insert(pp);
                            }
                        }
                    } else {
                        log::info!("possibly corrupted FSCB update record: {:x?}", block);
                    }
                }
            }
            // at this point, fspace_cache should contain a collection of either Free or Dirty pages. Both are
            // fair game for being recycled.

            // 3. Check to see if we have a target for our next cache write, if not, make one up.
            if self.fspace_log_next_addr.is_none() {
                if blank_pages.len() != 0 {
                    // pick a random page out of the blank pool (random for wear levelling)
                    let random_index = self.entropy.borrow_mut().get_u32() as usize % blank_pages.len();
                    // set the next log address at an offset of one AES block in from the top.
                    self.fspace_log_next_addr =
                        Some((blank_pages[random_index] + aes::BLOCK_SIZE) as PhysAddr);
                } else {
                    log::warn!(
                        "FSCB has no blank space for new update records. This will cause fast_space_alloc() to fail, which can be remedied with a call to fast_space_generate()."
                    );
                }
            }
        } else {
            panic!("invalid state!");
        }
    }

    /// returns a count of the number of pages in the fspace cache
    pub fn fast_space_len(&self) -> usize { self.fspace_cache.len() }

    /// Normally, the fspace_log_next_addr is just incremented, but when it hits the end of the
    /// page, it's set to None. This function will do a modestly expensive scan of the FSCB area
    /// to try and either find another partially filled page, or a completely empty page.
    pub fn fast_space_ensure_next_log(&mut self) -> bool {
        if self.fspace_log_next_addr.is_some() {
            true
        } else {
            let fscb_slice = self.fscb_deref();
            let blank: [u8; aes::BLOCK_SIZE] = [0xff; aes::BLOCK_SIZE];
            let mut blank_pages = Vec::new();
            for page_start in (0..fscb_slice.len()).step_by(PAGE_SIZE) {
                if (fscb_slice[page_start..page_start + aes::BLOCK_SIZE] == blank)
                    && (fscb_slice[page_start + aes::BLOCK_SIZE..page_start + aes::BLOCK_SIZE * 2] == blank)
                {
                    // page has met the criteria for being blank, skip to the next page
                    blank_pages.push(page_start);
                    continue;
                } else if fscb_slice[page_start..page_start + aes::BLOCK_SIZE] == blank {
                    // this page contains update records; scan it for an empty slot
                    for (index, block) in fscb_slice[page_start + aes::BLOCK_SIZE..page_start + PAGE_SIZE]
                        .chunks_exact(aes::BLOCK_SIZE)
                        .enumerate()
                    {
                        // start with a size check; a failure mode of just iterating is the iterator will
                        // terminate early if the block sizes aren't the same.
                        let mut is_blank = block.len() == blank.len();
                        // now confirm that every item is the same
                        for (&a, &b) in block.iter().zip(blank.iter()) {
                            if a != b {
                                is_blank = false;
                            }
                        }
                        if is_blank {
                            self.fspace_log_next_addr =
                                Some((page_start + (1 + index) * aes::BLOCK_SIZE) as PhysAddr);
                            log::info!("Next FSCB entry: {:x?}", self.fspace_log_next_addr);
                            return true;
                        }
                    }
                } else {
                    // this is probably an encrypted FastSpace page, just skip it
                    continue;
                }
            }
            // if we got to this point, we couldn't find a partially full page. Pull a random page from the
            // blank page pool.
            if blank_pages.len() != 0 {
                // pick a random page out of the blank pool (random for wear levelling)
                let random_index = self.entropy.borrow_mut().get_u32() as usize % blank_pages.len();
                // set the next log address at an offset of one AES block in from the top.
                self.fspace_log_next_addr = Some((blank_pages[random_index] + aes::BLOCK_SIZE) as PhysAddr);
                log::warn!("Moving log to fresh FSCB page: {:x?}", self.fspace_log_next_addr);
                true
            } else {
                false
            }
        }
    }

    pub fn fast_space_has_pages(&self, count: usize) -> bool {
        let mut free_count = 0;
        for pp in self.fspace_cache.iter() {
            if (pp.space_state() == SpaceState::Free || pp.space_state() == SpaceState::Dirty)
                && (pp.journal() < PHYS_PAGE_JOURNAL_MAX)
            {
                free_count += 1;
            } else {
                log::trace!("fastpace other entry: {:?}", pp.space_state());
            }
            if free_count >= count {
                return true;
            }
        }
        false
    }

    /// Attempts to allocate a page out of the fspace cache (in RAM). This is the "normal" route for
    /// allocating pages. This call should be prefixed by a call to ensure_fast_space_alloc() to make sure
    /// it doesn't fail. We do a two-stage "look before you leap" because trying to dynamically redo the
    /// fast space allocation tables in the middle of trying to allocate pages causes a borrow checker
    /// problem -- because you're already using the page maps to figure out you ran out of space, but then
    /// you're trying to mutate them to allocate more space. This can lead to concurrency issues, so we do
    /// a "look before you leap" method instead, where we just check that the correct amount of free space
    /// is available before doing an allocation, and if not, we mutate the map to populate new free space;
    /// and if so, we mutate the map to remove the allocated page.
    pub fn try_fast_space_alloc(&mut self) -> Option<PhysPage> {
        // 1. Confirm that the fspace_log_next_addr is valid. If not, regenerate it, or fail.
        if !self.fast_space_ensure_next_log() {
            log::warn!("Couldn't ensure fast space log entry: {}", self.fspace_log_len);
            None
        } else {
            // 2. find the first page that is Free or Dirty.
            // We made a mistake early on and assumed that the HashSet underlying type would have enough
            // randomness, but unfortunately, it tends to be an MRU list of pages: so, recently
            // de-allocated pages are returned preferentially. This is the opposite of what we
            // want. Thus, we are now paying a bit of a price in computational efficiency to map
            // the HashSet into a Vec and then pick a random entry to get the propreties that we
            // were originally hoping for :-/
            let mut maybe_alloc = None;
            let mut candidates = Vec::<PhysPage>::new();
            for pp in self.fspace_cache.iter() {
                if (pp.space_state() == SpaceState::Free || pp.space_state() == SpaceState::Dirty)
                    && (pp.journal() < PHYS_PAGE_JOURNAL_MAX)
                {
                    candidates.push(pp.clone());
                }
            }
            // now pull a random candidate out of the pool
            if candidates.len() > 0 {
                let mut ppc = candidates[self.entropy.borrow_mut().get_u32() as usize % candidates.len()];
                // take the state directly to Used, skipping MaybeUsed. If the system crashes between now and
                // when the page is actually used, the consequence is a "lost" entry in the FastSpace cache.
                // However, the entry will be reclaimed on the next full-space scan. This is a
                // less-bad outcome than filling up the log with 2x the number of operations
                // to record MaybeUsed and then Used.
                ppc.set_space_state(SpaceState::Used);
                ppc.set_clean(false); // the allocated page is not clean, because it hasn't been written to disk
                ppc.set_valid(true); // the allocated page is now valid, so it should be flushed to disk
                ppc.set_journal(ppc.journal() + 1); // this is guaranteed not to overflow because of a check in the "if" clause above

                // commit the usage to the journal
                self.syskey_ensure();
                let cipher =
                    self.cipher_ecb.as_ref().expect("Inconsistent internal state - syskey_ensure() failed");
                let mut update = SpaceUpdate::new(self.entropy.borrow_mut().get_u64(), ppc);
                let mut block = Block::from_mut_slice(update.deref_mut());
                log::trace!("block: {:x?}", block);
                cipher.encrypt_block(&mut block);
                let log_addr = self.fspace_log_next_addr.take().unwrap() as PhysAddr;
                log::trace!("patch: {:x?}", block);
                self.patch_fscb(&block, log_addr);
                self.fspace_log_len += 1;
                let next_addr = log_addr + aes::BLOCK_SIZE as PhysAddr;
                if (next_addr & (PAGE_SIZE as PhysAddr - 1)) != 0 {
                    self.fspace_log_next_addr = Some(next_addr as PhysAddr);
                } else {
                    // fspace_log_next_addr is already None because we used "take()". We'll find a free spot
                    // for the next journal entry the next time around.
                }
                maybe_alloc = Some(ppc);
            }
            if maybe_alloc.is_none() {
                log::warn!("Ran out of free space. fspace cache has {} entries", self.fspace_cache.len());
                //for entry in self.fspace_cache.iter() {
                //    log::info!("{:?}", entry);
                //}
            }
            if let Some(alloc) = maybe_alloc {
                assert!(
                    self.fspace_cache.remove(&alloc),
                    "inconsistent state: we found a free page, but later when we tried to update it, it wasn't there!"
                );
            }
            maybe_alloc
        }
    }

    pub fn fast_space_free(&mut self, pp: &mut PhysPage) {
        self.fast_space_ensure_next_log();
        if !self.fspace_cache.remove(&pp) {
            log::warn!("Freeing a page that's not already in cache: {:x?}", pp);
        }
        // update the fspace cache
        log::debug!("fast_space_free pp incoming: {:x?}", pp);
        pp.set_space_state(SpaceState::Dirty);
        pp.set_journal(pp.journal() + 1);

        // re-cycle the space into the fspace_cache
        self.fspace_cache.insert(pp.clone());

        // commit the free'd block to the journal
        self.syskey_ensure();
        let cipher = self.cipher_ecb.as_ref().expect("Inconsistent internal state - syskey_ensure() failed");
        let mut update = SpaceUpdate::new(self.entropy.borrow_mut().get_u64(), pp.clone());
        let mut block = Block::from_mut_slice(update.deref_mut());
        log::trace!("block: {:x?}", block);
        cipher.encrypt_block(&mut block);
        let log_addr = self.fspace_log_next_addr.take().unwrap() as PhysAddr;
        log::trace!("patch: {:x?}", block);
        self.patch_fscb(&block, log_addr);
        self.fspace_log_len += 1;
        let next_addr = log_addr + aes::BLOCK_SIZE as PhysAddr;
        if (next_addr & (PAGE_SIZE as PhysAddr - 1)) != 0 {
            self.fspace_log_next_addr = Some(next_addr as PhysAddr);
        } else {
            // fspace_log_next_addr is already None because we used "take()". We'll find a free spot for the
            // next journal entry the next time around.
        }
        // mark the page as invalid, so that it will be deleted on the next PT sync
        pp.set_valid(false);
    }

    /// This is a "look before you leap" function that will potentially pause all system operations
    /// and do a deep scan for space if the required amount is not available.
    pub fn ensure_fast_space_alloc(&mut self, pages: usize, cache: &Vec<BasisCacheEntry>) -> bool {
        const BUFFER: usize = 1; // a bit of slop in the trigger point
        let has_pages = self.fast_space_has_pages(pages + BUFFER);
        log::trace!(
            "alloc fast_space_len: {}, log_len {}, has {} pages: {}",
            self.fast_space_len(),
            self.fspace_log_len,
            pages + BUFFER,
            has_pages
        );
        // make sure we have fast space pages...
        if has_pages
        // ..and make sure we have space for fast space log entries
        && (self.fspace_log_len < (FSCB_PAGES - FASTSPACE_PAGES - 1) * PAGE_SIZE / aes::BLOCK_SIZE)
        {
            true
        } else {
            if !has_pages {
                log::warn!("FastSpace alloc forced by lack of free space");
                // if we're really out of space, do an expensive full-space sweep
                if let Some(used_pages) = self.pddb_generate_used_map(cache) {
                    let free_pool = self.fast_space_generate(used_pages);
                    if free_pool.len() == 0 {
                        // we're out of free space
                        false
                    } else {
                        let mut fast_space = FastSpace { free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN] };
                        for pp in fast_space.free_pool.iter_mut() {
                            pp.set_journal(self.trng_u8() % FSCB_JOURNAL_RAND_RANGE)
                        }
                        for (&src, dst) in free_pool.iter().zip(fast_space.free_pool.iter_mut()) {
                            *dst = src;
                        }
                        // write just commits a new record to disk, but doesn't update our internal data cache
                        // this also clears the fast space log.
                        self.fast_space_write(&fast_space);
                        // this will ensure the data cache is fully in sync
                        self.fast_space_read();

                        // check that we have enough space now -- if not, we're just out of disk space!
                        if self.fast_space_len() > pages { true } else { false }
                    }
                } else {
                    false
                }
            } else {
                // log regenration is faster & less intrusive than fastspace regeneration, and we would have
                // to do this more often. So we have a separate path for this outcome.
                log::warn!("FastSpace alloc forced by lack of log space");
                self.fast_space_flush();
                true
            }
        }
    }

    /// This is a "fast" flush that expires all the PDDB SpaceUpdate journal
    pub(crate) fn fast_space_flush(&mut self) {
        let mut fast_space = FastSpace { free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN] };
        for pp in fast_space.free_pool.iter_mut() {
            pp.set_journal(self.trng_u8() % FSCB_JOURNAL_RAND_RANGE)
        }
        // regenerate from the existing fast space cache
        for (&src, dst) in self.fspace_cache.iter().zip(fast_space.free_pool.iter_mut()) {
            *dst = src;
        }
        log::info!("SpaceUpdate flush with {} pages", fast_space.free_pool.len());
        let start = self.timestamp_now();
        // write just commits a new record to disk, but doesn't update our internal data cache
        // this also clears the fast space log.
        self.fast_space_write(&fast_space);
        // this will re-read back in the data, shuffle the alloc order a bit, and ensure the data cache is
        // fully in sync
        self.fast_space_read();
        // this will locate the next fast space log point.
        self.fast_space_ensure_next_log();
        log::info!("Flush took {}ms", self.timestamp_now() - start);
    }

    pub(crate) fn data_aad(&self, name: &str) -> Vec<u8> {
        let mut aad = Vec::<u8>::new();
        aad.extend_from_slice(&name.as_bytes());
        aad.extend_from_slice(&PDDB_VERSION.to_le_bytes());
        match self.dna_mode {
            DnaMode::Normal | DnaMode::Churn => aad.extend_from_slice(&self.dna.to_le_bytes()),
            DnaMode::Migration => aad.extend_from_slice(&self.migration_dna.to_le_bytes()),
        }
        aad
    }

    /// returns a decrypted page that still includes the journal number at the very beginning
    /// We don't clip it off because it would require re-allocating a vector, and it's cheaper (although less
    /// elegant) to later just index past it.
    pub(crate) fn data_decrypt_page(
        &self,
        cipher: &Aes256GcmSiv,
        aad: &[u8],
        page: &PhysPage,
    ) -> Option<Vec<u8>> {
        let ct_slice = unsafe {
            &self.pddb_mr.as_slice()[self.data_phys_base.as_usize() + page.page_number() as usize * PAGE_SIZE
                ..self.data_phys_base.as_usize() + (page.page_number() as usize + 1) * PAGE_SIZE]
        };
        let nonce = &ct_slice[..size_of::<Nonce>()];
        let ct = &ct_slice[size_of::<Nonce>()..];
        match cipher.decrypt(Nonce::from_slice(nonce), Payload { aad, msg: ct }) {
            Ok(data) => {
                assert!(
                    data.len() == VPAGE_SIZE + size_of::<JournalType>(),
                    "authentication successful, but wrong amount of data was recovered"
                );
                Some(data)
            }
            Err(e) => {
                log::trace!("Error decrypting page: {:?}", e); // sometimes this is totally "normal", like when we're testing for valid data.
                None
            }
        }
    }

    /// returns a decrypted page that also encodes a key commitments. In this case, a raw key is passed,
    /// instead of the generic AES-GCM-SIV cipher, because we need to derive the key commitment.
    /// Key commitments are a patch to work-around the salamander problem in AES-GCM-SIV see https://eprint.iacr.org/2020/1456.pdf
    ///
    /// The structure of a page with commit key storage is as follows:
    /// - Nonce - 12 bytes
    /// - ciphertext - 4004 bytes (includes the journal number)
    ///   - kcomm_nonce - 32 bytes
    ///   - kcomm - 32 bytes
    /// - MAC - 16 bytes
    /// We stripe the MAC at the end just in case the MAC has some arithmetic property that can betray the
    /// existence of a basis root record with key commitment. The committed key and the nonce both should
    /// be indistinguishable from ciphertext.
    pub(crate) fn data_decrypt_page_with_commit(
        &self,
        key: &[u8],
        aad: &[u8],
        page: &PhysPage,
    ) -> Option<Vec<u8>> {
        use aes::cipher::KeyInit;
        const KCOM_NONCE_LEN: usize = 32;
        const KCOM_LEN: usize = 32;
        const MAC_LEN: usize = 16;
        let ct_slice = unsafe {
            &self.pddb_mr.as_slice()[self.data_phys_base.as_usize() + page.page_number() as usize * PAGE_SIZE
                ..self.data_phys_base.as_usize() + (page.page_number() as usize + 1) * PAGE_SIZE]
        };
        log::debug!(
            "commit data at 0x{:x}",
            self.data_phys_base.as_usize() + page.page_number() as usize * PAGE_SIZE
        );
        let nonce = &ct_slice[..size_of::<Nonce>()];
        let ct_total = &ct_slice[size_of::<Nonce>()..];

        // extract the regions of the stored data and place them into their respective buffers
        let mut ct_plus_mac = [0u8; KCOM_CT_LEN + MAC_LEN];
        let mut nonce_comm = [0u8; KCOM_NONCE_LEN];
        let mut key_comm_stored = [0u8; KCOM_LEN];
        let mut ct_pos = 0;

        for (&src, dst) in ct_total[ct_pos..].iter().zip(ct_plus_mac[..KCOM_CT_LEN].iter_mut()) {
            *dst = src;
            ct_pos += 1;
        }
        for (&src, dst) in ct_total[ct_pos..].iter().zip(nonce_comm.iter_mut()) {
            *dst = src;
            ct_pos += 1;
        }
        for (&src, dst) in ct_total[ct_pos..].iter().zip(key_comm_stored.iter_mut()) {
            *dst = src;
            ct_pos += 1;
        }
        for (&src, dst) in ct_total[ct_pos..].iter().zip(ct_plus_mac[KCOM_CT_LEN..].iter_mut()) {
            *dst = src;
            ct_pos += 1;
        }
        assert!(
            ct_pos == PAGE_SIZE - size_of::<Nonce>(),
            "struct sizing error in unpacking page with key commit"
        );
        log::debug!("found nonce of {:x?}", nonce);
        log::debug!("found kcom_nonce of {:x?}", nonce_comm);

        let (kenc, kcom) = self.kcom_func(key.try_into().unwrap(), &nonce_comm);
        let cipher = Aes256GcmSiv::new(&kenc.into());

        // Attempt decryption. This is None on failure
        let plaintext = cipher.decrypt(Nonce::from_slice(nonce), Payload { aad, msg: &ct_plus_mac }).ok();

        // Only return the plaintext if the stored key commitment agrees with the computed one
        if kcom.ct_eq(&key_comm_stored).into() { plaintext } else { None }
    }

    /// `data` includes the journal entry on top. The data passed in must be exactly one vpage plus the
    /// journal entry
    pub(crate) fn data_encrypt_and_patch_page(
        &self,
        cipher: &Aes256GcmSiv,
        aad: &[u8],
        data: &mut [u8],
        pp: &PhysPage,
    ) {
        assert!(
            data.len() == VPAGE_SIZE + size_of::<JournalType>(),
            "did not get a page-sized region to patch"
        );
        let j = JournalType::from_le_bytes(data[..size_of::<JournalType>()].try_into().unwrap())
            .saturating_add(1);
        for (&src, dst) in j.to_le_bytes().iter().zip(data[..size_of::<JournalType>()].iter_mut()) {
            *dst = src;
        }
        let nonce = self.nonce_gen();
        let ciphertext = cipher.encrypt(&nonce, Payload { aad, msg: &data }).expect("couldn't encrypt data");
        // log::trace!("calling patch. nonce {:x?}, ct {:x?}, data {:x?}", nonce.as_slice(),
        // &ciphertext[..32], &data[..32]);
        self.patch_data(&[nonce.as_slice(), &ciphertext].concat(), pp.page_number() * PAGE_SIZE as u32);
    }

    /// `data` includes the journal entry on top.
    /// The data passed in must be exactly one vpage plus the journal entry minus the length of the commit
    /// structure (64 bytes), which is 4004 bytes total
    /// This function increments the journal and re-nonces the structure.
    pub(crate) fn data_encrypt_and_patch_page_with_commit(
        &self,
        key: &[u8],
        aad: &[u8],
        data: &mut [u8],
        pp: &PhysPage,
    ) {
        use aes::cipher::KeyInit;
        assert!(data.len() == KCOM_CT_LEN, "did not get a key-commit sized region to patch");
        // updates the journal type
        let j = JournalType::from_le_bytes(data[..size_of::<JournalType>()].try_into().unwrap())
            .saturating_add(1);
        data[..size_of::<JournalType>()].copy_from_slice(&j.to_le_bytes());
        // gets the AES-GCM-SIV nonce
        let nonce = self.nonce_gen();
        // makes a nonce for the key commit
        let mut kcom_nonce = [0u8; 32];
        self.trng_slice(&mut kcom_nonce);
        // generates the encryption and commit keys
        let (kenc, kcom) = self.kcom_func(key.try_into().unwrap(), &kcom_nonce);
        let cipher = Aes256GcmSiv::new(&kenc.into());
        let ciphertext = cipher.encrypt(&nonce, Payload { aad, msg: &data }).expect("couldn't encrypt data");
        let mut dest_page = [0u8; PAGE_SIZE];

        let mut written = 0; // used as a sanity check on the insane iterator chain constructed below
        for (&src, dst) in nonce
            .as_slice()
            .iter()
            .chain(ciphertext[..KCOM_CT_LEN].iter())
            .chain(kcom_nonce.iter())
            .chain(kcom.iter())
            .chain(ciphertext[KCOM_CT_LEN..].iter())
            .zip(dest_page.iter_mut())
        {
            *dst = src;
            written += 1;
        }
        assert!(written == 4096, "data sizing error in encryption with key commit");
        log::trace!("nonce: {:x?}", &nonce);
        log::debug!("dest_page[kcom_nonce]: {:x?}", &dest_page[12 + 4004..12 + 4004 + 32]);
        self.patch_data(&dest_page, pp.page_number() * PAGE_SIZE as u32);
    }

    /// Derive a key commitment. This takes in a base `key`, which is 256 bits;
    /// a `nonce` which is the 96-bit nonce used in the AES-GCM-SIV for a given block;
    /// and `nonce_com` which is the commitment nonce, set at 256 bits.
    /// The result is two tuples, (kenc, kcom).
    fn kcom_func(&self, key: &[u8; 32], nonce_com: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
        let mut h_enc = Sha512_256Sw::new();
        h_enc.update(key);
        // per https://eprint.iacr.org/2020/1456.pdf Table 4 on page 13 Type I Lenc
        h_enc.update([0x43, 0x6f, 0x6, 0xd6, 0xd, 0x69, 0x74, 0x01, 0x01]);
        h_enc.update(nonce_com);
        let k_enc = h_enc.finalize();

        let mut h_com = Sha512_256Sw::new();
        h_com.update(key);
        // per https://eprint.iacr.org/2020/1456.pdf Table 4 on page 13 Type I Lcom. Note one-bit difference in last byte.
        h_com.update([0x43, 0x6f, 0x6, 0xd6, 0xd, 0x69, 0x74, 0x01, 0x02]);
        h_com.update(nonce_com);
        let k_com = h_com.finalize();
        (k_enc.into(), k_com.into())
    }

    /// Meant to be called on boot. This will read the FastSpace record, and then attempt to load
    /// in the system basis.
    pub(crate) fn pddb_mount(&mut self) -> Option<BasisCacheEntry> {
        self.fast_space_read();
        self.syskey_ensure();
        if let Some(syskey) = self.system_basis_key.take() {
            if let Some(sysbasis_map) = self.pt_scan_key(&syskey.pt, &syskey.data, PDDB_DEFAULT_SYSTEM_BASIS)
            {
                let aad = self.data_aad(PDDB_DEFAULT_SYSTEM_BASIS);
                // get the first page, where the basis root is guaranteed to be
                if let Some(root_page) = sysbasis_map.get(&VirtAddr::new(VPAGE_SIZE as u64).unwrap()) {
                    let vpage = match self.data_decrypt_page_with_commit(&syskey.data, &aad, root_page) {
                        Some(data) => data,
                        None => {
                            log::error!("System basis decryption did not authenticate. Unrecoverable error.");
                            return None;
                        }
                    };
                    // if the below assertion fails, you will need to re-code this to decrypt more than one
                    // VPAGE and stripe into a basis root struct
                    assert!(
                        size_of::<BasisRoot>() <= VPAGE_SIZE,
                        "BasisRoot has grown past a single VPAGE, this routine needs to be re-coded to accommodate the extra bulk"
                    );
                    let mut basis_root = BasisRoot::default();
                    for (&src, dst) in
                        vpage[size_of::<JournalType>()..].iter().zip(basis_root.deref_mut().iter_mut())
                    {
                        *dst = src;
                    }
                    if basis_root.magic != PDDB_MAGIC {
                        log::error!("Basis root did not deserialize correctly, unrecoverable error.");
                        return None;
                    }
                    if basis_root.version != PDDB_VERSION {
                        log::error!("PDDB version mismatch in system basis root. Unrecoverable error.");
                        return None;
                    }
                    let basis_name =
                        std::str::from_utf8(&basis_root.name.data[..basis_root.name.len as usize])
                            .expect("basis is not valid utf-8");
                    if basis_name != PDDB_DEFAULT_SYSTEM_BASIS {
                        log::error!(
                            "PDDB system basis name is incorrect: {}; aborting mount operation.",
                            basis_name
                        );
                        return None;
                    }
                    log::info!("System BasisRoot record found, generating cache entry");
                    let bce = BasisCacheEntry::mount(
                        self,
                        &basis_name,
                        &syskey,
                        false,
                        BasisRetentionPolicy::Persist,
                    );
                    self.system_basis_key = Some(syskey);
                    bce
                } else {
                    // i guess technically we could try a brute-force search for the page, but meh.
                    log::error!("System basis did not contain a root page -- unrecoverable error.");
                    self.system_basis_key = Some(syskey);
                    None
                }
            } else {
                self.system_basis_key = Some(syskey);
                None
            }
        } else {
            None
        }
    }

    #[cfg(feature = "gen1")]
    fn pw_check(&self, modals: &modals::Modals) -> Result<()> {
        let mut success = false;
        while !success {
            // the "same password check" is accomplished by just encrypting the all-zeros block twice
            // with the cipher after clearing the password and re-entering it, and then comparing that
            // the results are identical. The test blocks are never committed or stored anywhere.
            // The actual creation of the "real" key material is done in step 3.
            let mut checkblock_a = [0u8; BLOCK_SIZE];
            self.rootkeys.decrypt_block(GenericArray::from_mut_slice(&mut checkblock_a));

            log::info!("{}PDDB.CHECKPASS,{}", xous::BOOKEND_START, xous::BOOKEND_END);
            #[cfg(any(feature = "precursor", feature = "renode"))] // skip this dialog in hosted mode
            modals.show_notification(t!("pddb.checkpass", locales::LANG), None).expect("notification failed");

            self.clear_password();
            let mut checkblock_b = [0u8; BLOCK_SIZE];
            self.rootkeys.decrypt_block(GenericArray::from_mut_slice(&mut checkblock_b));

            if checkblock_a == checkblock_b {
                success = true;
            } else {
                log::info!("{}PDDB.PWFAIL,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                modals
                    .show_notification(t!("pddb.checkpass_fail", locales::LANG), None)
                    .expect("notification failed");
                self.clear_password();
            }
        }
        if success { Ok(()) } else { Err(Error::new(ErrorKind::PermissionDenied, "Password entry failed")) }
    }

    /// this function attempts to change the PIN. returns Ok() if changed, error if not.
    #[cfg(feature = "gen1")]
    pub(crate) fn pddb_change_pin(&mut self, modals: &modals::Modals) -> Result<()> {
        if let Some(system_keys) = &self.system_basis_key {
            // get the new password
            self.clear_password();
            modals
                .show_notification(t!("pddb.changepin.enter_new_pin", locales::LANG), None)
                .map_err(|_| Error::new(ErrorKind::Other, "Internal error"))?;
            self.pw_check(modals)?;

            // wrap keys in the new password, and store them to disk
            let wrapped_key = self
                .rootkeys
                .wrap_key(&system_keys.data)
                .expect("Internal error wrapping our encryption key");
            let wrapped_key_pt =
                self.rootkeys.wrap_key(&system_keys.pt).expect("Internal error wrapping our encryption key");
            let mut crypto_keys = StaticCryptoData::default();
            crypto_keys.deref_mut().copy_from_slice(&self.static_crypto_data_get().deref());
            assert!(wrapped_key_pt.len() == 40);
            assert!(wrapped_key.len() == 40);
            crypto_keys.system_key_pt.copy_from_slice(&wrapped_key_pt);
            crypto_keys.system_key.copy_from_slice(&wrapped_key);
            self.patch_keys(crypto_keys.deref(), 0);
            Ok(())
        } else {
            Err(Error::new(ErrorKind::PermissionDenied, "System basis keys not unlocked"))
        }
    }

    /// this function is dangerous in that calling it will completely erase all of the previous data
    /// in the PDDB an replace it with a brand-spanking new, blank PDDB.
    /// The number of servers that can connect to the Spinor crate is strictly tracked, so we borrow a
    /// reference to the Spinor object allocated to the PDDB implementation for this operation.
    pub(crate) fn pddb_format(&mut self, fast: bool, progress: Option<&modals::Modals>) -> Result<()> {
        use aes::cipher::KeyInit;
        if !self.rootkeys.is_initialized().unwrap() {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "Root keys are not initialized; cannot format a PDDB without root keys!",
            ));
        }
        // step 0. If we have a modal, confirm that the password entered was correct with a double-entry.
        if let Some(modals) = progress {
            self.pw_check(modals)?;
        }

        // step 1. Erase the entire PDDB region - leaves the state in all 1's
        if !fast {
            log::info!("Erasing the PDDB region");
            if let Some(modals) = progress {
                modals
                    .start_progress(
                        t!("pddb.erase", locales::LANG),
                        xous::PDDB_LOC,
                        xous::PDDB_LOC + PDDB_A_LEN as u32,
                        xous::PDDB_LOC,
                    )
                    .expect("couldn't raise progress bar");
                // retain this delay, because the next section is so compute-intensive, it may take a
                // while for the GAM to catch up.
                self.tt.sleep_ms(100).unwrap();
            }
            for offset in (xous::PDDB_LOC..(xous::PDDB_LOC + PDDB_A_LEN as u32))
                .step_by(SPINOR_BULK_ERASE_SIZE as usize)
            {
                if (offset / SPINOR_BULK_ERASE_SIZE) % 4 == 0 {
                    log::info!("Initial erase: {}/{}", offset - xous::PDDB_LOC, PDDB_A_LEN as u32);
                    if let Some(modals) = progress {
                        modals.update_progress(offset as u32).expect("couldn't update progress bar");
                    }
                }
                // do a blank check first to see if the sector really needs erasing
                let mut blank = true;
                let slice_start = (offset - xous::PDDB_LOC) as usize / size_of::<u32>();
                for word in unsafe {
                    self.pddb_mr.as_slice::<u32>()
                        [slice_start..slice_start + SPINOR_BULK_ERASE_SIZE as usize / size_of::<u32>()]
                        .iter()
                } {
                    if *word != 0xFFFF_FFFF {
                        blank = false;
                        break;
                    }
                }
                if !blank {
                    self.spinor.bulk_erase(offset, SPINOR_BULK_ERASE_SIZE).expect("couldn't erase memory");
                }
            }
            if let Some(modals) = progress {
                modals
                    .update_progress(xous::PDDB_LOC + PDDB_A_LEN as u32)
                    .expect("couldn't update progress bar");
                modals.finish_progress().expect("couldn't dismiss progress bar");
                #[cfg(feature = "ux-swap-delay")]
                self.tt.sleep_ms(100).unwrap();
            }
        }

        // step 2. fill in the page table with junk, which marks it as cryptographically empty
        if let Some(modals) = progress {
            modals
                .start_progress(t!("pddb.initpt", locales::LANG), 0, size_of::<PageTableInFlash>() as u32, 0)
                .expect("couldn't raise progress bar");
        }
        let mut temp: [u8; PAGE_SIZE] = [0; PAGE_SIZE];
        for page in (0..(size_of::<PageTableInFlash>() & !(PAGE_SIZE - 1))).step_by(PAGE_SIZE) {
            self.entropy.borrow_mut().get_slice(&mut temp);
            self.patch_pagetable_raw(&temp, page as u32);
            if let Some(modals) = progress {
                if (page / PAGE_SIZE) % 16 == 0 {
                    modals.update_progress(page as u32).expect("couldn't update progress bar");
                }
            }
        }
        if let Some(modals) = progress {
            modals
                .update_progress(size_of::<PageTableInFlash>() as u32)
                .expect("couldn't update progress bar");
        }
        if size_of::<PageTableInFlash>() & (PAGE_SIZE - 1) != 0 {
            let remainder_start = size_of::<PageTableInFlash>() & !(PAGE_SIZE - 1);
            log::info!(
                "Page table does not end on a page boundary. Handling trailing page case of {} bytes",
                size_of::<PageTableInFlash>() - remainder_start
            );
            let mut temp = Vec::<u8>::new();
            for _ in remainder_start..size_of::<PageTableInFlash>() {
                temp.push(self.entropy.borrow_mut().get_u8());
            }
            self.patch_pagetable_raw(&temp, remainder_start as u32);
        }
        if let Some(modals) = progress {
            modals.finish_progress().expect("couldn't dismiss progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }

        // step 3. create our key material
        // consider: making ensure_aes_password() a pub-scoped function? let's see how this works in practice.
        //if !self.rootkeys.ensure_aes_password() {
        //    return Err(Error::new(ErrorKind::PermissionDenied, "unlock password was incorrect"));
        //}
        if let Some(modals) = progress {
            modals
                .start_progress(t!("pddb.key", locales::LANG), 0, 100, 0)
                .expect("couldn't raise progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }
        assert!(
            size_of::<StaticCryptoData>() == PAGE_SIZE,
            "StaticCryptoData structure is not correctly sized"
        );
        let mut system_basis_key_pt: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
        let mut system_basis_key: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
        self.entropy.borrow_mut().get_slice(&mut system_basis_key_pt);
        self.entropy.borrow_mut().get_slice(&mut system_basis_key);
        // build the ECB cipher for page table entries
        self.cipher_ecb = Some(Aes256::new(GenericArray::from_slice(&system_basis_key_pt)));
        let cipher_ecb = Aes256::new(GenericArray::from_slice(&system_basis_key_pt)); // a second copy for patching the page table later in this routine interior mutability blah blah work around oops
        // now wrap the key for storage
        let wrapped_key =
            self.rootkeys.wrap_key(&system_basis_key).expect("Internal error wrapping our encryption key");
        let wrapped_key_pt =
            self.rootkeys.wrap_key(&system_basis_key_pt).expect("Internal error wrapping our encryption key");
        self.system_basis_key = Some(BasisKeys { pt: system_basis_key_pt, data: system_basis_key }); // this causes system_basis_key to be owned by self and go out of scope
        let mut crypto_keys = StaticCryptoData::default();
        crypto_keys.version = SCD_VERSION; // should already be set by `default()` but let's be sure.
        if let Some(modals) = progress {
            modals.update_progress(50).expect("couldn't update progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }
        // copy the encrypted key into the data structure for commit to Flash
        // the wrapped key should have a well-defined length of 40 bytes
        assert!(wrapped_key.len() == 40, "wrapped key did not have the expected length");
        for (&src, dst) in wrapped_key.iter().zip(crypto_keys.system_key.iter_mut()) {
            *dst = src;
        }
        assert!(wrapped_key_pt.len() == 40, "wrapped pt key did not have the expected length");
        for (&src, dst) in wrapped_key_pt.iter().zip(crypto_keys.system_key_pt.iter_mut()) {
            *dst = src;
        }
        // initialize the salt
        self.entropy.borrow_mut().get_slice(&mut crypto_keys.salt_base);
        // commit keys
        self.patch_keys(crypto_keys.deref(), 0);
        if let Some(modals) = progress {
            modals.update_progress(100).expect("couldn't update progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
            modals.finish_progress().expect("couldn't dismiss progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }
        // now we have a copy of the AES key necessary to encrypt the default System basis that we created in
        // step 2.

        #[cfg(not(target_os = "xous"))]
        self.tt.sleep_ms(500).unwrap(); // delay for UX to catch up in emulation

        // step 4. mbbb handling
        // mbbb should just be blank at this point, and the flash was erased in step 1, so there's nothing to
        // do.

        // step 5. fscb handling
        // pick a set of random pages from the free pool and assign it to the fscb
        // pass the generator an empty cache - this causes it to treat the entire disk as free space
        if let Some(modals) = progress {
            modals
                .start_progress(t!("pddb.fastspace", locales::LANG), 0, 100, 0)
                .expect("couldn't raise progress bar");
            self.tt.sleep_ms(100).unwrap();
        }
        let free_pool = self.fast_space_generate(BinaryHeap::<Reverse<u32>>::new());
        let mut fast_space = FastSpace { free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN] };
        for (&src, dst) in free_pool.iter().zip(fast_space.free_pool.iter_mut()) {
            *dst = src;
        }
        if let Some(modals) = progress {
            modals.update_progress(50).expect("couldn't update progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }
        self.fast_space_write(&fast_space);
        if let Some(modals) = progress {
            modals.update_progress(100).expect("couldn't update progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
            modals.finish_progress().expect("couldn't dismiss progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }

        #[cfg(not(target_os = "xous"))]
        self.tt.sleep_ms(500).unwrap();

        // step 5. salt the free space with random numbers. this can take a while, we might need a "progress
        // report" of some kind... this is coded using "direct disk" offsets...under the assumption
        // that we only ever really want to do this here, and not re-use this routine elsewhere.
        if let Some(modals) = progress {
            modals
                .start_progress(
                    t!("pddb.randomize", locales::LANG),
                    self.data_phys_base.as_u32(),
                    PDDB_A_LEN as u32,
                    self.data_phys_base.as_u32(),
                )
                .expect("couldn't raise progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }
        let blank = [0xffu8; aes::BLOCK_SIZE];
        for offset in (self.data_phys_base.as_usize()..PDDB_A_LEN).step_by(PAGE_SIZE) {
            if fast {
                // we could "skip" pages already encrypted to an old key as a short cut -- because we nuked
                // our session key, previous data should be undecipherable. You shouldn't do
                // this for a production erase but this is good for speeding up testing.
                let mut is_blank = true;
                let block: &[u8] = unsafe {
                    &self.pddb_mr.as_slice()[offset + aes::BLOCK_SIZE * 3..offset + aes::BLOCK_SIZE * 4]
                };
                for (&a, &b) in block.iter().zip(blank.iter()) {
                    if a != b {
                        is_blank = false;
                        break;
                    }
                }
                if !is_blank {
                    if (offset / PAGE_SIZE) % 16 == 0 {
                        log::info!(
                            "Page at {} is likely to already have cryptographic data, skipping...",
                            offset
                        );
                    }
                    continue;
                }
            }
            self.entropy.borrow_mut().get_slice(&mut temp);
            if (offset / PAGE_SIZE) % 256 == 0 {
                // ~one update per megabyte
                log::info!("Cryptographic 'erase': {}/{}", offset, PDDB_A_LEN);
                if let Some(modals) = progress {
                    modals.update_progress(offset as u32).expect("couldn't update progress bar");
                }
            }
            self.spinor
                .patch(unsafe { self.pddb_mr.as_slice() }, xous::PDDB_LOC, &temp, offset as u32)
                .expect("couldn't fill in disk with random datax");
        }
        if let Some(modals) = progress {
            modals.update_progress(PDDB_A_LEN as u32).expect("couldn't update progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
            modals.finish_progress().expect("couldn't dismiss progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }

        // step 6. create the system basis root structure
        if let Some(modals) = progress {
            modals
                .start_progress(t!("pddb.structure", locales::LANG), 0, 100, 0)
                .expect("couldn't raise progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }
        let basis_root = BasisRoot {
            magic: api::PDDB_MAGIC,
            version: api::PDDB_VERSION,
            name: BasisRootName::try_from_str(PDDB_DEFAULT_SYSTEM_BASIS).unwrap(),
            age: 0,
            num_dictionaries: 0,
        };

        // step 7. Create a hashmap for our reverse PTE, allocate sectors, and add it to the Pddb's cache
        self.fast_space_read(); // we reconstitute our fspace map even though it was just generated, partially as a sanity check that everything is ok
        if let Some(modals) = progress {
            modals.update_progress(33).expect("couldn't update progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }

        let mut basis_v2p_map = HashMap::<VirtAddr, PhysPage>::new();
        // allocate one page for the basis root
        if let Some(alloc) = self.try_fast_space_alloc() {
            let mut rpte = alloc.clone();
            rpte.set_clean(true); // it's not clean _right now_ but it will be by the time this routine is done...
            rpte.set_valid(true);
            rpte.set_space_state(SpaceState::Used);
            let va = VirtAddr::new((1 * VPAGE_SIZE) as u64).unwrap(); // page 1 is where the root goes, by definition
            log::info!("adding basis va {:x?} with pte {:?}", va, rpte);
            basis_v2p_map.insert(va, rpte);
        }

        // step 8. write the System basis to Flash, at the physical locations noted above. This is an extract
        // from the basis_sync() method on a BasisCache entry, but because we haven't created a cache entry,
        // we're copypasta'ing the code here
        let aad = basis_root.aad(self.dna);
        let pp = basis_v2p_map
            .get(&VirtAddr::new(1 * VPAGE_SIZE as u64).unwrap())
            .expect("Internal consistency error: Basis exists, but its root map was not allocated!");
        assert!(pp.valid(), "v2p returned an invalid page");
        let journal_bytes = (self.trng_u32() % JOURNAL_RAND_RANGE).to_le_bytes();
        let slice_iter = journal_bytes.iter() // journal rev
            .chain(basis_root.as_ref().iter());
        let mut block = [0 as u8; KCOM_CT_LEN];
        for (&src, dst) in slice_iter.zip(block.iter_mut()) {
            *dst = src;
        }
        let syskey = self.system_basis_key.take().unwrap(); // take the key out
        self.data_encrypt_and_patch_page_with_commit(&syskey.data, &aad, &mut block, &pp);
        self.system_basis_key = Some(syskey); // put the key back
        if let Some(modals) = progress {
            modals.update_progress(66).expect("couldn't update progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }

        // step 9. generate & write initial page table entries
        for (&virt, phys) in basis_v2p_map.iter_mut() {
            self.pt_patch_mapping(virt, phys.page_number(), &cipher_ecb);
            // mark the entry as clean, as it has been sync'd to disk
            phys.set_clean(true);
        }
        if let Some(modals) = progress {
            modals.update_progress(100).expect("couldn't update progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
            modals.finish_progress().expect("couldn't dismiss progress bar");
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(100).unwrap();
        }
        Ok(())
    }

    /// This function will prompt the user to unlock all the Basis. If the user asserts all
    /// Basis have been unlocked, the function returns `true`. The other option is the user
    /// can decline to unlock all the Basis right now, cancelling out of the process, which will
    /// cause the requesting free space sweep to fail.
    pub(crate) fn pddb_generate_used_map(
        &self,
        cache: &Vec<BasisCacheEntry>,
    ) -> Option<BinaryHeap<Reverse<u32>>> {
        if let Some(all_keys) = self.pddb_get_all_keys(&cache) {
            let mut page_heap = BinaryHeap::new();
            let mut page_check = std::collections::HashSet::new(); // this is just for sanity checking because you can't query a heap
            for (basis_keys, name) in all_keys {
                // scan the disclosed bases
                if let Some(map) = self.pt_scan_key(&basis_keys.pt, &basis_keys.data, &name) {
                    for pp in map.values() {
                        page_heap.push(Reverse(pp.page_number()));
                        if !page_check.insert(pp.page_number()) {
                            log::warn!("double-insert detected of page number {}", pp.page_number());
                        }
                    }
                } else {
                    log::warn!("pt_scan for basis {} failed, data may be lost", name);
                }
            }

            // need to incorporate all of the FSCB knowledge into the scan as well, because there could be
            // cached pages inside "live" Basis that have not been committed to disk. However, it *should* be
            // the case that any cached allocations came out of the previous FSCB pool, therefore, by merging
            // in the "used list" from the existing FSCB we should be able to generate an accurate
            // representation of the actually used pages
            for pp in self.fspace_cache.iter() {
                if pp.space_state() == SpaceState::Used || pp.space_state() == SpaceState::MaybeUsed {
                    if !page_check.insert(pp.page_number()) {
                        log::debug!(
                            "FSCB and page table both record this used page: {} (this is normal)",
                            pp.page_number()
                        );
                    } else {
                        page_heap.push(Reverse(pp.page_number()));
                        log::info!(
                            "FSCB contained {}, but not yet committed to disk; added to page_heap",
                            pp.page_number()
                        );
                    }
                }
            }

            Some(page_heap)
        } else {
            None
        }
    }

    /// pddb_rekey() must be called from a state where all the data has been synchronized to disk.
    ///
    /// If there are items allocated in the FSCB that have not had their corresponding physical
    /// page table entries on disk written, the operation will fail.
    pub(crate) fn pddb_rekey(&mut self, op: PddbRekeyOp, cache: &Vec<BasisCacheEntry>) -> PddbRekeyOp {
        use aes::cipher::KeyInit;
        match op {
            PddbRekeyOp::FromDnaFast(dna) | PddbRekeyOp::FromDnaSafe(dna) => {
                if cache.len() != 0 {
                    log::error!("We can't have any previously mounted Bases if the PDDB has the wrong key.");
                    return PddbRekeyOp::InternalError;
                }
                self.migration_dna = dna;
                // acquire the .System basis key.
                self.dna_mode = DnaMode::Migration;
                self.syskey_ensure(); // this routine will continue to bang the user for a password. There is no way out otherwise.
                log::info!("migrating from dna 0x{:x} -> 0x{:x}", self.migration_dna, self.dna);
                self.fast_space_read(); // we hawe to re-read the FSCB, because it would have failed previously with lots of warnings
            }
            PddbRekeyOp::Churn => self.dna_mode = DnaMode::Churn,
            _ => (),
        };

        // clean up any MBBB, as it will mess up our algorithm
        if let Some(mbbb) = self.mbbb_retrieve() {
            if let Some(erased_offset) = self.pt_find_erased_slot() {
                self.spinor
                    .patch(
                        unsafe { self.pddb_mr.as_slice() },
                        xous::PDDB_LOC,
                        &mbbb,
                        self.pt_phys_base.as_u32() + erased_offset,
                    )
                    .expect("couldn't write to page table");
            }
            self.mbbb_erase();
        }

        // 0. acquire all the keys
        if let Some(all_keys) = self.pddb_get_all_keys(&cache) {
            // build a modals for rekey progress
            let xns = xous_names::XousNames::new().unwrap();
            let modals = modals::Modals::new(&xns).unwrap();

            modals.start_progress(t!("pddb.rekey.keys", locales::LANG), 0, all_keys.len() as u32, 0).ok();
            // we need a map of page numbers to encryption keys. The keys are referenced by their basis name.
            let mut pagemap = HashMap::<PhysAddr, &str>::new();
            // transform the returned Vec into a HashMap that maps basis names into pre-keyed ciphers.
            let mut keymap = HashMap::<&str, MigrationCiphers>::new();
            for (index, (basis_keys, name)) in all_keys.iter().enumerate() {
                if let Some(map) = self.pt_scan_key(&basis_keys.pt, &basis_keys.data, &name) {
                    for pp in map.values() {
                        log::info!("page {} -> basis {}", pp.page_number(), name);
                        if pagemap.insert(pp.page_number(), &name).is_some() {
                            log::warn!("double-insert detected of page number {}", pp.page_number());
                        }
                    }
                } else {
                    log::warn!("pt_scan for basis {} failed, data may be lost", name);
                }
                let pt_ecb = Aes256::new(&GenericArray::from_slice(&basis_keys.pt));
                let data_gcm_siv = Aes256GcmSiv::new(&basis_keys.data.into());
                self.dna_mode = DnaMode::Normal;
                let aad_local = self.data_aad(&name);
                self.dna_mode = DnaMode::Migration;
                let aad_incoming = self.data_aad(&name);
                keymap.insert(
                    &name,
                    MigrationCiphers {
                        pt_ecb,
                        data_gcm_siv,
                        aad_incoming,
                        aad_local,
                        data_key: basis_keys.data.into(),
                    },
                );
                modals.update_progress(index as u32 + 1).ok();
            }

            // check that we have no pages in the FSCB that aren't already mapped in the
            // page table.
            let mut clean = true;
            for pp in self.fspace_cache.iter() {
                if pp.space_state() == SpaceState::Used || pp.space_state() == SpaceState::MaybeUsed {
                    if !pagemap.contains_key(&pp.page_number()) {
                        log::warn!(
                            "FSCB records page {} ({:x?}) as used, but it's not defined in the page table. Please sync the PDDB before calling rekey.",
                            pp.page_number(),
                            pp
                        );
                        clean = false; // allow full enumeration of all errors, to help with debugging
                    }
                }
            }
            modals.finish_progress().ok();
            if !clean {
                log::warn!(
                    "Unmapped FSCB records were found, but this is likely due to FSCB flushes called. Continuing."
                );
                // return PddbRekeyOp::VerifyFail
            }

            // future note: if changing the password on just one basis, we'd want to ask for the new
            // password now, so we can use its new key to re-encrypt things. But for now, let's just
            // focus on rotating the DNA out.

            // iterate through every possible page in the system.
            // First, map the PageTableInFlash structure directly onto the region. It's located
            // at the base of the `pddb_mr`.
            let pddb_data_len = PDDB_A_LEN - self.data_phys_base.as_usize();
            let pddb_data_pages = pddb_data_len / PAGE_SIZE;
            let pagetable: &[u8] = unsafe { &self.pddb_mr.as_slice()[..pddb_data_pages * size_of::<Pte>()] };
            log::info!("Derived page table of len 0x{:x}", pagetable.len());
            let entries_per_page = PAGE_SIZE / size_of::<Pte>();
            modals
                .start_progress(
                    t!("pddb.rekey.running", locales::LANG),
                    0,
                    (pddb_data_pages * size_of::<Pte>()) as u32,
                    0,
                )
                .ok();
            for (chunk_enum, page) in pagetable.chunks(PAGE_SIZE).enumerate() {
                // this is the actual offset into pagetable[] that the page[] slice comes from
                let chunk_start_address = chunk_enum * PAGE_SIZE;
                let chunk_start_ppn = chunk_enum * entries_per_page;
                let mut chunk_len = 0; // we have to track this because the .chunks() will return a less-than-page sized chunk on the last iteration
                log::info!(
                    "processing chunk at 0x{:x} (page number {}",
                    chunk_start_address,
                    chunk_start_ppn
                );

                let mut new_pt_page = [0u8; PAGE_SIZE];
                assert!(page.len() % size_of::<Pte>() == 0);
                for (index, (enc_pte, new_pte)) in page
                    .chunks_exact(size_of::<Pte>())
                    .zip(new_pt_page.chunks_exact_mut(size_of::<Pte>()))
                    .enumerate()
                {
                    let ppn = (chunk_start_ppn + index) as u32;
                    chunk_len += size_of::<Pte>();
                    if let Some(&keyname) = pagemap.get(&ppn) {
                        log::debug!("re-encrypt page {} of {}", ppn, keyname);
                        let ciphers = keymap
                            .get(keyname)
                            .expect("How do we have a mapping to a key that we don't have?");
                        // ok, we know the physical page table number, and thus the page location, and the
                        // keys. we can now do the following:
                        // 1. re-encrypt the page table. It is relatively "free" to do a thorough
                        //    re-encryption
                        // so we will always either redo the nonce on every entry and/or fill the unused
                        // entries with fresh noise.
                        let mut block = Block::clone_from_slice(enc_pte);
                        ciphers.pt_ecb.decrypt_block(&mut block);
                        if let Some(mut pte) = Pte::try_from_slice(block.as_slice()) {
                            pte.re_nonce(Rc::clone(&self.entropy));
                            let mut enc_entry = Block::from_mut_slice(pte.deref_mut());
                            ciphers.pt_ecb.encrypt_block(&mut enc_entry);
                            new_pte.copy_from_slice(enc_entry.as_slice());
                        }

                        // 2. re-encrypt the disclosed basis data from the previous DNA to the current DNA.
                        //    Redo all the nonces
                        // while we are at it.
                        if let Some(mut data) = self.data_decrypt_page(
                            &ciphers.data_gcm_siv,
                            &ciphers.aad_incoming,
                            &PhysPage(ppn),
                        ) {
                            // this routine also redoes the nonce correctly.
                            self.data_encrypt_and_patch_page(
                                &ciphers.data_gcm_siv,
                                &ciphers.aad_local,
                                &mut data,
                                &PhysPage(ppn),
                            );
                        } else {
                            // see if it's a root page, which is encrypted differently due to AES-GCM-SIV
                            // salamanders and key commits.
                            if let Some(mut data) = self.data_decrypt_page_with_commit(
                                &ciphers.data_key,
                                &ciphers.aad_incoming,
                                &PhysPage(ppn),
                            ) {
                                log::debug!("...as basis root page...");
                                self.data_encrypt_and_patch_page_with_commit(
                                    &ciphers.data_key,
                                    &ciphers.aad_local,
                                    &mut data,
                                    &PhysPage(ppn),
                                );
                            } else {
                                log::warn!("Page number {} failed to decrypt. This data is now lost.", ppn);
                            }
                        }
                    } else {
                        // it's an unused page. fill in the PTE with garbage, and *maybe* fill in the mapped
                        // page with new garbage, too.
                        self.trng_slice(new_pte);

                        // the chance is expressed as a number from 0-255. It is compared against an 8-bit
                        // random number, and if the random number is less than the fraction, a rekey op will
                        // happen
                        let blank_rekey_chance = match op {
                            PddbRekeyOp::FromDnaFast(_) => FAST_REKEY_CHANCE,
                            PddbRekeyOp::FromDnaSafe(_) => 256, // 100% chance of rekey
                            PddbRekeyOp::Churn => 256,          // 100% chance of rekey
                            _ => continue,                      // don't rekey blanks for all other ops
                        };
                        if (self.trng_u8() as u32) < blank_rekey_chance {
                            log::debug!("re-noising unused page {}", ppn);
                            let mut noise = [0u8; PAGE_SIZE];
                            self.trng_slice(&mut noise);
                            self.patch_data(&noise, ppn * PAGE_SIZE as u32);
                        } else {
                            log::trace!("skipping unused page {}", ppn);
                        }
                    }
                }
                self.patch_pagetable_raw(&new_pt_page[..chunk_len], chunk_start_address as u32);
                // this only updates once per page of PTEs, so, every 256 pages that get re-encrypted in the
                // worst case.
                modals.update_progress(chunk_start_address as u32).ok();
            }
            modals.finish_progress().ok();

            self.dna_mode = DnaMode::Normal; // we're done with the legacy DNA!

            // 3. if we are not just changing the password on one Basis, regenerate the FSCB, since we
            // have all the Bases open
            let do_fscb = match op {
                PddbRekeyOp::FromDnaFast(_dna) | PddbRekeyOp::FromDnaSafe(_dna) => true,
                PddbRekeyOp::Churn => true,
                _ => false,
            };
            if do_fscb {
                log::info!("regenerating fast space...");
                modals.dynamic_notification(Some(t!("pddb.rekey.fastspace", locales::LANG)), None).ok();
                // convert our used page map into the structure needed by fast_space_generate()
                let mut page_heap = BinaryHeap::new();
                // drain doesn't actually de-allocate memory, but it gives us an opportunity
                // to recode this to use shrink_to_fit() every so many iterations to save on RAM
                // if we're bumping our head into that problem.
                for (ppn, _name) in pagemap.drain() {
                    page_heap.push(Reverse(ppn));
                }
                // generate and nuke old free space records
                let free_pool = self.fast_space_generate(page_heap);
                let mut fast_space = FastSpace { free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN] };
                for pp in fast_space.free_pool.iter_mut() {
                    pp.set_journal(self.trng_u8() % FSCB_JOURNAL_RAND_RANGE)
                }
                for (&src, dst) in free_pool.iter().zip(fast_space.free_pool.iter_mut()) {
                    *dst = src;
                }
                // write just commits a new record to disk, but doesn't update our internal data cache
                // this also clears the fast space log.
                self.fast_space_write(&fast_space);
                // this will ensure the data cache is fully in sync
                self.fast_space_read();
                modals.dynamic_notification_close().ok();
                log::info!("fast space generation done.");
            }
            PddbRekeyOp::Success
        } else {
            PddbRekeyOp::UserAbort
        }
    }

    /// UX function that informs the user of the currently open Basis, and prompts the user to enter passwords
    /// for other basis that may not currently be open. This function is also responsible for validating
    /// that the password is correct by doing a quick scan for "any" PTEs that happen to decrypt to something
    /// valid (it'll scan up and stop as soon as one Pte checks out). Note that it then only returns
    /// a Vec of keys & names, not a BasisCacheEntry -- so it means that the Basis still are "closed"
    /// at the conclusion of the sweep, but their page use can be accounted for.
    #[cfg(not(all(feature = "pddbtest", feature = "autobasis")))]
    pub(crate) fn pddb_get_all_keys(&self, cache: &Vec<BasisCacheEntry>) -> Option<Vec<(BasisKeys, String)>> {
        #[cfg(feature = "ux-swap-delay")]
        const SWAP_DELAY_MS: usize = 300;
        // populate the "known" entries
        let mut ret = Vec::<(BasisKeys, String)>::new();
        for entry in cache {
            ret.push((BasisKeys { pt: entry.pt_key.into(), data: entry.key.into() }, entry.name.to_string()));
        }
        log::info!("{} basis are open, with the following names:", cache.len());
        for entry in cache {
            log::info!(" - {}", entry.name);
        }
        // In the case of a migration, the basis cache would be empty, but the system basis key is already set
        // up
        if self.dna_mode == DnaMode::Migration {
            let syskeys = self
                .system_basis_key
                .as_ref()
                .expect("Internal error: syskey_ensure() must be called prior to get_all_keys()");
            ret.push((
                BasisKeys { pt: syskeys.pt.into(), data: syskeys.data.into() },
                PDDB_DEFAULT_SYSTEM_BASIS.to_string(),
            ));
            // note: because self.dna_mode is in 'Migration', the AAD checks in this section will be
            // using the original DNA, so they should pass.
        }

        // 0. allow user to cancel out of the operation -- this will abort everything and cause the current
        //    alloc operation to fail
        let xns = xous_names::XousNames::new().unwrap();
        let modals = modals::Modals::new(&xns).unwrap();
        modals
            .show_notification(
                match self.dna_mode {
                    DnaMode::Normal => t!("pddb.freespace.request", locales::LANG),
                    DnaMode::Migration => t!("pddb.rekey.request", locales::LANG),
                    DnaMode::Churn => t!("pddb.churn.request", locales::LANG),
                },
                None,
            )
            .ok();
        #[cfg(feature = "ux-swap-delay")]
        self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();

        // 0.5 display the Bases that we know
        let mut blist = String::from(t!("pddb.freespace.currentlist", locales::LANG));
        for (_key, name) in ret.iter() {
            blist.push_str("\n");
            blist.push_str(name);
        }
        modals.show_notification(&blist, None).ok();
        #[cfg(feature = "ux-swap-delay")]
        self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();

        // 1. prompt user to enter any name/password combos for other basis we want to keep
        while self.yes_no_approval(&modals, t!("pddb.freespace.enumerate_another", locales::LANG)) {
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();

            match modals.alert_builder(t!("pddb.freespace.name", locales::LANG)).field(None, None).build() {
                Ok(bname) => {
                    let name = bname.first().as_str().to_string();
                    let request =
                        BasisRequestPassword { db_name: String::from(name.to_string()), plaintext_pw: None };
                    let mut buf = Buffer::into_buf(request).unwrap();
                    buf.lend_mut(self.pw_cid, PwManagerOpcode::RequestPassword.to_u32().unwrap()).unwrap();
                    let retpass = buf.to_original::<BasisRequestPassword, _>().unwrap();
                    // 2. validate the name/password combo
                    let basis_key = self.basis_derive_key(&name, retpass.plaintext_pw.unwrap().as_str());
                    // validate the password by finding the root block of the basis. We rely entirely
                    // upon the AEAD with key commit to ensure the password is correct.
                    let maybe_entry = if let Some(basis_map) =
                        self.pt_scan_key(&basis_key.pt, &basis_key.data, &name)
                    {
                        let aad = self.data_aad(&name);
                        if let Some(root_page) = basis_map.get(&VirtAddr::new(VPAGE_SIZE as u64).unwrap()) {
                            match self.data_decrypt_page_with_commit(&basis_key.data, &aad, root_page) {
                                Some(_data) => {
                                    // if the root page decrypts, we accept the password; no further checking
                                    // is done.
                                    Some((basis_key, name.to_string()))
                                }
                                None => None,
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    // 3. add to the Aes256 return vec
                    if let Some((basis_key, name)) = maybe_entry {
                        ret.push((basis_key, name));
                    } else {
                        modals.show_notification(t!("pddb.freespace.badpass", locales::LANG), None).ok();
                        #[cfg(feature = "ux-swap-delay")]
                        self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                    }
                }
                _ => return None,
            };
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
            // 4. repeat summary print-out
            let mut blist = String::from(t!("pddb.freespace.currentlist", locales::LANG));
            for (_key, name) in ret.iter() {
                blist.push_str("\n");
                blist.push_str(name);
            }
            modals.show_notification(&blist, None).ok();
            #[cfg(feature = "ux-swap-delay")]
            self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
        }
        // done!
        if self.yes_no_approval(
            &modals,
            match self.dna_mode {
                DnaMode::Normal => t!("pddb.freespace.finished", locales::LANG),
                DnaMode::Migration => t!("pddb.rekey.finished", locales::LANG),
                DnaMode::Churn => t!("pddb.churn.finished", locales::LANG),
            },
        ) {
            Some(ret)
        } else {
            None
        }
    }

    fn yes_no_approval(&self, modals: &modals::Modals, request: &str) -> bool {
        modals
            .add_list(vec![t!("pddb.yes", locales::LANG), t!("pddb.no", locales::LANG)])
            .expect("couldn't build confirmation dialog");
        match modals.get_radiobutton(request) {
            Ok(response) => {
                if &response == t!("pddb.yes", locales::LANG) {
                    true
                } else {
                    false
                }
            }
            _ => {
                log::error!("get approval failed");
                false
            }
        }
    }

    /// Derives a 256-bit AES encryption key for a basis given a basis name and its password.
    /// You will also need to derive the AAD for the basis using the basis_name.
    pub(crate) fn basis_derive_key(&self, basis_name: &str, password: &str) -> BasisKeys {
        // 1. derive the salt from the "key" region. First step is to create the salt lookup
        // table, which is done by hashing the name and password together with SHA-512
        // manage the allocation of the data for the basis & password explicitly so that we may wipe them
        // later
        let mut bname_copy = [0u8; BASIS_NAME_LEN];
        for (src, dst) in basis_name.bytes().zip(bname_copy.iter_mut()) {
            *dst = src;
        }
        let mut plaintext_pw: [u8; 73] = [0; 73];
        for (src, dst) in password.bytes().zip(plaintext_pw.iter_mut()) {
            *dst = src;
        }
        plaintext_pw[72] = 0; // always null terminate

        log::info!("creating salt");
        // uses Sha512_256 on the salt array to generate a compressed version of
        // the basis name and plaintext password, which forms the Salt that is fed into bcrypt
        // our salt is probably way too big but what else are we going to use all that page's data for?
        let scd = self.static_crypto_data_get();
        let mut salt = [0u8; 16];
        let mut hasher = Sha512_256Sw::new();
        hasher.update(&scd.salt_base[32..]); // reserve the first 32 bytes of salt for the HKDF
        hasher.update(&bname_copy);
        hasher.update(&plaintext_pw);
        let result = hasher.finalize();
        for (&src, dst) in result.iter().zip(salt.iter_mut()) {
            *dst = src;
        }
        #[cfg(feature = "hazardous-debug")]
        log::info!("derived salt: {:x?}", salt);

        // 3. use the salt + password and run bcrypt on it to derive a key.
        let mut hashed_password: [u8; 24] = [0; 24];
        let start_time = self.timestamp_now();
        bcrypt(BCRYPT_COST, &salt, password, &mut hashed_password); // note: this internally makes a copy of the password, and destroys it
        let elapsed = self.timestamp_now() - start_time;
        log::info!("derived bcrypt password in {}ms", elapsed);

        // 4. take the resulting 24-byte password and expand it to 2x 32 byte keys using HKDF.
        // one key is for the AES-256 ECB-encoded page tables, one key is for the AES-GCM-SIV data pages
        let hkpt = hkdf::Hkdf::<sha2::Sha256>::new(Some(&scd.salt_base[..32]), &hashed_password);
        let mut okm_pt = [0u8; 32];
        hkpt.expand(b"pddb page table key", &mut okm_pt).expect("invalid length specified for HKDF");

        let hkdt = hkdf::Hkdf::<sha2::Sha256>::new(Some(&scd.salt_base[..32]), &hashed_password);
        let mut okm_data = [0u8; 32];
        hkdt.expand(b"pddb data key", &mut okm_data).expect("invalid length specified for HKDF");

        // 5. erase extra plaintext copies made of the basis name and password using a routine that
        // shouldn't be optimized out or re-ordered
        let bn_ptr = bname_copy.as_mut_ptr();
        for i in 0..bname_copy.len() {
            unsafe {
                bn_ptr.add(i).write_volatile(core::mem::zeroed());
            }
        }
        let pt_ptr = plaintext_pw.as_mut_ptr();
        for i in 0..plaintext_pw.len() {
            unsafe {
                pt_ptr.add(i).write_volatile(core::mem::zeroed());
            }
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        #[cfg(feature = "hazardous-debug")]
        log::info!("okm_pt: {:x?}", okm_pt);
        #[cfg(feature = "hazardous-debug")]
        log::info!("okm_data: {:x?}", okm_data);

        // 6. return the keys
        BasisKeys { pt: okm_pt, data: okm_data }
    }

    pub(crate) fn reset_dont_ask_init(&self) { self.rootkeys.do_reset_dont_ask_init(); }

    pub(crate) fn checksums(&self, modals: Option<&Modals>) -> root_keys::api::Checksums {
        let mut checksums = root_keys::api::Checksums::default();
        let pddb = unsafe { self.pddb_mr.as_slice() };
        if let Some(m) = modals {
            m.start_progress(t!("pddb.checksums", locales::LANG), 0, checksums.checksums.len() as u32, 0)
                .ok();
        }
        for (index, region) in
            pddb.chunks(root_keys::api::CHECKSUM_BLOCKLEN_PAGE as usize * PAGE_SIZE).enumerate()
        {
            assert!(
                region.len() == root_keys::api::CHECKSUM_BLOCKLEN_PAGE as usize * PAGE_SIZE,
                "CHECKSUM_BLOCKLEN_PAGE is not an even divisor of the PDDB size"
            );
            let mut hasher = Sha512_256Hw::new();
            hasher.update(region); // reserve the first 32 bytes of salt for the HKDF
            let digest = hasher.finalize();
            // copy only the first 128 bits of the hash into the checksum array
            checksums.checksums[index].copy_from_slice(&digest.as_slice()[..16]);
            if let Some(m) = modals {
                m.update_progress(index as u32).ok();
            }
        }
        if let Some(m) = modals {
            m.finish_progress().ok();
        }
        checksums
    }

    //-------------------------------- TESTING -----------------------------------------
    // always gated behind a feature flag. Includes routines that are nonsensicle in normal operation at best,
    // and very dangerous from a security perspective at worst.

    /// basis_testing takes an argument a vector of supplemental test Bases to mount or unmount.
    /// If the basis does not exist, it is created. This accompanies a warning, because we can't know
    /// if the intention was to create the basis or just mount it; but for testing, the diagnosis is always
    /// going to be in the test logs, so it's more direct to print the error here instead of percolating
    /// it back toward the caller.
    /// The `Vec` is interpretd as follows:
    ///   - index of `vec` corresponds to the test basis number.
    ///   - the `Option<bool>` if `true` means to mount; if `false` means to unmount; None means skip
    /// The basis always has the naming format of `test#`, where # is the index in the Vec. This allows
    /// testing routines to selectively write to one basis by designating it as, for example
    /// `test0`, `test1`, `test2`,...; the password is the same name as the Basis, for simplicity.
    ///
    /// The tooling and argument spec on this is deliberately sparse because this routine is meant to
    /// be only used "if you know what you're doing" and we want to make the arg passing as minimal/easy
    /// as possible (just a scalar) to cut down on testing overhead.
    #[cfg(all(feature = "pddbtest", feature = "autobasis"))]
    pub(crate) fn basis_testing(&mut self, cache: &mut BasisCache, config: &[Option<bool>; 32]) {
        for (index, &maybe_op) in config.iter().enumerate() {
            let name = format!("{}{}", BASIS_TEST_ROOTNAME, index);
            if let Some(op) = maybe_op {
                if op {
                    // try to mount the basis, or create it if it does not exist.
                    self.testnames.insert(name.to_string());
                    let maybe_basis = cache.basis_unlock(self, &name, &name, BasisRetentionPolicy::Persist);

                    if maybe_basis.is_some() {
                        cache.basis_add(maybe_basis.unwrap());
                    } else {
                        cache.basis_create(self, &name, &name).expect("couldn't create basis");
                        let basis = cache
                            .basis_unlock(self, &name, &name, BasisRetentionPolicy::Persist)
                            .expect("couldn't open just created basis");
                        cache.basis_add(basis);
                    }
                } else {
                    let blist = cache.basis_list();
                    if blist.contains(&name) {
                        match cache.basis_unmount(self, &name) {
                            Ok(_) => {}
                            Err(e) => log::error!("error unmounting basis {}: {:?}", name, e),
                        }
                    } else {
                        log::info!(
                            "attempted to unmount {} but it does not exist or is already locked",
                            name
                        );
                    }
                }
            }
        }
    }

    /// In testing, we want a way to automatically unlock all known test Bases. This stand-in feature
    /// automates that.
    #[cfg(all(feature = "pddbtest", feature = "autobasis"))]
    pub(crate) fn pddb_get_all_keys<'a>(
        &'a self,
        cache: &'a Vec<BasisCacheEntry>,
        _op: GetKeyOp,
    ) -> Option<Vec<(BasisKeys, String)>> {
        // populate the "known" entries
        let mut ret = Vec::<(BasisKeys, String)>::new();
        for entry in cache {
            ret.push((BasisKeys { pt: entry.pt_key.into(), data: entry.key.into() }, entry.name.to_string()));
        }

        // now iterate through the bases that have been enumerated so far, and populate the missing ones.
        for name in self.testnames.iter() {
            // if the entry isn't known already, add it
            // yah, this is O(n^2) awful. yolo!
            let mut exists = false;
            for (_bk, bname) in ret.iter() {
                if bname == name {
                    exists = true;
                    break;
                }
            }
            if !exists {
                let basis_key = self.basis_derive_key(name, name);
                ret.push((basis_key, name.to_string()));
            }
        }
        log::info!("for FSCB testing, we auto-unlocked {} bases:", ret.len());
        for (_bk, entry) in ret.iter() {
            log::info!("  - {}", entry);
        }
        Some(ret)
    }

    #[cfg(all(feature = "pddbtest", feature = "autobasis"))]
    pub(crate) fn dbg_extra(&self) -> Vec<KeyExport> {
        let mut ret = Vec::<KeyExport>::new();
        for name in self.testnames.iter() {
            let basis_key = self.basis_derive_key(name, name);
            let mut basis_name = [0 as u8; 64];
            for (&src, dst) in name.as_bytes().iter().zip(basis_name.iter_mut()) {
                *dst = src;
            }
            ret.push(KeyExport { basis_name, key: basis_key.data, pt_key: basis_key.pt });
        }
        ret
    }

    //-------------------------------- MIGRATIONS -----------------------------------------
    // these are always gated behind feature flags, and disabled in builds once obsolete.

    /// legacy key derivation for migrations. This can be removed once the migration is de-supported.
    /// Must be within this structure because it accesses the rootkeys, and we don't want to make that public.
    /// Need to pass this the old version of the StaticCryptoData, because it's already erased and replaced
    /// by updated keys by the time this routine is called.
    #[cfg(feature = "migration1")]
    pub(crate) fn basis_derive_key_v00_00_01_01(
        &self,
        basis_name: &str,
        password: &str,
        scd: &StaticCryptoDataV1,
    ) -> [u8; AES_KEYSIZE] {
        // 1. derive the salt from the "key" region. First step is to create the salt lookup
        // table, which is done by hashing the name and password together with SHA-512
        // manage the allocation of the data for the basis & password explicitly so that we may wipe them
        // later
        let mut bname_copy = [0u8; BASIS_NAME_LEN];
        for (src, dst) in basis_name.bytes().zip(bname_copy.iter_mut()) {
            *dst = src;
        }
        let mut plaintext_pw: [u8; 73] = [0; 73];
        for (src, dst) in password.bytes().zip(plaintext_pw.iter_mut()) {
            *dst = src;
        }
        plaintext_pw[72] = 0; // always null terminate

        log::info!("creating salt");
        // uses Sha512_256 on the salt array to generate a compressed version of
        // the basis name and plaintext password, which forms the Salt that is fed into bcrypt
        // our salt is probably way too big but what else are we going to use all that page's data for?
        let mut salt = [0u8; 16];
        let mut hasher = Sha512_256Sw::new();
        hasher.update(&scd.salt_base);
        hasher.update(&bname_copy);
        hasher.update(&plaintext_pw);
        let result = hasher.finalize();
        for (&src, dst) in result.iter().zip(salt.iter_mut()) {
            *dst = src;
        }
        #[cfg(feature = "hazardous-debug")]
        log::info!("derived salt: {:x?}", salt);

        // 3. use the salt + password and run bcrypt on it to derive a key.
        let mut hashed_password: [u8; 24] = [0; 24];
        let start_time = self.timestamp_now();
        bcrypt(BCRYPT_COST, &salt, password, &mut hashed_password); // note: this internally makes a copy of the password, and destroys it
        let elapsed = self.timestamp_now() - start_time;
        log::info!("derived bcrypt password in {}ms", elapsed);

        // 4. take the resulting 24-byte password and expand it to 32 bytes using sha512trunc256
        let mut expander = Sha512_256Sw::new();
        expander.update(hashed_password);
        let final_key = expander.finalize();
        let mut key = [0u8; AES_KEYSIZE];
        for (&src, dst) in final_key.iter().zip(key.iter_mut()) {
            *dst = src;
        }

        // 5. erase extra plaintext copies made of the basis name and password using a routine that
        // shouldn't be optimized out or re-ordered
        let bn_ptr = bname_copy.as_mut_ptr();
        for i in 0..bname_copy.len() {
            unsafe {
                bn_ptr.add(i).write_volatile(core::mem::zeroed());
            }
        }
        let pt_ptr = plaintext_pw.as_mut_ptr();
        for i in 0..plaintext_pw.len() {
            unsafe {
                pt_ptr.add(i).write_volatile(core::mem::zeroed());
            }
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        // 6. return the key
        key
    }

    #[cfg(feature = "migration1")]
    fn migration_v1_to_v2_inner(
        &mut self,
        aad_v1: &Vec<u8>,
        aad_v2: &Vec<u8>,
        key_v1: &[u8; AES_KEYSIZE], // needed to decrypt page with commit
        cipher_pt_v1: &Aes256,
        cipher_data_v1: &Aes256GcmSiv,
        key_data_v2: &[u8; AES_KEYSIZE], // this is needed because we have to do a key commitment
        cipher_pt_v2: &Aes256,
        cipher_data_v2: &Aes256GcmSiv,
        used_pages: &mut BinaryHeap<Reverse<u32>>,
    ) -> bool {
        let blank = [0xffu8; aes::BLOCK_SIZE];
        let pt = self.pt_as_slice();
        let mut found_basis = false;

        // move over the MBBB, if it exists
        for (page_index, pt_page) in pt.chunks(PAGE_SIZE).enumerate() {
            if pt_page[..aes::BLOCK_SIZE] == blank {
                if let Some(page) = self.mbbb_retrieve() {
                    log::info!(
                        "Found MBBB, restoring 0x100 pages @ ppn {:x} & erasing MBBB",
                        (page_index * PAGE_SIZE / aes::BLOCK_SIZE)
                    );
                    self.spinor
                        .patch(
                            self.pddb_mr.as_slice(),
                            xous::PDDB_LOC,
                            &page,
                            self.pt_phys_base.as_u32() + (page_index * PAGE_SIZE) as u32,
                        )
                        .expect("couldn't write to page table");
                    self.mbbb_erase();
                } else {
                    log::info!(
                        "Blank page in PT found, but no MBBB entry exists. PT is either corrupted or not initialized!"
                    );
                }
            }
        }

        // now do the full page table scan
        for (page_index, pt_page) in pt.chunks(PAGE_SIZE).enumerate() {
            for (index, candidate) in pt_page.chunks(aes::BLOCK_SIZE).enumerate() {
                let mut block = Block::clone_from_slice(candidate);
                cipher_pt_v1.decrypt_block(&mut block);
                /* // some focused debugging code for reference in the future
                let dbg_pagenum = (page_index * PAGE_SIZE / aes::BLOCK_SIZE) + index;
                if dbg_pagenum == 0x5091 {
                    log::info!("c: {:x?}", candidate);
                    log::info!("p: {:x?}", block.as_slice());
                }
                if dbg_pagenum >= 0x5077 && dbg_pagenum <= 0x5094 {
                    log::info!("c{:x}: {:x?}", dbg_pagenum, candidate);
                }
                */
                // *** 2. if an entry matches, also decrypt the target page and store it here
                if let Some(pte) = Pte::try_from_slice(block.as_slice()) {
                    // compute the physical page number that correspons to this entry, and store it in `pp`
                    let mut pp = PhysPage(0);
                    pp.set_page_number(((page_index * PAGE_SIZE / aes::BLOCK_SIZE) + index) as PhysAddr);

                    // root records require key commitment (to avoid AES-GCM-SIV salamanders), so detect &
                    // handle that
                    if pte.vaddr_v1().get() == VPAGE_SIZE as u64 {
                        // v1 addresses were full vaddrs, not page numbers
                        log::info!(
                            "migration: potential root block at (pp){:x}/(vp){:x}",
                            pp.page_number(),
                            pte.vaddr_v1().get()
                        );
                        // retrieve the data at `pp`
                        let migrating_data = self.data_decrypt_page_with_commit(key_v1, aad_v1, &pp);
                        if let Some(vpage) = migrating_data {
                            log::info!("migration: found root block!");
                            found_basis = true;
                            let mut basis_root = BasisRoot::default();
                            for (&src, dst) in (&vpage)[size_of::<JournalType>()..]
                                .iter()
                                .zip(basis_root.deref_mut().iter_mut())
                            {
                                *dst = src;
                            }
                            let (previous, _) = PDDB_MIGRATE_1;
                            if basis_root.version != previous {
                                log::warn!(
                                    "Root basis record did not match expected version during migration. Ignoring and attempting to move on..."
                                );
                            }
                            basis_root.version = PDDB_VERSION; // update the version to our current one
                            log::info!("Basis root: {:?}", basis_root);
                            let slice_iter = (&vpage[..size_of::<JournalType>()]).iter() // just copy the journal rev
                                .chain(basis_root.as_ref().iter());
                            let mut block = [0 as u8; KCOM_CT_LEN];
                            for (&src, dst) in slice_iter.zip(block.iter_mut()) {
                                *dst = src;
                            }
                            self.data_encrypt_and_patch_page_with_commit(
                                key_data_v2,
                                aad_v2,
                                &mut block,
                                &pp,
                            );
                        } else {
                            // could be a checksum collision that triggers this, so it's not a hard error. A
                            // later "real" version will succeed and all would be fine.
                            log::warn!(
                                "Root basis record did not decrypt correctly, ignoring and hoping for the best, but your chances are slim."
                            );
                        }
                    } else {
                        log::info!("migrating (pp){:x}/(vp){:x}", pp.page_number(), pte.vaddr_v1().get());
                        // retrieve the data at `pp`
                        let migrating_data = self.data_decrypt_page(&cipher_data_v1, aad_v1, &pp);
                        // store the data back to the same location, but with the new keys
                        if let Some(mut vpage) = migrating_data {
                            self.data_encrypt_and_patch_page(&cipher_data_v2, aad_v2, &mut vpage, &pp);
                        } else {
                            log::warn!(
                                "Potential checksum collision found at pp {}; ignoring",
                                pp.page_number()
                            );
                        }
                    }

                    // *** 3. re-encrypt the PTE and the target page to the v2 keys and corrected addressing
                    // scheme a deconstructed pt_patch_mapping() call -- because the
                    // normal call would insert a MBBB block, which is not what we want in this case.
                    let mut pte = Pte::new(pte.vaddr_v1(), PtFlags::CLEAN, Rc::clone(&self.entropy));
                    let mut pt_block = Block::from_mut_slice(pte.deref_mut());
                    cipher_pt_v2.encrypt_block(&mut pt_block);
                    self.patch_pagetable_raw(&pt_block, pp.page_number() * aes::BLOCK_SIZE as u32);

                    // track the used pages
                    used_pages.push(Reverse(pp.page_number()));
                }
            }
        }
        found_basis
    }

    /// Migrates a v1 database to a v2 database. The main change is a modification to the key
    /// derivation schedule to make different keys for the page table versus the pages themselves.
    ///
    /// note that v1 also had an error where page tables were encoded in full addresess, when
    /// we meant to encode them using page numbers. This code also handles that migration.
    /// Must be within this structure because it accesses the rootkeys, and we don't want to make that public.
    #[cfg(feature = "migration1")]
    pub(crate) fn migration_v1_to_v2(&mut self, pw_cid: xous::CID) -> PasswordState {
        use aes::cipher::KeyInit;
        let scd = self.static_crypto_data_get_v1();
        if scd.version == 0xFFFF_FFFF {
            // system is in the blank state
            return PasswordState::Uninit;
        }
        if scd.version != SCD_VERSION_MIGRATION1 {
            return PasswordState::Uninit;
        }
        log::info!("v1 PDDB detected. Attempting to migrate from v1->v2.");
        log::info!("old SCD block: {:x?}", &scd.deref()[..128]); // this is not hazardous because the keys were wrapped

        #[cfg(not(target_os = "xous"))]
        let mut export = Vec::<KeyExport>::new(); // export any basis keys for verification in hosted mode

        // derive a v1 key

        match self.rootkeys.unwrap_key(&scd.system_key, AES_KEYSIZE) {
            Ok(mut syskey) => {
                // build a modals for migration messages
                let xns = xous_names::XousNames::new().unwrap();
                let modals = modals::Modals::new(&xns).unwrap();
                // v1->v2 messages willl only be in English, because we don't have any non-English users yet
                // (afaik)

                let cipher_v1 = Aes256::new(GenericArray::from_slice(&syskey));
                let mut system_key_v1: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
                for (&src, dst) in syskey.iter().zip(system_key_v1.iter_mut()) {
                    *dst = src;
                }
                // erase the old vector completely, now that the key is in system_key_v1
                let nuke = syskey.as_mut_ptr();
                for i in 0..syskey.len() {
                    unsafe { nuke.add(i).write_volatile(0) };
                }
                modals
                    .dynamic_notification(Some("PDDB v1->v2 migration"), Some("Migrating System keys to v2"))
                    .unwrap();

                // ------ I. migrate the system basis -------
                // *** 0. generate v2 keys. This will immediately overwrite the SCD -- if we have an error or
                // power outage after this point, we lose the entire PDDB. The other option is
                // to commit the v2 keys at the end, but, similarly, we end up with a partial
                // migration either way.
                let mut system_basis_key_pt: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
                let mut system_basis_key: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE];
                let mut syskey_data_copy: [u8; AES_KEYSIZE] = [0; AES_KEYSIZE]; // we will need a copy of this later on for re-encrypting with key commit
                self.entropy.borrow_mut().get_slice(&mut system_basis_key_pt);
                self.entropy.borrow_mut().get_slice(&mut system_basis_key);
                for (&s, d) in system_basis_key.iter().zip(syskey_data_copy.iter_mut()) {
                    *d = s;
                }
                // build the ECB cipher for page table entries, and data cipher for data pages
                self.cipher_ecb = Some(Aes256::new(GenericArray::from_slice(&system_basis_key_pt)));
                let cipher_v2 = Aes256::new(GenericArray::from_slice(&system_basis_key_pt)); // a second copy for patching the page table later in this routine interior mutability blah blah work around oops
                let data_cipher_v2 = Aes256GcmSiv::new(Key::from_slice(&system_basis_key));
                let aad_v2 = self.data_aad(PDDB_DEFAULT_SYSTEM_BASIS);
                // now wrap the key for storage
                let wrapped_key = self
                    .rootkeys
                    .wrap_key(&system_basis_key)
                    .expect("Internal error wrapping our encryption key");
                let wrapped_key_pt = self
                    .rootkeys
                    .wrap_key(&system_basis_key_pt)
                    .expect("Internal error wrapping our encryption key");
                self.system_basis_key = Some(BasisKeys { pt: system_basis_key_pt, data: system_basis_key }); // this causes system_basis_key to be owned by self and go out of scope
                let mut crypto_keys = StaticCryptoData::default();
                crypto_keys.version = SCD_VERSION; // should already be set by `default()` but let's be sure.

                // copy the encrypted key into the data structure for commit to Flash
                // the wrapped key should have a well-defined length of 40 bytes
                for (&src, dst) in wrapped_key.iter().zip(crypto_keys.system_key.iter_mut()) {
                    *dst = src;
                }
                for (&src, dst) in wrapped_key_pt.iter().zip(crypto_keys.system_key_pt.iter_mut()) {
                    *dst = src;
                }
                // initialize the salt
                self.entropy.borrow_mut().get_slice(&mut crypto_keys.salt_base);
                // commit keys
                log::info!("patching new SCD block in: {:x?}", &crypto_keys.deref()[..128]); // this is not hazardous because the keys were wrapped
                self.patch_keys(crypto_keys.deref(), 0);

                // now we have a copy of the AES key necessary to re-encrypt all the basis
                // build the data cipher for v1 pages
                let data_cipher_v1 = Aes256GcmSiv::new(Key::from_slice(&system_key_v1));
                let aad_v1 = data_aad_v1(&self, PDDB_DEFAULT_SYSTEM_BASIS);

                // track used pages so we can create the FSCB at the end
                let mut used_pages = BinaryHeap::new();

                // *** 1. scan the page table
                modals.dynamic_notification_update(None, Some("Migrating System Basis")).unwrap();
                // *** 2. if an entry matches, also decrypt the target page and store it here
                // *** 3. re-encrypt the PTE and the target page to the v2 keys and corrected addressing
                // scheme
                if !self.migration_v1_to_v2_inner(
                    &aad_v1,
                    &aad_v2,
                    &system_key_v1,
                    &cipher_v1,
                    &data_cipher_v1,
                    &syskey_data_copy,
                    &cipher_v2,
                    &data_cipher_v2,
                    &mut used_pages,
                ) {
                    log::warn!("System basis migration failed");
                } else {
                    log::info!("System basis migration successful");
                }

                // *** 4. The MBBB is natively migrated as part of this process, so nothing explictly needs to
                // be done here.

                // ------ II. migrate any hidden basis ------
                modals.dynamic_notification_close().unwrap();
                let mut prompt = String::from(
                    "Any secret Bases must be migrated now, or else their data will be lost.\n\nUnlock a Basis for migration?",
                );
                loop {
                    modals
                        .add_list_item(t!("pddb.yes", locales::LANG))
                        .expect("couldn't build radio item list");
                    modals
                        .add_list_item(t!("pddb.no", locales::LANG))
                        .expect("couldn't build radio item list");
                    match modals.get_radiobutton(&prompt) {
                        Ok(response) => {
                            if response.as_str() == t!("pddb.yes", locales::LANG) {
                                match modals
                                    .alert_builder("Enter the Basis name")
                                    .field(Some("My Secret Basis".to_string()), None)
                                    .build()
                                {
                                    Ok(bname) => {
                                        let request = BasisRequestPassword {
                                            db_name: String::from(bname.first().as_str()),
                                            plaintext_pw: None,
                                        };
                                        let mut buf = Buffer::into_buf(request).unwrap();
                                        buf.lend_mut(
                                            pw_cid,
                                            PwManagerOpcode::RequestPassword.to_u32().unwrap(),
                                        )
                                        .unwrap();
                                        let ret = buf.to_original::<BasisRequestPassword, _>().unwrap();
                                        if let Some(pw) = ret.plaintext_pw {
                                            // derive old and new keys
                                            let basis_key_v1 = self.basis_derive_key_v00_00_01_01(
                                                bname.first().as_str(),
                                                pw.as_str(),
                                                &scd,
                                            );
                                            let basis_aad_v1 = data_aad_v1(&self, bname.first().as_str());
                                            let basis_aad_v2 = self.data_aad(bname.first().as_str());
                                            let basis_pt_cipher_v1 =
                                                Aes256::new(GenericArray::from_slice(&basis_key_v1));
                                            let basis_data_cipher_v1 =
                                                Aes256GcmSiv::new(Key::from_slice(&basis_key_v1));
                                            let basis_keys = self.basis_derive_key(
                                                bname.first().as_str(),
                                                pw.as_str().unwrap_or("UTF8 error"),
                                            );
                                            let basis_pt_cipher_2 =
                                                Aes256::new(GenericArray::from_slice(&basis_keys.pt));
                                            let basis_data_cipher_2 =
                                                Aes256GcmSiv::new(Key::from_slice(&basis_keys.data));
                                            // perform the migration
                                            if self.migration_v1_to_v2_inner(
                                                &basis_aad_v1,
                                                &basis_aad_v2,
                                                &basis_key_v1,
                                                &basis_pt_cipher_v1,
                                                &basis_data_cipher_v1,
                                                &basis_keys.data,
                                                &basis_pt_cipher_2,
                                                &basis_data_cipher_2,
                                                &mut used_pages,
                                            ) {
                                                #[cfg(not(target_os = "xous"))]
                                                {
                                                    let mut name = [0 as u8; 64];
                                                    for (&src, dst) in bname
                                                        .first()
                                                        .as_str()
                                                        .as_bytes()
                                                        .iter()
                                                        .zip(name.iter_mut())
                                                    {
                                                        *dst = src;
                                                    }
                                                    export.push(KeyExport {
                                                        basis_name: name,
                                                        key: basis_keys.data,
                                                        pt_key: basis_keys.pt,
                                                    });
                                                }
                                                prompt.clear();
                                                prompt.push_str(
                                                    "Migration success, migrate another secret Basis?",
                                                );
                                            } else {
                                                prompt.clear();
                                                prompt.push_str("Migration failure, retry and/or migrate another secret Basis?");
                                            }
                                        } else {
                                            log::warn!(
                                                "Couldn't retrieve password for the basis, ignoring and moving on"
                                            );
                                            prompt.clear();
                                            prompt.push_str("Error unlocking Basis, retry?");
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!("error {:?} unlocking basis, aborting and moving on", e);
                                        prompt.clear();
                                        prompt.push_str("Error unlocking Basis, retry?");
                                    }
                                }
                            } else if response.as_str() == t!("pddb.no", locales::LANG) {
                                break;
                            } else {
                                log::warn!("Got unexpected return from radiobutton: {}", response);
                            }
                        }
                        _ => log::warn!("get_radiobutton failed"),
                    }
                }

                // ------ III. nuke and regnerate fast space -----
                modals
                    .dynamic_notification(
                        Some("Finalizing PDDB v1->v2 migration"),
                        Some("Regenerate FastSpace"),
                    )
                    .unwrap();
                let free_pool = self.fast_space_generate(used_pages);
                let mut fast_space = FastSpace { free_pool: [PhysPage(0); FASTSPACE_FREE_POOL_LEN] };
                for pp in fast_space.free_pool.iter_mut() {
                    pp.set_journal(self.trng_u8() % FSCB_JOURNAL_RAND_RANGE)
                }
                for (&src, dst) in free_pool.iter().zip(fast_space.free_pool.iter_mut()) {
                    *dst = src;
                }
                // write just commits a new record to disk, but doesn't update our internal data cache
                // this also clears the fast space log.
                self.fast_space_write(&fast_space);
                // this will ensure the data cache is fully in sync
                self.fast_space_read();

                modals.dynamic_notification_close().unwrap();

                // clear out the v1 key
                let sk_ptr = system_key_v1.as_mut_ptr();
                for i in 0..system_key_v1.len() {
                    unsafe {
                        sk_ptr.add(i).write_volatile(core::mem::zeroed());
                    }
                }
                // clear out the copy of the v2 system data key, required for key commits of the basis
                let sk_ptr = syskey_data_copy.as_mut_ptr();
                for i in 0..syskey_data_copy.len() {
                    unsafe {
                        sk_ptr.add(i).write_volatile(core::mem::zeroed());
                    }
                }
                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

                #[cfg(not(target_os = "xous"))]
                self.dbg_dump(Some("migration".to_string()), Some(&export));

                // indicate the migration worked
                self.failed_logins = 0;
                PasswordState::Correct
            }
            Err(e) => {
                log::error!("Couldn't unwrap our system key: {:?}", e);
                PasswordState::Incorrect(self.failed_logins)
            }
        }
    }
}
