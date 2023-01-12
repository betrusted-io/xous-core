use xous::MemoryRange;

pub struct BlockDevice{
    backing: MemoryRange,
}
impl BlockDevice {
    pub fn new() -> Self {
        let mut backing = xous::syscall::map_memory(
            None,
            None,
            512 * 1024,
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
        let backing_slice: &[u8] = self.backing.as_slice();
        block.copy_from_slice(
            &backing_slice[lba as usize * Self::BLOCK_BYTES..(lba as usize + 1) * Self::BLOCK_BYTES]
        );
        Ok(())
    }

    fn write_block(&mut self, lba: u32, block: &[u8]) -> Result<(), usbd_scsi::BlockDeviceError> {
        let backing_slice: &mut [u8] = self.backing.as_slice_mut();
        backing_slice[lba as usize * Self::BLOCK_BYTES..(lba as usize + 1) * Self::BLOCK_BYTES].copy_from_slice(block);
        Ok(())
    }

    fn max_lba(&self) -> u32 {
        1023
    }
}