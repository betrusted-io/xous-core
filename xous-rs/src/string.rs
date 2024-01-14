use crate::Error;

#[derive(Copy, Clone)]
pub struct String<const N: usize> {
    bytes: [u8; N],
    len: u32, // length in bytes, not characters
}

impl<const N: usize> String<N> {
    pub fn new() -> String<N> { String { bytes: [0; N], len: 0 } }

    pub fn as_bytes(&self) -> [u8; N] { self.bytes }

    pub fn as_str(&self) -> core::result::Result<&str, core::str::Utf8Error> {
        core::str::from_utf8(&self.bytes[0..self.len as usize])
    }

    pub fn len(&self) -> usize { self.len as usize }

    pub fn is_empty(&self) -> bool { self.len == 0 }

    /// Clear the contents of this String and set the length to 0
    pub fn clear(&mut self) {
        self.len = 0;
        self.bytes = [0; N];
    }

    pub fn to_str(&self) -> &str { unsafe { core::str::from_utf8_unchecked(&self.bytes[0..self.len()]) } }

    pub fn push(&mut self, ch: char) -> core::result::Result<usize, Error> {
        match ch.len_utf8() {
            1 => {
                if self.len() < self.bytes.len() {
                    self.bytes[self.len()] = ch as u8;
                    self.len += 1;
                    Ok(1)
                } else {
                    Err(Error::OutOfMemory)
                }
            }
            _ => {
                let mut bytes: usize = 0;
                let mut data: [u8; 4] = [0; 4];
                let subslice = ch.encode_utf8(&mut data);
                if self.len() + subslice.len() < self.bytes.len() {
                    for c in subslice.bytes() {
                        self.bytes[self.len()] = c;
                        self.len += 1;
                        bytes += 1;
                    }
                    Ok(bytes)
                } else {
                    Err(Error::OutOfMemory)
                }
            }
        }
    }

    pub fn append(&mut self, s: &str) -> core::result::Result<usize, Error> {
        let mut bytes_added = 0;
        for ch in s.chars() {
            if let Ok(bytes) = self.push(ch) {
                bytes_added += bytes;
            } else {
                return Err(Error::OutOfMemory);
            }
        }
        Ok(bytes_added)
    }

    pub fn push_byte(&mut self, b: u8) -> core::result::Result<(), Error> {
        if self.len() < self.bytes.len() {
            self.bytes[self.len()] = b;
            self.len += 1;
            Ok(())
        } else {
            Err(Error::OutOfMemory)
        }
    }
}

impl<const N: usize> Default for String<N> {
    fn default() -> Self { Self::new() }
}

impl<const N: usize> core::str::FromStr for String<N> {
    type Err = &'static str;

    fn from_str(src: &str) -> core::result::Result<String<N>, &'static str> {
        let mut s = Self::new();
        // Copy the string into our backing store.
        for (&src_byte, dest_byte) in src.as_bytes().iter().zip(&mut s.bytes) {
            *dest_byte = src_byte;
        }
        // Set the string length to the length of the passed-in String,
        // or the maximum possible length. Which ever is smaller.
        s.len = s.bytes.len().min(src.as_bytes().len()) as u32;

        // If the string is not valid, set its length to 0.
        if s.as_str().is_err() {
            s.len = 0;
        }
        Ok(s)
    }
}

impl<const N: usize> core::fmt::Display for String<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{}", self.to_str()) }
}

impl<const N: usize> core::fmt::Write for String<N> {
    fn write_str(&mut self, s: &str) -> core::result::Result<(), core::fmt::Error> {
        for c in s.bytes() {
            if self.len() < self.bytes.len() {
                self.bytes[self.len()] = c;
                self.len += 1;
            }
        }
        Ok(())
    }
}

impl<const N: usize> core::fmt::Debug for String<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{}", self.to_str()) }
}

impl<const N: usize> core::convert::AsRef<str> for String<N> {
    fn as_ref(&self) -> &str { self.to_str() }
}

impl<const N: usize> PartialEq for String<N> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes[..self.len as usize] == other.bytes[..other.len as usize] && self.len == other.len
    }
}

impl<const N: usize> Eq for String<N> {}
