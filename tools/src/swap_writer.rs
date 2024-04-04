use std::convert::TryInto;
use std::io::{Cursor, Result, Seek, SeekFrom, Write};

use aes_gcm_siv::{
    aead::{Aead, KeyInit, Payload},
    Aes256GcmSiv, Nonce,
};
use rand::Rng;

const SWAP_VERSION: u32 = 0x01_01_0000;

pub struct SwapWriter {
    pub buffer: Cursor<Vec<u8>>,
}

struct SwapHeader {
    aad: Vec<u8>,
    partial_nonce: u64,
    /// offset, starting from the top of payload data (so, +0x1000 to get absolute offset after the header)
    mac_offset: usize,
}
impl SwapHeader {
    pub fn new(swap_len: usize) -> Self {
        let mut rng = rand::thread_rng();
        SwapHeader {
            aad: "swap".as_bytes().to_vec(),
            partial_nonce: rng.gen(),
            mac_offset: ((swap_len + 0xFFF) / 0x1000) * 0x1000,
        }
    }

    /// Returns exactly a page of data with the header format serialized
    pub fn serialize(&self) -> Result<[u8; 4096]> {
        let mut data = Cursor::new(Vec::<u8>::new());
        let mut output = [0u8; 4096];

        data.write(&SWAP_VERSION.to_le_bytes())?;
        // note that the nonce is in big-endian format, as is expected for cryptographic matter
        data.write(&self.partial_nonce.to_be_bytes())?;

        // serialize the AAD
        data.write(&(self.aad.len() as u32).to_le_bytes())?; // LE because this is a size field
        data.write(&self.aad)?; // BE because this is cryptographic matter

        // serialize the MAC data offset
        data.write(&(self.mac_offset as u32).to_le_bytes())?; // LE because this is a size field

        output[..data.position() as usize].copy_from_slice(&data.into_inner());

        Ok(output)
    }
}

impl SwapWriter {
    pub fn new() -> Self { SwapWriter { buffer: Cursor::new(Vec::new()) } }

    /// Take the swap file and wrap it data structures that facilitate per-device encryption
    /// after deployment to a user device.
    pub fn encrypt_to<T>(&mut self, mut f: T) -> Result<usize>
    where
        T: Write + Seek,
    {
        let header = SwapHeader::new(self.buffer.get_ref().len());
        let mut macs = Vec::<u8>::new();

        f.write(&header.serialize()?)?;

        // encrypt using the "zero key" for the default distribution. The intention is not to
        // provide security, but to lay out the data structure so that a future re-encryption to a
        // secret key generated in the device can provide security.
        let zero_key = [0u8; 32];
        let cipher = Aes256GcmSiv::new(&zero_key.try_into().unwrap());
        let buf = self.buffer.get_ref();
        for (offset, block) in buf.chunks(0x1000).enumerate() {
            let padded_block = if block.len() != 0x1000 {
                [block, &vec![0u8; 0x1000 - block.len()]].concat()
            } else {
                block.to_owned()
            };
            let mut nonce_vec = Vec::new();
            // use BE encoding because nonce is cryptographic matter
            nonce_vec.extend_from_slice(&(offset as u32).to_be_bytes());
            nonce_vec.extend_from_slice(&header.partial_nonce.to_be_bytes());
            let nonce = Nonce::from_slice(&nonce_vec);
            let enc = cipher
                .encrypt(nonce, Payload { aad: &header.aad, msg: &padded_block })
                .expect("couldn't encrypt block");
            assert!(enc.len() == 0x1010);
            f.write(&enc[..0x1000])?;
            macs.extend_from_slice(&enc[0x1000..]);
        }

        f.write(&macs)
    }
}

impl Write for SwapWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> { self.buffer.write(buf) }

    fn flush(&mut self) -> std::io::Result<()> { self.buffer.flush() }
}

impl Seek for SwapWriter {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> { self.buffer.seek(pos) }
}
