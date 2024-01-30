#![cfg_attr(all(target_os = "none", not(test)), no_std)]

#[cfg(test)]
#[macro_use]
extern crate lazy_static;
#[cfg(test)]
use std::sync::Mutex;

// build a "fake" flash area for tests
#[cfg(test)]
lazy_static! {
    static ref EMU_FLASH: Mutex<Vec<u8>> = Mutex::new(vec![]);
}

pub mod api;
pub use api::*;
use num_traits::*;
use xous::{send_message, Message, CID};
use xous_ipc::Buffer;

#[derive(Debug)]
pub struct Spinor {
    conn: CID,
    token: [u32; 4],
}
impl Spinor {
    #[cfg(test)]
    pub fn new(_: &xous_names::XousNames) -> Self { Spinor { conn: 0, token: [0, 0, 0, 0] } }

    #[cfg(not(test))]
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(api::SERVER_NAME_SPINOR).expect("Can't connect to Spinor server");

        let trng = trng::Trng::new(&xns).expect("Can't connect to TRNG servere");
        Ok(Spinor {
            conn,
            token: [
                trng.get_u32().unwrap(),
                trng.get_u32().unwrap(),
                trng.get_u32().unwrap(),
                trng.get_u32().unwrap(),
            ],
        })
    }

    /// this returns the minimum alignment for an erase block. `write` and `erase` operations
    /// benefit in performance if the requests are aligned to this number.
    pub fn erase_alignment(&self) -> u32 { SPINOR_ERASE_SIZE }

    /// this function needs to be called by the "one true" authorized updater for the SOC gateware region
    /// it must be called very early in the boot process, while we are still in the fully trusted code zone
    /// This registers that server as the only server which is authorized to request a patch to the SOC
    /// gateware region. later on, anyone is welcome to try to call it; it will have no effect.
    ///
    /// Note to self: because we don't have the SOC updater written, this token is curretnly occupied by
    /// keys.rs in the shellchat command. Later on, we will want to move this to the final server, once it
    /// is written.
    pub fn register_soc_token(&self) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_scalar(
                Opcode::RegisterSocToken.to_usize().unwrap(),
                self.token[0] as usize,
                self.token[1] as usize,
                self.token[2] as usize,
                self.token[3] as usize,
            ),
        )
        .map(|_| ())
    }

    pub fn set_staging_write_protect(&self, protect: bool) -> Result<(), xous::Error> {
        if protect {
            send_message(
                self.conn,
                Message::new_scalar(
                    Opcode::SetStagingWriteProtect.to_usize().unwrap(),
                    self.token[0] as usize,
                    self.token[1] as usize,
                    self.token[2] as usize,
                    self.token[3] as usize,
                ),
            )
            .map(|_| ())
        } else {
            send_message(
                self.conn,
                Message::new_scalar(
                    Opcode::ClearStagingWriteProtect.to_usize().unwrap(),
                    self.token[0] as usize,
                    self.token[1] as usize,
                    self.token[2] as usize,
                    self.token[3] as usize,
                ),
            )
            .map(|_| ())
        }
    }

    #[cfg(not(test))]
    fn send_write_region(&self, wr: &WriteRegion) -> Result<(), SpinorError> {
        let mut buf = Buffer::into_buf(*wr).or(Err(SpinorError::IpcError))?;
        buf.lend_mut(self.conn, Opcode::WriteRegion.to_u32().unwrap()).or(Err(SpinorError::IpcError))?;

        match buf.to_original::<WriteRegion, _>() {
            Ok(wr) => {
                if let Some(res) = wr.result {
                    match res {
                        SpinorError::NoError => Ok(()),
                        _ => Err(res),
                    }
                } else {
                    Err(SpinorError::ImplementationError)
                }
            }
            _ => Err(SpinorError::ImplementationError),
        }
    }

    #[cfg(test)]
    fn send_write_region(&self, wr: &WriteRegion) -> Result<(), SpinorError> {
        let mut i = 0;
        if !wr.clean_patch {
            assert!(
                (wr.start & 0xFFF) == 0,
                "erasing is required, but start address is not erase-sector aligned"
            );
            for addr in wr.start..wr.start + 4096 {
                EMU_FLASH.lock().unwrap()[addr as usize] = 0xFF;
            }
        }
        for addr in wr.start..wr.start + wr.len {
            assert!(
                EMU_FLASH.lock().unwrap()[addr as usize] == 0xFF,
                "attempt to write memory that's not erased"
            );
            EMU_FLASH.lock().unwrap()[addr as usize] = wr.data[i];
            i += 1;
        }
        Ok(())
    }

    #[cfg(not(test))]
    fn send_bulk_erase(&self, be: &BulkErase) -> Result<(), SpinorError> {
        let mut buf = Buffer::into_buf(*be).or(Err(SpinorError::IpcError))?;
        buf.lend_mut(self.conn, Opcode::BulkErase.to_u32().unwrap()).or(Err(SpinorError::IpcError))?;

        match buf.to_original::<BulkErase, _>() {
            Ok(wr) => {
                if let Some(res) = wr.result {
                    match res {
                        SpinorError::NoError => Ok(()),
                        _ => Err(res),
                    }
                } else {
                    Err(SpinorError::ImplementationError)
                }
            }
            _ => Err(SpinorError::ImplementationError),
        }
    }

    #[cfg(test)]
    fn send_bulk_erase(&self, be: &BulkErase) -> Result<(), SpinorError> {
        let mut i = 0;
        for addr in be.start..be.start + be.len {
            EMU_FLASH.lock().unwrap()[addr as usize] = 0xFF;
            i += 1;
        }
        Ok(())
    }

    /// `bulk_erase` is a function to be used fairly rarely, as it will erase just about anything and
    /// everything and it requires a 64k-alignment for the start and len arguments. It's a bit too coarse
    /// a hammer to be used in many functions, and confers little performance benefit when erasing fewer
    /// than a couple megabytes of data. The main intended use of this is for PDDB-region bulk erase,
    /// which is about 100MiB and this should reduce the erase time by about 20-30% instead of using the
    /// `patch` function to patch a blank sector. The current implementation enforces the bounds of
    /// `bulk_erase` to be within the PDDB region with a check on the server side.
    ///
    /// Also note that the `start` address is given as an offset from the start of FLASH, and not as an
    /// absolute memory address.
    pub fn bulk_erase(&self, start: u32, len: u32) -> Result<(), SpinorError> {
        if (start & (SPINOR_BULK_ERASE_SIZE - 1)) != 0 {
            return Err(SpinorError::AlignmentError);
        }
        if (len & (SPINOR_BULK_ERASE_SIZE - 1)) != 0 {
            return Err(SpinorError::AlignmentError);
        }
        // acquire a write lock on the unit
        #[cfg(not(test))]
        {
            const RETRY_LIMIT: usize = 5;
            for i in 0..RETRY_LIMIT {
                let response = send_message(
                    self.conn,
                    Message::new_blocking_scalar(
                        Opcode::AcquireExclusive.to_usize().unwrap(),
                        self.token[0] as usize,
                        self.token[1] as usize,
                        self.token[2] as usize,
                        self.token[3] as usize,
                    ),
                )
                .expect("couldn't send AcquireExclusive message to Spinor hardware!");
                if let xous::Result::Scalar1(result) = response {
                    if result == 0 {
                        if i == RETRY_LIMIT - 1 {
                            return Err(SpinorError::BusyTryAgain);
                        }
                        xous::yield_slice();
                    } else {
                        break;
                    }
                }
            }
        }
        let be = BulkErase { id: self.token, start, len, result: None };
        let ret = self.send_bulk_erase(&be);
        // release the write lock before exiting
        #[cfg(not(test))]
        let _ = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ReleaseExclusive.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send ReleaseExclusive message");
        ret
    }

    /// `patch` is an extremely low-level function that can patch data on FLASH. Access control must be
    /// enforced by higher level     abstractions. However, there is a single sanity check on the server
    /// side that prevents rogue writes to the SoC gateware area.     Aside from that, it's the wild west!
    /// Think of this as `dd` into a raw disk device node, and not a filesystem-abstracted `write`
    /// `region` is a slice that points to the target region that we wish to patch -- ostensibly we should
    /// have access to it, so this should     just be the MemoryRange turned into a slice.
    /// `region_base` is the physical base address of `region`, given as an offset from base of FLASH (that
    /// is, physical address minus 0x2000_0000)     this *must* be aligned to an erase sector.
    /// `patch_data` is a slice that contains exactly the data we want to have patched into FLASH. If you're
    /// lazy and you just send a large     amount of unchanged data with a couple small changes scattered
    /// about, this will not be "smart" about it and do a diff and     optimally patch sub-regions. It
    /// will erase and re-write the entire region included in the patch. It must also be a multiple     of
    /// two u8s in length, because the DDR interface can only tranfer even multiples of bytes.
    /// `patch_index` is the offset relative to the base address of `region` to patch. It is *not* relative to
    /// the base of FLASH,   it is relative to the base of `region`. You can think of it as the index into
    /// the `region` slice where the patch data should go.   patch_index /can/ have an odd byte offset,
    /// but it will still need to write at a minimum two bytes of data starting at the odd   byte offset.
    ///  Notes:
    ///    - the server will entirely skip writing over 256-byte pages that are blank. So, if the goal is to
    ///      erase a region, call patch with data of all 0xFF - this will effectively only do an erase, but no
    ///      subsequent writes.
    pub fn patch(
        &self,
        region: &[u8],
        region_base: u32,
        patch_data: &[u8],
        patch_index: u32,
    ) -> Result<(), SpinorError> {
        let align_mask = self.erase_alignment() - 1;
        if (region_base & align_mask) != 0 {
            return Err(SpinorError::AlignmentError);
        }
        if patch_data.len() % 2 != 0 {
            return Err(SpinorError::AlignmentError);
        }
        if patch_index % 2 != 0 {
            // seems in DDR mode, the alignment must be to 16-bit boundaries
            return Err(SpinorError::AlignmentError);
        }
        // acquire a write lock on the unit
        #[cfg(not(test))]
        {
            const RETRY_LIMIT: usize = 5;
            for i in 0..RETRY_LIMIT {
                let response = send_message(
                    self.conn,
                    Message::new_blocking_scalar(
                        Opcode::AcquireExclusive.to_usize().unwrap(),
                        self.token[0] as usize,
                        self.token[1] as usize,
                        self.token[2] as usize,
                        self.token[3] as usize,
                    ),
                )
                .expect("couldn't send AcquireExclusive message to Spinor hardware!");
                if let xous::Result::Scalar1(result) = response {
                    if result == 0 {
                        if i == RETRY_LIMIT - 1 {
                            return Err(SpinorError::BusyTryAgain);
                        }
                        xous::yield_slice();
                    } else {
                        break;
                    }
                }
            }
        }

        // pre-allocate a buffer that we'll use repeatedly to communicate with the server
        let mut wr = WriteRegion {
            id: self.token,
            start: 0,
            data: [0xFF; 4096],
            len: 0,
            result: None,
            clean_patch: false,
        };

        // snap the patch index to the next nearest lower erase block boundary
        let patch_index_aligned = patch_index & !align_mask;
        let patch_pre_pad = patch_index - patch_index_aligned;
        // add the alignment difference to the total length
        let mut patch_len_aligned = patch_data.len() as u32 + patch_pre_pad;
        // if the total length is not aligned to an erase block, add the padding up until the next erase block
        if patch_len_aligned & align_mask != 0 {
            patch_len_aligned += self.erase_alignment() - (patch_len_aligned & align_mask);
        }

        let mut cur_index = patch_index_aligned;
        let mut cur_patch_index = 0;
        let mut ret: Result<(), SpinorError> = Ok(());
        for sector in region[patch_index_aligned as usize..(patch_index_aligned + patch_len_aligned) as usize]
            .chunks_exact(self.erase_alignment() as usize)
            .into_iter()
        {
            // we get chunks instead of chunks_exact() as we /want/ to catch errors in computing alignments
            assert!(sector.len() as u32 == self.erase_alignment(), "alignment masks not computed correctly");

            // check to see if we can just write, without having to erase:
            //   - visit every value in the region to be patched, and if it's not already 0xFF, short-circuit
            //     and move to the erase-then-write implementation
            //   - as we check every value, copy them to the write buffer; we may end up discarding that,
            //     though, if we come across an unerased value. that's ok, still faster than always doing an
            //     erase then write on average
            let mut check_index = cur_index; // copy the writing index to a temporary checking index
            let mut check_patch_index = cur_patch_index; // copy the patching index to a temporary patch checking index
            let mut data_index = 0;
            let mut erased = true;
            let mut patch_start: Option<u32> = None;
            let mut patch_dirty = false;
            for &rom_byte in sector.iter() {
                if !((check_index < patch_index) || (check_index >= (patch_index + patch_data.len() as u32)))
                {
                    if rom_byte != patch_data[check_patch_index as usize] {
                        patch_dirty = true;
                    }
                    if rom_byte != 0xFF {
                        erased = false;
                        break; // skip this loop and go to the full-sector replace version
                    }
                    if patch_start.is_none() {
                        patch_start = Some(check_index + region_base);
                    }
                    // we're actually in a data region to be patched
                    wr.data[data_index] = patch_data[check_patch_index as usize];
                    data_index += 1;
                    check_patch_index += 1;
                }
                check_index += 1;
            }

            if erased && patch_dirty {
                wr.clean_patch = true;
                wr.start = patch_start
                    .expect("check region did not intersect patch region; this shouldn't be possible.");
                wr.len = data_index as u32;
                ret = self.send_write_region(&wr);
                if ret.is_err() {
                    break;
                }
                cur_index = check_index;
                cur_patch_index = check_patch_index;
            } else {
                // the sector needs erasing first, assemble the primitives accordingly:
                // load WriteRegion with one sector of data:
                //   - copy from original region for pre-pad data
                //   - if we're in the patch region, copy the patch_data
                //   - after the patch region, copy the pre-pad data
                wr.start = cur_index + region_base;
                let mut dirty = false;
                let mut first_patch_addr: Option<u32> = None;
                for (&src, dst) in sector.iter().zip(wr.data.iter_mut()) {
                    if (cur_index < patch_index) || (cur_index >= (patch_index + patch_data.len() as u32)) {
                        *dst = src;
                    } else {
                        if src != patch_data[cur_patch_index as usize] {
                            dirty = true;
                            if first_patch_addr.is_none() {
                                first_patch_addr = Some(cur_index + region_base);
                            }
                        }
                        *dst = patch_data[cur_patch_index as usize];
                        cur_patch_index += 1;
                    }
                    cur_index += 1;
                }
                wr.len = sector.len() as u32;
                wr.result = None;
                wr.clean_patch = false;
                assert!(wr.start & align_mask == 0, "write became misaligned");

                // if the requested patch data happens to be identical to the existing data already, don't
                // even send the request.
                if dirty {
                    ret = self.send_write_region(&wr);
                    if ret.is_err() {
                        break; // abort fast if we encounter an error
                    }
                }
            }
        }

        // release the write lock before exiting
        #[cfg(not(test))]
        let _ = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ReleaseExclusive.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send ReleaseExclusive message");

        ret
    }

    /// these functions are intended for use by the suspend/resume manager. most functions wouldn't have a
    /// need to call this.
    pub fn acquire_suspend_lock(&self) -> Result<bool, xous::Error> {
        let response = send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::AcquireSuspendLock.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("Couldn't issue AcquireSuspendLock message");
        if let xous::Result::Scalar1(result) = response {
            if result != 0 { Ok(true) } else { Ok(false) }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn release_suspend_lock(&self) -> Result<(), xous::Error> {
        // we ignore the result and just turn it into () once we get anything back, as release_suspend "can't
        // fail"
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::ReleaseSuspendLock.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .map(|_| ())
    }
}

use core::{
    sync::atomic::{AtomicU32, Ordering},
    u8,
};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(not(test))]
impl Drop for Spinor {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this
        // object within a single process do not end up de-allocating the CID on other threads before
        // they go out of scope. Note to future me: you want this. Don't get rid of it because you
        // think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to
        // the object instance), de-allocate those items here. They don't need a reference count
        // because they are object-specific
    }
}

