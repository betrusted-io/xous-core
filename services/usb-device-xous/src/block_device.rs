pub struct BlockDevice{}

impl usbd_scsi::BlockDevice for BlockDevice {
    const BLOCK_BYTES: usize = 512;

    fn read_block(&self, lba: u32, block: &mut [u8]) -> Result<(), usbd_scsi::BlockDeviceError> {
        Err(usbd_scsi::BlockDeviceError::HardwareError)
    }

    fn write_block(&mut self, lba: u32, block: &[u8]) -> Result<(), usbd_scsi::BlockDeviceError> {
        Err(usbd_scsi::BlockDeviceError::WriteError)
    }

    fn max_lba(&self) -> u32 {
        512
    }
}