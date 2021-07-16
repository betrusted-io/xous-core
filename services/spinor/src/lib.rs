#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;

use xous::{CID, send_message, Message};
use num_traits::*;
use xous_ipc::Buffer;

pub struct Spinor {
    conn: CID,
    token: [u32; 4],
}
impl Spinor {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_SPINOR).expect("Can't connect to Spinor server");

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
    pub fn erase_alignment(&self) -> u32 {
        SPINOR_ERASE_SIZE
    }

    /// this function needs to be called by the "one true" authorized updater for the SOC gateware region
    /// it must be called very early in the boot process, while we are still in the fully trusted code zone
    /// This registers that server as the only server which is authorized to request a patch to the SOC gateware region.
    /// later on, anyone is welcome to try to call it; it will have no effect.
    pub fn register_soc_token(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::RegisterSocToken.to_usize().unwrap(),
            self.token[0] as usize,
            self.token[1] as usize,
            self.token[2] as usize,
            self.token[3] as usize,
        )).map(|_| ())
    }

    fn send_write_region(&mut self, wr: &WriteRegion) -> Result<(), SpinorError> {
        let mut buf = Buffer::into_buf(*wr).or(Err(SpinorError::IpcError))?;
        buf.lend_mut(self.conn, Opcode::WriteRegion.to_u32().unwrap()).or(Err(SpinorError::IpcError))?;

        match buf.to_original::<WriteRegion, _>() {
            Ok(wr) => {
                if let Some(res) = wr.result {
                    match res {
                        SpinorError::NoError => Ok(()),
                        _ => Err(res)
                    }
                } else {
                    Err(SpinorError::ImplementationError)
                }
            }
            _ => Err(SpinorError::ImplementationError)
        }
    }

    /*
    cases to check:
       - mis-aligned start address
       - mis-aligned end address
       - mis-aligned start and end address
       - aligned start and end address

       - patch of data into a mostly pre-erased sector with existing data, but into the already-erased section
       - the above, but where the patch length goes beyond the length of a single sector
     */

    /// `patch` is an extremely low-level function that can patch data on FLASH. Access control must be enforced by higher level
    ///     abstractions. However, there is a single sanity check on the server side that prevents rogue writes to the SoC gateware area.
    ///     Aside from that, it's the wild west! Think of this as `dd` into a raw disk device node, and not a filesystem-abstracted `write`
    /// `region` is a slice that points to the ostensible region that we have access to, and wish to patch
    /// `region_base` is the base address of `region`, given as an offset from base of FLASH (that is, physical address minus 0x2000_0000)
    ///     this *must* be aligned to an erase sector.
    /// `patch_data` is a slice that contains exactly the data we want to have patched into FLASH. If you're lazy and you just send a large
    ///     amount of unchanged data with a couple small changes scattered about, this will not be "smart" about it and do a diff and
    ///     optimally patch sub-regions. It will erase and re-write the entire region included in the patch. It must also be a multiple
    ///     of two u8s in length, because the DDR interface can only tranfer even multiples of bytes.
    /// `patch_index` is the offset relative to the base address of `region` to patch. It is *not* relative to the base of FLASH,
    ///   it is relative to the base of `region`. You can think of it as the index into the `region` slice where the patch data should go.
    ///   patch_index /can/ have an odd byte offset, but it will still need to write at a minimum two bytes of data starting at the odd
    ///   byte offset.
    ///  Notes:
    ///    - the server will entirely skip writing over 256-byte pages that are blank. So, if the goal is to erase a region,
    ///      call patch with data of all 0xFF - this will effectively only do an erase, but no subsequent writes.
    pub fn patch(&mut self, region: &[u8], region_base: u32, patch_data: &[u8], patch_index: u32) -> Result<(), SpinorError> {
        let align_mask = self.erase_alignment() - 1;
        if (region_base & align_mask) != 0 {
            return Err(SpinorError::AlignmentError);
        }
        if patch_data.len() % 2 != 0 {
            return Err(SpinorError::AlignmentError);
        }
        // acquire a write lock on the unit
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AcquireExclusive.to_usize().unwrap(),
                self.token[0] as usize,
                self.token[1] as usize,
                self.token[2] as usize,
                self.token[3] as usize,
            )
        ).expect("couldn't send AcquireExclusive message to Sha2 hardware!");
        if let xous::Result::Scalar1(result) = response {
            if result == 0 {
                return Err(SpinorError::BusyTryAgain)
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
            patch_len_aligned += self.erase_alignment() - patch_len_aligned & align_mask;
        }

        let mut cur_index = patch_index_aligned;
        let mut cur_patch_index = 0;
        let mut ret: Result<(), SpinorError> = Ok(());
        for sector in region[patch_index_aligned as usize ..(patch_index_aligned + patch_len_aligned) as usize].chunks_exact(self.erase_alignment() as usize).into_iter() {
            // we get chunks instead of chunks_exact() as we /want/ to catch errors in computing alignments
            assert!(sector.len() as u32 == self.erase_alignment(), "alignment masks not computed correctly");

            // check to see if the region we're writing is already erased; if so, just send the data that needs patching
            let mut check_index = cur_index;
            let mut check_patch_index = cur_patch_index;
            let mut data_index = 0;
            let mut erased = true;
            let mut patch_start: Option<u32> = None;
            let mut patch_dirty = false;
            for &rom_byte in sector.iter() {
                if !((check_index < check_patch_index) || (check_index >= (check_patch_index + patch_data.len() as u32))) {
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
                wr.start = patch_start.expect("check region did not intersect patch region; this shouldn't be possible.");
                wr.len = data_index as u32;
                ret = self.send_write_region(&wr);
                if ret.is_err() {
                    break;
                }
                cur_index = check_index;
                cur_patch_index = check_patch_index;
            } else {
                // the sector needs erasing first, assemble the primitives accordingly

                wr.start = cur_index + region_base;
                // load WriteRegion with one sector of data:
                //   - copy from original region for pre-pad data
                //   - if we're in the patch region, copy the patch_data
                //   - after the patch region, copy the pre-pad data
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

                // if the requested patch data happens to be identical to the existing data already, don't even send
                // the request.
                if dirty {
                    ret = self.send_write_region(&wr);
                    if ret.is_err() {
                        break; // abort fast if we encounter an error
                    }
                }
            }
        }

        // release the write lock before exiting
        let _ = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::ReleaseExclusive.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't send ReleaseExclusive message");

        ret
    }

    /// these functions are intended for use by the suspend/resume manager. most functions wouldn't have a need to call this.
    pub fn acquire_suspend_lock(&self) -> Result<bool, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AcquireSuspendLock.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("Couldn't issue AcquireSuspendLock message");
        if let xous::Result::Scalar1(result) = response {
            if result != 0 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }
    pub fn release_suspend_lock(&self) -> Result<(), xous::Error> {
        // we ignore the result and just turn it into () once we get anything back, as release_suspend "can't fail"
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::ReleaseSuspendLock.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_| ())
    }
}

use core::{sync::atomic::{AtomicU32, Ordering}, u8};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Spinor {
    fn drop(&mut self) {
        // the connection to the server side must be reference counted, so that multiple instances of this object within
        // a single process do not end up de-allocating the CID on other threads before they go out of scope.
        // Note to future me: you want this. Don't get rid of it because you think, "nah, nobody will ever make more than one copy of this object".
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
        // if there was object-specific state (such as a one-time use server for async callbacks, specific to the object instance),
        // de-allocate those items here. They don't need a reference count because they are object-specific
    }
}