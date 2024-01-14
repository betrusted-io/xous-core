/// no_std replacement for Cursor.

pub struct BufferWrapper<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

impl<'a> BufferWrapper<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self { BufferWrapper { buf, offset: 0 } }

    pub fn len(&self) -> usize { self.offset }
}

impl<'a> core::fmt::Write for BufferWrapper<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();

        // Skip over already-copied data
        let remainder = &mut self.buf[self.offset..];

        // Check if there is space remaining (return error instead of panicking)
        if remainder.len() < bytes.len() {
            return Err(core::fmt::Error);
        }

        // Make the two slices the same length
        let remainder = &mut remainder[..bytes.len()];

        // Copy
        remainder.copy_from_slice(bytes);

        // Update offset to avoid overwriting
        self.offset += bytes.len();

        Ok(())
    }
}
