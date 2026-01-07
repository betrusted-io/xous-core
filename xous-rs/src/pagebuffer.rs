use core::fmt;

/// A variant of StringBuffer that is stack-allocated. Used primarily for passing debug information
/// to and from the kernel.
#[repr(align(4096))]
pub struct PageBuf {
    len: usize,
    data: [u8; 4092],
}

impl PageBuf {
    pub fn new() -> Self { Self { len: 0, data: [0u8; 4092] } }

    /// Returns the written data as a byte slice
    pub fn as_bytes(&self) -> &[u8] { &self.data[..self.len] }

    /// Returns the written data as a string slice
    /// Safe because we maintain UTF-8 validity in write_str
    pub fn as_str(&self) -> &str { unsafe { core::str::from_utf8_unchecked(&self.data[..self.len]) } }

    /// Clears the buffer
    pub fn clear(&mut self) { self.len = 0; }

    /// Returns the number of bytes written
    pub fn len(&self) -> usize { self.len }

    /// Returns remaining capacity
    pub fn remaining(&self) -> usize { self.data.len() - self.len }

    /// Reconstructs a PageBuf reference from a raw pointer address.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - The pointer points to a valid, initialized PageBuf
    /// - The memory is properly aligned (4096 bytes)
    /// - The memory will remain valid for the lifetime 'a
    /// - No other mutable references to this memory exist
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The pointer is null
    /// - The pointer is not 4096-byte aligned
    /// - The len field exceeds 4092
    /// - The data[..len] contains invalid UTF-8
    pub unsafe fn from_raw_ptr_mut<'a>(ptr: usize) -> &'a mut Self {
        // Check non-null
        assert_ne!(ptr, 0, "PageBuf pointer must not be null");

        // Check alignment
        assert_eq!(
            ptr % 4096,
            0,
            "PageBuf pointer must be 4096-byte aligned, got alignment of {}",
            ptr % 4096
        );

        // Cast to reference
        let page_buf = &mut *(ptr as *mut PageBuf);

        // Validate len field
        assert!(page_buf.len <= 4092, "PageBuf len ({}) exceeds maximum (4092)", page_buf.len);

        // Validate UTF-8 invariant
        core::str::from_utf8(&page_buf.data[..page_buf.len]).expect("PageBuf data must contain valid UTF-8");

        page_buf
    }

    /// Returns the address of this PageBuf as a usize for passing across boundaries
    pub fn as_ptr(&self) -> usize { self as *const Self as usize }
}

impl fmt::Write for PageBuf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let available = self.data.len() - self.len;

        if available == 0 {
            // Buffer is full, silently discard
            return Ok(());
        }

        let to_write = if bytes.len() <= available {
            bytes.len()
        } else {
            // Find the last valid UTF-8 boundary within available space
            // This prevents cutting in the middle of a multi-byte character
            let mut boundary = available;
            while boundary > 0 && !s.is_char_boundary(boundary) {
                boundary -= 1;
            }
            boundary
        };

        self.data[self.len..self.len + to_write].copy_from_slice(&bytes[..to_write]);
        self.len += to_write;

        Ok(())
    }
}

impl Default for PageBuf {
    fn default() -> Self { Self::new() }
}