#[cfg(test)]
use core::sync::atomic::AtomicU64;

#[cfg(test)]
static TEST_RNG_STATE: AtomicU64 = AtomicU64::new(4);

// run with `cargo test -- --nocapture --test-threads=1`:
//   --nocapture to see the print output (while debugging)
//   --test-threads=1 because the FLASH ROM is a shared state object
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construction() {
        let spinor = Spinor::new();
        assert!(spinor.erase_alignment() == SPINOR_ERASE_SIZE);
    }

    #[test]
    fn test_send_write_region() {
        // test that the basic flash emulation layer does what we expect it to do
        init_emu_flash(5);

        // check "clean patching"
        let mut wr = WriteRegion {
            id: [0, 0, 0, 0],
            start: 8,
            clean_patch: true,
            data: [0; 4096],
            len: 4,
            result: None,
        };
        wr.data[0] = 0xAA;
        wr.data[1] = 0xBB;
        wr.data[2] = 0xCC;
        wr.data[3] = 0xDD;
        let mut spinor = Spinor::new();
        let ret = spinor.send_write_region(&wr);
        assert!(ret.is_ok(), "spinor write returned an error");

        for (addr, &byte) in EMU_FLASH.lock().unwrap().iter().enumerate() {
            // we should see 0xAA 0xBB 0xCC 0xDD from addresses 8-12, everyhing else as 0xFF
            if addr < 8 || addr >= 12 {
                assert!(byte == 0xFF, "non-erased byte when erasure expected: {:08x} : {:02x}", addr, byte);
            } else {
                match addr {
                    8 => assert!(byte == 0xAA, "wrong patch value, expected 0xAA got {:02x}", byte),
                    9 => assert!(byte == 0xBB, "wrong patch value, expected 0xBB got {:02x}", byte),
                    10 => assert!(byte == 0xCC, "wrong patch value, expected 0xCC got {:02x}", byte),
                    11 => assert!(byte == 0xDD, "wrong patch value, expected 0xDD got {:02x}", byte),
                    _ => assert!(false, "test is constructed incorrectly, should not reach this statement"),
                }
            }
        }

        // now check erasing-then-writing
        wr.clean_patch = false;
        wr.start = 0;
        for d in wr.data.iter_mut() {
            *d = 0xFF;
        }
        wr.data[6] = 0x1;
        wr.data[7] = 0x2;
        wr.data[8] = 0x3;
        wr.data[9] = 0x4;
        wr.len = 10;
        let ret = spinor.send_write_region(&wr);
        assert!(ret.is_ok(), "spinor write returned an error");

        for (addr, &byte) in EMU_FLASH.lock().unwrap().iter().enumerate() {
            // we should see 1, 2, 3, 4 from addresses 6-10, everyhing else as 0xFF
            if addr < 6 || addr >= 10 {
                assert!(byte == 0xFF, "non-erased byte when erasure expected: {:08x} : {:02x}", addr, byte);
            } else {
                match addr {
                    6 => assert!(byte == 1, "wrong patch value, expected 1 got {:02x}", byte),
                    7 => assert!(byte == 2, "wrong patch value, expected 2 got {:02x}", byte),
                    8 => assert!(byte == 3, "wrong patch value, expected 3 got {:02x}", byte),
                    9 => assert!(byte == 4, "wrong patch value, expected 4 got {:02x}", byte),
                    _ => assert!(false, "test is constructed incorrectly, should not reach this statement"),
                }
            }
        }
    }

    fn init_emu_flash(sectors: usize) {
        EMU_FLASH.lock().unwrap().clear();
        for _ in 0..sectors * 4096 {
            EMU_FLASH.lock().unwrap().push(0xFF);
        }
    }
    fn flash_fill_rand() {
        use rand::prelude::*;
        use rand_chacha::ChaCha8Rng;
        let mut rng = ChaCha8Rng::seed_from_u64(
            TEST_RNG_STATE.load(Ordering::SeqCst)
                + xous::TESTING_RNG_SEED.load(core::sync::atomic::Ordering::SeqCst),
        );
        for byte in EMU_FLASH.lock().unwrap().iter_mut() {
            *byte = rng.gen::<u8>();
        }
        TEST_RNG_STATE.store(rng.next_u64(), Ordering::SeqCst);
    }

    /*
    ALL RIGHT! i came to chew gum and write some tests, and they don't allow chewing gum in Singapore. So let's DO EEEEET!!!
    */
    /*
    cases to check:
       - mis-aligned start address
       - mis-aligned end address
       - mis-aligned start and end address
       - aligned start and end address

       - patch of data into a mostly pre-erased sector with existing data, but into the already-erased section
       - the above, but where the patch length goes beyond the length of a single sector
     */
    //     pub fn patch(&mut self, region: &[u8], region_base: u32, patch_data: &[u8], patch_index: u32) ->
    // Result<(), SpinorError> {

    #[test]
    fn test_aligned_start_aligned_end() {
        let mut spinor = Spinor::new();
        init_emu_flash(8);
        flash_fill_rand();

        // create our "flash region" -- normally this would be a memory-mapped block that's owned by our
        // calling function here we bodge it out of the emulated flash space with some rough parameter
        let mut flash_orig = Vec::<u8>::new();
        flash_orig.extend(EMU_FLASH.lock().unwrap().as_slice().iter().copied()); // snag a copy of the original state, so we can be sure the patch was targeted

        let region_base = 0x1000; // region bases are required to be aligned to an erase sector; guaranteed by fiat
        let region_len = 0x1000 * 4; // four sectors long, one sector up
        let region = &flash_orig[region_base as usize..(region_base + region_len) as usize];

        // exactly one sector of patch data
        let mut patch: [u8; 4096] = [0; 4096];
        for (i, p) in patch.iter_mut().enumerate() {
            *p = i as u8;
        }

        // patch region base + index = 0x2000 with 0x1000 of data
        let result = spinor.patch(region, region_base, &patch, 0x1000);
        assert!(result.is_ok(), "patch threw an error");

        for (addr, (&patched, &orig)) in EMU_FLASH.lock().unwrap().iter().zip(flash_orig.iter()).enumerate() {
            // we should see a repeating pattern of 0, 1, 2... from 0x2000-0x3000
            if addr < 0x2000 || addr >= 0x3000 {
                assert!(patched == orig, "data disturbed: {:08x} : e.{:02x} a.{:02x}", addr, orig, patched); // e = expected, a = actual
            } else {
                assert!(
                    patched == addr as u8,
                    "data was not patched: {:08x} : e.{:02x} a.{:02x}",
                    addr,
                    addr as u8,
                    patched
                );
            }
        }
    }

    #[test]
    fn test_small_patch() {
        let mut spinor = Spinor::new();
        init_emu_flash(8);
        // flash_fill_rand();

        // create our "flash region" -- normally this would be a memory-mapped block that's owned by our
        // calling function here we bodge it out of the emulated flash space with some rough parameter
        let mut flash_orig = Vec::<u8>::new();
        flash_orig.extend(EMU_FLASH.lock().unwrap().as_slice().iter().copied()); // snag a copy of the original state, so we can be sure the patch was targeted

        let region_base = 0x1000; // region bases are required to be aligned to an erase sector; guaranteed by fiat
        let region_len = 0x1000 * 4; // four sectors long, one sector up
        let region = &flash_orig[region_base as usize..(region_base + region_len) as usize];

        // patch two words, the minimum allowed amount
        let patch: [u8; 2] = [0x33, 0xCC];

        // patch region base + index = 0x2704 with two bytes of data
        print!("{:x?}", &EMU_FLASH.lock().unwrap()[0x2700..0x2708]);
        let result = spinor.patch(region, region_base, &patch, 0x1704);
        assert!(result.is_ok(), "patch threw an error");

        for (addr, (&patched, &orig)) in EMU_FLASH.lock().unwrap().iter().zip(flash_orig.iter()).enumerate() {
            match addr {
                0x2704 => assert!(
                    patched == 0x33,
                    "data was not patched: {:08x} : e.{:02x} a.{:02x}",
                    addr,
                    0x33,
                    patched
                ),
                0x2705 => assert!(
                    patched == 0xCC,
                    "data was not patched: {:08x} : e.{:02x} a.{:02x}",
                    addr,
                    0xCC,
                    patched
                ),
                _ => assert!(
                    patched == orig,
                    "data disturbed: {:08x} : e.{:02x} a.{:02x}",
                    addr,
                    orig,
                    patched
                ),
            }
        }
        print!("{:x?}", &EMU_FLASH.lock().unwrap()[0x2700..0x2708]);
    }

    #[test]
    fn test_misaligned_start_end_full_sector() {
        let mut spinor = Spinor::new();
        init_emu_flash(8);
        flash_fill_rand();

        // create our "flash region" -- normally this would be a memory-mapped block that's owned by our
        // calling function here we bodge it out of the emulated flash space with some rough parameter
        let mut flash_orig = Vec::<u8>::new();
        flash_orig.extend(EMU_FLASH.lock().unwrap().as_slice().iter().copied()); // snag a copy of the original state, so we can be sure the patch was targeted

        let region_base = 0x1000; // region bases are required to be aligned to an erase sector; guaranteed by fiat
        let region_len = 0x1000 * 4; // four sectors long, one sector up
        let region = &flash_orig[region_base as usize..(region_base + region_len) as usize];

        // patch one full page
        let mut patch: [u8; 4096] = [0; 4096];
        for (i, p) in patch.iter_mut().enumerate() {
            *p = i as u8;
        }

        // patch region base + index = 0x2704 with one full page of data
        print!("{:x?}", &EMU_FLASH.lock().unwrap()[0x2700..0x2708]);
        let result = spinor.patch(region, region_base, &patch, 0x1704);
        assert!(result.is_ok(), "patch threw an error");

        let mut i = 0;
        for (addr, (&patched, &orig)) in EMU_FLASH.lock().unwrap().iter().zip(flash_orig.iter()).enumerate() {
            if addr < 0x2704 || addr >= 0x3704 {
                assert!(patched == orig, "data disturbed: {:08x} : e.{:02x} a.{:02x}", addr, orig, patched);
            } else {
                assert!(
                    patched == i as u8,
                    "data was not patched: {:08x} : e.{:02x} a.{:02x}",
                    addr,
                    i as u8,
                    patched
                );
                i += 1;
            }
        }
        print!("{:x?}", &EMU_FLASH.lock().unwrap()[0x2700..0x2708]);
    }

    #[test]
    fn test_misaligned_start_partial_sector() {
        let mut spinor = Spinor::new();
        init_emu_flash(8);
        flash_fill_rand();

        // create our "flash region" -- normally this would be a memory-mapped block that's owned by our
        // calling function here we bodge it out of the emulated flash space with some rough parameter
        let mut flash_orig = Vec::<u8>::new();
        flash_orig.extend(EMU_FLASH.lock().unwrap().as_slice().iter().copied()); // snag a copy of the original state, so we can be sure the patch was targeted

        let region_base = 0x1000; // region bases are required to be aligned to an erase sector; guaranteed by fiat
        let region_len = 0x1000 * 4; // four sectors long, one sector up
        let region = &flash_orig[region_base as usize..(region_base + region_len) as usize];

        // patch over a page boundary, but not a full page
        let mut patch: [u8; 2304] = [0; 2304];
        for (i, p) in patch.iter_mut().enumerate() {
            *p = i as u8;
        }

        // patch region base + index = 0x2704 with two bytes of data
        let result = spinor.patch(region, region_base, &patch, 0x1704);
        assert!(result.is_ok(), "patch threw an error");

        let mut i = 0;
        for (addr, (&patched, &orig)) in EMU_FLASH.lock().unwrap().iter().zip(flash_orig.iter()).enumerate() {
            match addr {
                0x2704..=0x3003 => {
                    assert!(
                        patched == i as u8,
                        "data was not patched: {:08x} : e.{:02x} a.{:02x}",
                        addr,
                        0x33,
                        patched
                    );
                    i += 1;
                }
                _ => assert!(
                    patched == orig,
                    "data disturbed: {:08x} : e.{:02x} a.{:02x}",
                    addr,
                    orig,
                    patched
                ),
            }
        }
    }

    #[test]
    fn test_aligned_start_partial_sector() {
        let mut spinor = Spinor::new();
        init_emu_flash(8);
        flash_fill_rand();

        // create our "flash region" -- normally this would be a memory-mapped block that's owned by our
        // calling function here we bodge it out of the emulated flash space with some rough parameter
        let mut flash_orig = Vec::<u8>::new();
        flash_orig.extend(EMU_FLASH.lock().unwrap().as_slice().iter().copied()); // snag a copy of the original state, so we can be sure the patch was targeted

        let region_base = 0x1000; // region bases are required to be aligned to an erase sector; guaranteed by fiat
        let region_len = 0x1000 * 4; // four sectors long, one sector up
        let region = &flash_orig[region_base as usize..(region_base + region_len) as usize];

        // patch over a page boundary, but not a full page
        let mut patch: [u8; 578] = [0; 578];
        for (i, p) in patch.iter_mut().enumerate() {
            *p = i as u8;
        }

        // patch region base + index = 0x2000 with 578 bytes of data
        let result = spinor.patch(region, region_base, &patch, 0x1000);
        assert!(result.is_ok(), "patch threw an error");

        let mut i = 0;
        for (addr, (&patched, &orig)) in EMU_FLASH.lock().unwrap().iter().zip(flash_orig.iter()).enumerate() {
            match addr {
                0x2000..=0x2241 => {
                    assert!(
                        patched == i as u8,
                        "data was not patched: {:08x} : e.{:02x} a.{:02x}",
                        addr,
                        0x33,
                        patched
                    );
                    i += 1;
                }
                _ => assert!(
                    patched == orig,
                    "data disturbed: {:08x} : e.{:02x} a.{:02x}",
                    addr,
                    orig,
                    patched
                ),
            }
        }
    }

    #[test]
    fn test_patch_edge() {
        let mut spinor = Spinor::new();
        init_emu_flash(8);
        flash_fill_rand();
        // poke a small "erased" region for patching in this test: exactly the right size for the anticipated
        // patch of 384 bytes
        for byte in EMU_FLASH.lock().unwrap()[0x1F00..0x2080].iter_mut() {
            *byte = 0xFF;
        }

        // create our "flash region" -- normally this would be a memory-mapped block that's owned by our
        // calling function here we bodge it out of the emulated flash space with some rough parameter
        let mut flash_orig = Vec::<u8>::new();
        flash_orig.extend(EMU_FLASH.lock().unwrap().as_slice().iter().copied()); // snag a copy of the original state, so we can be sure the patch was targeted

        let region_base = 0x1000; // region bases are required to be aligned to an erase sector; guaranteed by fiat
        let region_len = 0x1000 * 4; // four sectors long, one sector up
        let region = &flash_orig[region_base as usize..(region_base + region_len) as usize];

        // patch over a page boundary, but not a full page
        let mut patch: [u8; 384] = [0; 384];
        for (i, p) in patch.iter_mut().enumerate() {
            *p = i as u8;
        }

        /*
        let mut blank = true;
        for page in patch[0..patch.len()].chunks(256) {
            for word in page.chunks(2) {
                let wdata = word[0] as u32 | ((word[1] as u32) << 8);
                if wdata != 0xFFFF {
                    blank = false;
                    break;
                }
            }
            for word in page.chunks(2) {
                let wdata = word[0] as u32 | ((word[1] as u32) << 8);
                print!("0x{:04x} ", wdata);
            }
            println!("");
        }
        */
        // patch region base + index = 0x1F00 with 384 bytes of data (to 0x2080)
        print!("{:x?}", &EMU_FLASH.lock().unwrap()[0x1EFC..0x1F04]);
        print!("{:x?}", &EMU_FLASH.lock().unwrap()[0x207C..0x2084]);
        let result = spinor.patch(region, region_base, &patch, 0xF00);
        assert!(result.is_ok(), "patch threw an error");

        let mut i = 0;
        for (addr, (&patched, &orig)) in EMU_FLASH.lock().unwrap().iter().zip(flash_orig.iter()).enumerate() {
            match addr {
                0x1F00..=0x207F => {
                    assert!(
                        patched == i as u8,
                        "data was not patched: {:08x} : e.{:02x} a.{:02x}",
                        addr,
                        0x33,
                        patched
                    );
                    i += 1;
                }
                _ => assert!(
                    patched == orig,
                    "data disturbed: {:08x} : e.{:02x} a.{:02x}",
                    addr,
                    orig,
                    patched
                ),
            }
        }
        print!("{:x?}", &EMU_FLASH.lock().unwrap()[0x1EFC..0x1F04]);
        print!("{:x?}", &EMU_FLASH.lock().unwrap()[0x207C..0x2084]);
    }

    #[test]
    fn test_patch_csr_area() {
        let mut spinor = Spinor::new();
        init_emu_flash(640);
        flash_fill_rand();
        // emulate the "erased" region at the end of the CSR file
        for byte in EMU_FLASH.lock().unwrap()[0x27b200..0x27f000].iter_mut() {
            *byte = 0xFF;
        }

        // create our "flash region" -- normally this would be a memory-mapped block that's owned by our
        // calling function here we bodge it out of the emulated flash space with some rough parameter
        let mut flash_orig = Vec::<u8>::new();
        flash_orig.extend(EMU_FLASH.lock().unwrap().as_slice().iter().copied()); // snag a copy of the original state, so we can be sure the patch was targeted

        let region_base = 0x0; // region bases are required to be aligned to an erase sector; guaranteed by fiat
        let region_len = 0x1000 * 28; // four sectors long, one sector up

        let mut patch = flash_orig.clone();
        for i in 0x27efc0..0x27f000 {
            patch[i] = 0x66;
        }

        print!("source {:x?}\n", &patch[0x27efc0..0x27f000]);
        print!("target {:x?}\n", &EMU_FLASH.lock().unwrap()[0x27efc0..0x27f000]);
        print!("wrong  {:x?}\n", &EMU_FLASH.lock().unwrap()[0x27e000..0x27e100]);
        let result = spinor.patch(&flash_orig, region_base, &patch[0x27_6000..0x27_f000], 0x27_6000);
        assert!(result.is_ok(), "patch threw an error");

        print!("source {:x?}\n", &patch[0x27efc0..0x27f000]);
        print!("target {:x?}\n", &EMU_FLASH.lock().unwrap()[0x27efc0..0x27f000]);
        print!("wrong  {:x?}\n", &EMU_FLASH.lock().unwrap()[0x27e000..0x27e100]);
    }
}
