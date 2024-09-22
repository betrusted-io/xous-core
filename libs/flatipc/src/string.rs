#[derive(Clone, Copy)]
pub struct String<const N: usize> {
    length: usize,
    buffer: [u8; N],
}

unsafe impl<const N: usize> crate::IpcSafe for String<N> {}

impl<const N: usize> String<N> {
    pub fn new() -> Self { String { buffer: [0; N], length: 0 } }

    pub fn from_str(s: &str) -> Self {
        let mut buffer = [0; N];
        let length = s.len();
        buffer.copy_from_slice(s.as_bytes());
        String { buffer, length }
    }
}

impl<const N: usize> From<&str> for String<N> {
    fn from(value: &str) -> Self { Self::from_str(value) }
}

impl<const N: usize> core::fmt::Write for String<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        assert!(self.length <= self.buffer.len());
        if s.len() + self.length > N {
            return Err(core::fmt::Error);
        }
        self.buffer[self.length..self.length + s.len()].copy_from_slice(s.as_bytes());
        self.length += s.len();
        Ok(())
    }
}

impl<const N: usize> core::fmt::Debug for String<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Safe because we guarantee the buffer is valid UTF-8
        write!(f, "{:?}", self.as_ref())
    }
}

impl<const N: usize> Default for String<N> {
    fn default() -> Self { String::new() }
}

impl<const N: usize> core::fmt::Display for String<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Safe because we guarantee the buffer is valid UTF-8
        write!(f, "{}", self.as_ref())
    }
}

impl<const N: usize> AsRef<str> for String<N> {
    fn as_ref(&self) -> &str {
        assert!(self.length <= self.buffer.len());
        core::str::from_utf8(&self.buffer[0..self.length]).unwrap()
    }
}
