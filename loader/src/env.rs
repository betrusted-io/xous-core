extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

#[repr(C, packed)]
pub struct EnvHeader {
    magic_app: [u8; 4],
    size: u32,
    /// The size of the entire application slice, in bytes, including all headers
    length_total: u32,
    /// Number of application parameters present. Must be at least 1 (this block)
    entries: u32,

    magic_envb: [u8; 4],
    /// Total number of bytes, excluding this header
    size_env: usize,
    /// The number of environment variables
    count: u16,
}

#[derive(Debug, Clone)]
pub struct EnvVariables {
    count: usize,
    flattened_vars: Vec<u8>,
}

impl EnvVariables {
    pub fn new() -> Self { Self { count: 0, flattened_vars: Vec::new() } }

    pub fn add_var(&mut self, key: &str, value: &str) {
        self.flattened_vars.extend_from_slice(&(key.len() as u16).to_le_bytes());
        self.flattened_vars.extend_from_slice(&key.as_bytes());
        self.flattened_vars.extend_from_slice(&(value.len() as u16).to_le_bytes());
        self.flattened_vars.extend_from_slice(&value.as_bytes());
        self.count += 1;
    }
}

impl EnvHeader {
    pub fn default() -> Self {
        EnvHeader {
            magic_app: *b"AppP",
            size: 8,
            length_total: 0,
            entries: 2,
            magic_envb: *b"EnvB",
            size_env: 0,
            count: 0,
        }
    }

    pub fn to_bytes<'a>(&'a mut self, vars: &'a EnvVariables) -> Vec<u8> {
        let header_size = size_of::<Self>();
        let total_len = header_size + vars.flattened_vars.len();
        // finalize the length
        self.length_total = total_len as u32;
        self.size_env = total_len - 24; // size of all the elements up to and including `size_env`
        // finalize env variables
        assert!(vars.count < u16::MAX as usize, "too many environment variables");
        self.count = vars.count as u16;

        // Allocate contiguous memory on the stack or heap
        let ptr = (self as *const Self) as *const u8;
        let header_bytes = unsafe { core::slice::from_raw_parts(ptr, header_size) };

        // Combine header + trailing data into one contiguous slice
        let mut buf = Vec::with_capacity(total_len);
        buf.extend_from_slice(header_bytes);
        buf.extend_from_slice(&vars.flattened_vars);
        buf.shrink_to_fit();
        buf
    }
}

pub fn to_hex_ascii(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(LUT[(b >> 4) as usize]);
        out.push(LUT[(b & 0xF) as usize]);
    }
    // Safe because LUT only contains valid ASCII bytes
    unsafe { String::from_utf8_unchecked(out) }
}
