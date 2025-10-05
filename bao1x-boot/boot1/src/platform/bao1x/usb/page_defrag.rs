use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use core::mem;

const PAGE_SIZE: usize = bao1x_api::baosec::SPINOR_PAGE_LEN as usize;
const SECTOR_SIZE: usize = bao1x_api::baosec::SPINOR_ERASE_SIZE as usize;
const PAGES_PER_SECTOR: usize = PAGE_SIZE / SECTOR_SIZE;

/// Manages assembly of pages from pages
pub struct PageAssembler<F> {
    /// Active pages being assembled (key is page address)
    pages: BTreeMap<usize, SectorState>,
    /// Callback for completed pages
    on_complete: F,
}

struct SectorState {
    /// The page data buffer
    data: Box<[u8; SECTOR_SIZE]>,
    /// Bitmap tracking which pages have been received (bit i = page i)
    received_mask: u16,
}

impl SectorState {
    fn new() -> Self { Self { data: Box::new([0u8; SECTOR_SIZE]), received_mask: 0 } }

    /// Add a page at the given offset (0-15)
    fn add_page(&mut self, page_offset: usize, data: &[u8; PAGE_SIZE]) {
        debug_assert!(page_offset < PAGES_PER_SECTOR);

        // Copy page data to the appropriate position
        let byte_offset = page_offset * PAGE_SIZE;
        self.data[byte_offset..byte_offset + PAGE_SIZE].copy_from_slice(data);

        // Mark page as received
        self.received_mask |= 1 << page_offset;
    }

    /// Check if all pages have been received
    fn is_complete(&self) -> bool {
        // All 16 bits should be set
        self.received_mask == 0xFFFF
    }
}

impl<F> PageAssembler<F>
where
    F: Fn(usize, Box<[u8; SECTOR_SIZE]>),
{
    pub fn new(on_complete: F) -> Self { Self { pages: BTreeMap::new(), on_complete } }

    /// Process an incoming page
    ///
    /// # Arguments
    /// * `address` - The byte address where this page belongs in memory
    /// * `data` - The page data (must be PAGE_SIZE bytes)
    ///
    /// # Returns
    /// * `Ok(true)` if this page completed a page
    /// * `Ok(false)` if the page is still incomplete
    /// * `Err(&str)` if the page is invalid
    pub fn add_page(&mut self, address: usize, data: &[u8; PAGE_SIZE]) -> Result<bool, &'static str> {
        // Calculate page and page offset
        let page_addr = address & !(SECTOR_SIZE - 1); // Align to sector boundary
        let byte_offset = address & (SECTOR_SIZE - 1); // Offset within page

        // Verify page alignment
        if byte_offset % PAGE_SIZE != 0 {
            return Err("page not aligned to 256-byte boundary");
        }

        let page_offset = byte_offset / PAGE_SIZE;

        // Get or create page state
        let page = self.pages.entry(page_addr).or_insert_with(SectorState::new);

        // Warn if page double-receive
        if page.received_mask & (1 << page_offset) != 0 {
            crate::println_d!("WARN: double-receive of page at {:x}", page_addr);
        }

        // Add the page
        // crate::println_d!("Adding to page offset {:x} on page at {:x}", page_offset, page_addr);
        page.add_page(page_offset, data);

        // Check if page is complete
        if page.is_complete() {
            // Remove completed page and trigger callback
            if let Some(mut completed) = self.pages.remove(&page_addr) {
                // Extract the data to pass to callback
                let data = mem::replace(&mut completed.data, Box::new([0u8; SECTOR_SIZE]));
                // crate::println!("calling on_complete with {:x}, {:x?}", page_addr, &data[..8]);
                (self.on_complete)(page_addr, data);
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get the number of active (incomplete) pages
    pub fn active_pages(&self) -> usize { self.pages.len() }

    /// Check progress of a specific page
    #[allow(dead_code)]
    pub fn page_progress(&self, page_addr: usize) -> Option<(usize, usize)> {
        self.pages.get(&page_addr).map(|page| {
            let received = page.received_mask.count_ones() as usize;
            (received, PAGES_PER_SECTOR)
        })
    }

    /// Clear all incomplete pages (useful for error recovery)
    #[allow(dead_code)]
    pub fn clear(&mut self) { self.pages.clear(); }

    /// Remove and return the next incomplete sector from the assembler
    ///
    /// This method removes sectors in address order (lowest address first)
    /// and returns the sector address and its current data (which may be partially filled).
    ///
    /// # Returns
    /// * `Some((address, data))` - The address and data of an incomplete sector
    /// * `None` - If there are no more incomplete sectors
    pub fn take_next_incomplete(&mut self) -> Option<(usize, Box<[u8; SECTOR_SIZE]>)> {
        // Use pop_first() to remove the lowest address sector
        // This gives predictable ordering
        self.pages.pop_first().map(|(addr, mut state)| {
            // Replace the data with a zero buffer to avoid double-move
            let data = mem::replace(&mut state.data, Box::new([0u8; SECTOR_SIZE]));
            (addr, data)
        })
    }
}

// Example usage
#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn test_in_order_assembly() {
        let mut completed_pages = Vec::new();
        let mut assembler = PageAssembler::new(|addr, data| {
            completed_pages.push((addr, data));
        });

        // Send pages in order for page at address 0x1000
        for i in 0..16 {
            let page = [i as u8; PAGE_SIZE];
            let addr = 0x1000 + (i * PAGE_SIZE);
            let completed = assembler.add_page(addr, &page).unwrap();

            if i < 15 {
                assert!(!completed);
            } else {
                assert!(completed);
            }
        }

        assert_eq!(completed_pages.len(), 1);
        assert_eq!(completed_pages[0].0, 0x1000);
    }

    #[test]
    fn test_out_of_order_assembly() {
        let mut completed_pages = Vec::new();
        let mut assembler = PageAssembler::new(|addr, _data| {
            completed_pages.push(addr);
        });

        // Send pages out of order
        let indices = [15, 0, 7, 3, 11, 1, 9, 2, 14, 4, 8, 5, 13, 6, 10, 12];
        for (step, &i) in indices.iter().enumerate() {
            let page = [i as u8; PAGE_SIZE];
            let addr = 0x2000 + (i * PAGE_SIZE);
            let completed = assembler.add_page(addr, &page).unwrap();

            if step < 15 {
                assert!(!completed);
            } else {
                assert!(completed);
            }
        }

        assert_eq!(completed_pages.len(), 1);
        assert_eq!(completed_pages[0], 0x2000);
    }
}
