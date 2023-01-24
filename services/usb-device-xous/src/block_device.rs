use xous::MemoryRange;

const CAPACITY: usize = 256 * 1024; // must be a multiple of one page (4096)
pub struct BlockDevice{
    backing: MemoryRange,
}
impl BlockDevice {
    pub fn new() -> Self {
        let mut backing = xous::syscall::map_memory(
            None,
            None,
            CAPACITY,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        ).unwrap();
        let backing_slice: &mut [u32] = backing.as_slice_mut();
        for (index, d) in backing_slice.iter_mut().enumerate() {
            *d = index as u32;
        }
        BlockDevice { backing }
    }
}

impl usbd_scsi::BlockDevice for BlockDevice {
    const BLOCK_BYTES: usize = 512;

    fn read_block(&self, lba: u32, block: &mut [u8]) -> Result<(), usbd_scsi::BlockDeviceError> {
        let block_bytes = Self::BLOCK_BYTES;
        let backing_slice: &[u8] = self.backing.as_slice();
        block.copy_from_slice(
            &backing_slice[lba as usize * block_bytes..(lba as usize + 1) * block_bytes]
        );
        Ok(())
    }

    fn write_block(&mut self, lba: u32, block: &[u8]) -> Result<(), usbd_scsi::BlockDeviceError> {
        let block_bytes = Self::BLOCK_BYTES;
        let backing_slice: &mut [u8] = self.backing.as_slice_mut();
        backing_slice[lba as usize * block_bytes..(lba as usize + 1) * block_bytes].copy_from_slice(block);
        Ok(())
    }

    fn max_lba(&self) -> u32 {
        (CAPACITY / Self::BLOCK_BYTES) as u32 - 1
    }
}