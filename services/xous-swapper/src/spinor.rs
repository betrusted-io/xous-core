use num_traits::*;
use xous::{CID, Message, send_message};
use xous_ipc::Buffer;

use crate::api;
use crate::api::*;

#[derive(Debug)]
pub struct Spinor {
    conn: CID,
    token: [u32; 4],
}
impl Spinor {
    pub fn new(xns: &xous_api_names::XousNames) -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(api::SERVER_NAME_SPINOR).expect("Can't connect to Spinor server");

        // create_server_id() returns a random number from the kernel TRNG, with no other side effects
        Spinor { conn, token: xous::create_server_id().unwrap().to_array() }
    }

    /// this returns the minimum alignment for an erase block. `write` and `erase` operations
    /// benefit in performance if the requests are aligned to this number.
    pub fn erase_alignment(&self) -> u32 { cramium_hal::udma::spim::FLASH_PAGE_LEN as u32 }

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
        if (start & (cramium_hal::udma::spim::BLOCK_ERASE_LEN as u32 - 1)) != 0 {
            return Err(SpinorError::AlignmentError);
        }
        if (len & (cramium_hal::udma::spim::BLOCK_ERASE_LEN as u32 - 1)) != 0 {
            return Err(SpinorError::AlignmentError);
        }
        // acquire a write lock on the unit
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
        let be = BulkErase { id: self.token, start, len, result: None };
        let ret = self.send_bulk_erase(&be);
        // release the write lock before exiting
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
}

use core::{
    sync::atomic::{AtomicU32, Ordering},
    u8,
};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
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
