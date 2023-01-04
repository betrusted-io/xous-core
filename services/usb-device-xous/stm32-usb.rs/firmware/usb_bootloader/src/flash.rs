use core::ops::RangeInclusive;
use usbd_scsi::BlockDeviceError;

pub trait Flash {
    /// Flash page size in bytes
    fn page_size(&self) -> u32;

    /// Valid address range for the flash
    fn address_range(&self) -> RangeInclusive<u32>;

    /// Mutable ref to the page buffer
    fn page_buffer(&mut self) -> &mut [u8];

    /// Last read page address
    fn current_page(&self) -> &Option<u32>;

    /// Unlock the flash for erasing/writing
    fn unlock_flash(&mut self) -> Result<(), BlockDeviceError>;

    /// Lock the flash to prevent erasing/writing
    fn lock_flash(&mut self) -> Result<(), BlockDeviceError>;

    /// Is the flash busy?
    fn is_operation_pending(&self) -> bool;

    /// Wait until is_operation_pending is false, no timeout implemented
    fn busy_wait(&self) {
        while self.is_operation_pending() {}
    }

    /// Erase the page at the given address
    ///
    /// Check the address is valid but don't check if erase is necessary, that's done in flush_page
    fn erase_page(&mut self, page_address: u32) -> Result<(), BlockDeviceError>;

    /// Check if the page is empty
    fn is_page_erased(&mut self, page_address: u32) -> bool;

    /// Read a whole page into page buffer
    ///
    /// Should check the address is a valid page and in range
    fn read_page(&mut self, page_address: u32) -> Result<(), BlockDeviceError>;

    /// Write a whole page from page buffer
    ///
    /// Implementor should probably read each half-word (or whatever the flash write size is) and
    /// compare it to the data being written before writing to reduce flash aging.
    fn write_page(&mut self) -> Result<(), BlockDeviceError>;

    /// Gets the page address for a given address
    fn page_address(&self, address: u32) -> u32 {
        address & !(self.page_size() - 1)
    }

    /// Save the current contents of the page buffer to flash at the address it was read from
    ///
    /// Check that erase and/or write are really necessary
    fn flush_page(&mut self) -> Result<(), BlockDeviceError>;

    /// Write the provided bytes to flash at the provided address
    ///
    /// Each touched page will be read into a buffer and flushed back to flash when the next page
    /// is modified or at the end of the function. 
    fn write_bytes(&mut self, address: u32, bytes: &[u8]) -> Result<(), BlockDeviceError> {
        let start_page = self.page_address(address);
        let end_page = self.page_address(address + bytes.len() as u32 - 1);
        let page_size = self.page_size() as usize;

        for page in (start_page..=end_page).step_by(page_size) {
            if let Some(cp) = self.current_page() {
                // If there's a page in the buffer and it's not the current one, flush it
                if *cp != page {
                    self.flush_page()?;
                    self.read_page(page)?;
                }
            } else {
                self.read_page(page)?;
            }

            if page < address {
                let offset = (address - page) as usize;
                let count = (page_size - offset).min(bytes.len());
                self.page_buffer()[offset..(offset+count)].copy_from_slice(&bytes[..count]);
            } else {
                let offset = (page - address) as usize;
                let count = (bytes.len() - offset).min(page_size);
                self.page_buffer()[..count].copy_from_slice(&bytes[offset..(offset + count)]);
            }
        }
        // Flush the last page 
        self.flush_page()?;

        Ok(())
    }

    /// Read bytes from the provided address
    fn read_bytes(&self, address: u32, bytes: &mut [u8]) -> Result<(), BlockDeviceError>;
}