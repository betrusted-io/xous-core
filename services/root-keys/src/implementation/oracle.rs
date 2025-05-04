use core::convert::TryInto;

use aes::Aes256;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use cipher::generic_array::GenericArray;
use keystore_api::rootkeys_api::*;

use crate::bitflip;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FpgaKeySource {
    Bbram,
    Efuse,
}

pub(crate) const AES_BLOCKSIZE: usize = 16;
// a "slight" lie in that it's not literally the bitstream command, it's the fully encoded word + length of
// args.
pub(crate) const BITSTREAM_CTL0_CMD: u32 = 0x3000_a001;
pub(crate) const BITSTREAM_CTL0_CMD_FLIP: u32 = 0x8005_000c;
pub(crate) const BITSTREAM_MASK_CMD: u32 = 0x3000_c001;
#[allow(dead_code)]
pub(crate) const BITSTREAM_MASK_CMD_FLIP: u32 = 0x8005_000c;
pub(crate) const BITSTREAM_IV_CMD: u32 = 0x3001_6004;
pub(crate) const BITSTREAM_CIPHERTEXT_CMD: u32 = 0x3003_4001;
/// This structure encapsulates the tools necessary to create an Oracle that can go from
/// the encrypted bitstream to plaintext and back again, based on the position in the bitstream.
/// It is a partial re-implementation of the Cbc crate from block-ciphers, and the reason we're
/// not just using the stock Cbc crate is that it doesn't seem to support restarting the Cbc
/// stream from an arbitrary position.
pub(crate) struct BitstreamOracle<'a> {
    bitstream: &'a [u8],
    base: u32,
    dec_cipher: Aes256,
    enc_cipher: Aes256,
    iv: [u8; AES_BLOCKSIZE],
    /// subslice of the bitstream that contains just the encrypted area of the bitstream
    ciphertext: &'a [u8],
    ct_absolute_offset: usize,
    type2_count: u32,
    /// start of type2 data as an absolute offset in the overall bitstream
    type2_absolute_offset: usize,
    /// start of type2 data as an offset relative to the start of ciphertext
    type2_ciphertext_offset: usize,
    /// length of the undifferentiated plaintext header -- this is right up to the IV specifier
    #[allow(dead_code)]
    pt_header_len: usize,
    enc_to_key: FpgaKeySource,
    dec_from_key: FpgaKeySource,
}
impl<'a> BitstreamOracle<'a> {
    /// oracle supports separate encryption and decryption keys, so that it may
    /// be used for re-encryption of bitstreams to a new key. If using it for patching,
    /// set the keys to the same value.
    /// key_target selects what we want the encrypted target to be for boot key source; if None, we retain the
    /// bitstream settings
    pub fn new(
        dec_key: &'a [u8],
        enc_key: &'a [u8],
        bitstream: &'a [u8],
        base: u32,
    ) -> Result<BitstreamOracle<'a>, RootkeyResult> {
        let mut position: usize = 0;

        // Search through the bitstream for the key words that define the IV and ciphertext length.
        // This is done so that if extra headers are added or modified, the code doesn't break (versus coding
        // in static offsets).
        let mut iv_pos = 0;
        let mut cwd = 0;
        while position < bitstream.len() {
            cwd = u32::from_be_bytes(bitstream[position..position + 4].try_into().unwrap());
            if cwd == BITSTREAM_IV_CMD {
                iv_pos = position + 4
            }
            if cwd == BITSTREAM_CIPHERTEXT_CMD {
                break;
            }
            position += 1;
        }

        let position = position + 4;
        let ciphertext_len = 4 * u32::from_be_bytes(bitstream[position..position + 4].try_into().unwrap());
        let ciphertext_start = position + 4;
        if ciphertext_start & (AES_BLOCKSIZE - 1) != 0 {
            log::error!(
                "Padding is incorrect on the bitstream. Check append_csr.py for padding and make sure you are burning gateware with --raw-binary, and not --bitstream as the latter strips padding from the top of the file."
            );
            return Err(RootkeyResult::AlignmentError);
        }
        log::debug!("ciphertext len: {} bytes, start: 0x{:08x}", ciphertext_len, ciphertext_start);
        let ciphertext = &bitstream[ciphertext_start..ciphertext_start + ciphertext_len as usize];

        let mut iv_bytes: [u8; AES_BLOCKSIZE] = [0; AES_BLOCKSIZE];
        bitflip(&bitstream[iv_pos..iv_pos + AES_BLOCKSIZE], &mut iv_bytes);
        log::debug!("recovered iv (pre-flip): {:x?}", &bitstream[iv_pos..iv_pos + AES_BLOCKSIZE]);
        log::debug!("recovered iv           : {:x?}", &iv_bytes);
        #[cfg(feature = "hazardous-debug")]
        log::debug!("dec key for oracle {:x?}", dec_key);
        #[cfg(feature = "hazardous-debug")]
        log::debug!("enc key for oracle {:x?}", enc_key);

        let dec_cipher = Aes256::new(dec_key.try_into().unwrap());
        let enc_cipher = Aes256::new(enc_key.try_into().unwrap());

        let mut oracle = BitstreamOracle {
            bitstream,
            base,
            dec_cipher,
            enc_cipher,
            iv: iv_bytes,
            ciphertext,
            ct_absolute_offset: ciphertext_start,
            type2_count: 0,
            type2_absolute_offset: 0,
            type2_ciphertext_offset: 0,
            pt_header_len: iv_pos, // plaintext header goes all the way up to the IV
            enc_to_key: FpgaKeySource::Efuse, // these get set later
            dec_from_key: FpgaKeySource::Efuse,
        };

        // search forward for the "type 2" region in the first kilobyte of plaintext
        // type2 is where regular dataframes start, and we can compute offsets for patching
        // based on this.
        let mut first_block: [u8; 1024] = [0; 1024];
        oracle.decrypt(0, &mut first_block);

        // check that the plaintext header has a well-known sequence in it -- sanity check the AES key
        let mut pass = true;
        for &b in first_block[32..64].iter() {
            if b != 0x6C {
                pass = false;
            }
        }
        if !pass {
            log::error!("hmac_header did not decrypt correctly: {:x?}", &first_block[..64]);
            return Err(RootkeyResult::KeyError);
        }

        // start searching for the commands *after* the IV and well-known sequence
        let mut pt_pos = 64;
        let mut flipword: [u8; 4] = [0; 4];
        for word in first_block[64..].chunks_exact(4).into_iter() {
            bitflip(&word[0..4], &mut flipword);
            cwd = u32::from_be_bytes(flipword);
            pt_pos += 4;
            if (cwd & 0xE000_0000) == 0x4000_0000 {
                break;
            }
        }
        oracle.type2_count = cwd & 0x3FF_FFFF;
        if pt_pos > 1000 {
            // pt_pos is usually found within the first 200 bytes of a bitstream, should definitely be in the
            // first 1k or else shenanigans
            log::error!("type 2 region not found in the expected region, is the FPGA key correct?");
            return Err(RootkeyResult::KeyError);
        }
        oracle.type2_absolute_offset = pt_pos + ciphertext_start;
        oracle.type2_ciphertext_offset = pt_pos;
        log::debug!(
            "type2 absolute: {}, relative to ct start: {}",
            oracle.type2_absolute_offset,
            oracle.type2_ciphertext_offset
        );

        // read the boot source out of the ciphertext
        let mut bytes = first_block.chunks(4).into_iter();
        let mut ctl0_enc: Option<u32> = None;
        loop {
            if let Some(b) = bytes.next() {
                let word = u32::from_be_bytes(b.try_into().unwrap());
                if word == BITSTREAM_CTL0_CMD_FLIP {
                    if let Some(val) = bytes.next() {
                        let mut flip: [u8; 4] = [0; 4];
                        bitflip(val, &mut flip);
                        let w = u32::from_be_bytes(flip.try_into().unwrap());
                        ctl0_enc = Some(w);
                    } else {
                        log::error!("didn't decrypt enough memory to find the ctl0 encrypted settings");
                        return Err(RootkeyResult::IntegrityError);
                    }
                    break;
                }
            } else {
                break;
            }
        }
        if (ctl0_enc.unwrap() & 0x8000_0000) == 0x8000_0000 {
            oracle.dec_from_key = FpgaKeySource::Efuse;
        } else {
            oracle.dec_from_key = FpgaKeySource::Bbram;
        }
        // by default, we always re-encrypt to the same key type
        oracle.enc_to_key = oracle.dec_from_key;

        Ok(oracle)
    }

    #[allow(dead_code)]
    pub fn pt_header_len(&self) -> usize { self.pt_header_len as usize }

    pub fn base(&self) -> u32 { self.base }

    pub fn bitstream(&self) -> &[u8] { self.bitstream }

    pub fn ciphertext_offset(&self) -> usize { self.ct_absolute_offset as usize }

    pub fn ciphertext(&self) -> &[u8] { self.ciphertext }

    pub fn get_original_key_type(&self) -> FpgaKeySource { self.dec_from_key }

    pub fn get_target_key_type(&self) -> FpgaKeySource { self.enc_to_key }

    pub fn set_target_key_type(&mut self, keytype: FpgaKeySource) {
        log::debug!("Oracle target key type set to {:?}", keytype);
        self.enc_to_key = keytype;
    }

    pub fn ciphertext_offset_to_frame(&self, offset: usize) -> (usize, usize) {
        let type2_offset = offset - self.type2_ciphertext_offset;

        let frame = type2_offset / (101 * 4);
        let frame_offset = type2_offset - (frame * 101 * 4);
        (frame, frame_offset / 4)
    }

    pub fn clear(&mut self) {
        self.enc_cipher.clear();
        self.dec_cipher.clear();

        for b in self.iv.iter_mut() {
            *b = 0;
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Decrypts a portion of the bitstream starting at "from", of length output.
    /// Returns the actual number of bytes processed.
    pub fn decrypt(&self, from: usize, output: &mut [u8]) -> usize {
        assert!(from & (AES_BLOCKSIZE - 1) == 0); // all requests must be an even multiple of an AES block size

        let mut index = from;
        let mut temp_block = [0; AES_BLOCKSIZE];
        let mut chain: [u8; AES_BLOCKSIZE] = [0; AES_BLOCKSIZE];
        let mut bytes_processed = 0;
        for block in output.chunks_mut(AES_BLOCKSIZE).into_iter() {
            if index > self.ciphertext_len() - AES_BLOCKSIZE {
                return bytes_processed;
            }
            if index == 0 {
                chain = self.iv;
            } else {
                bitflip(&self.ciphertext[index - AES_BLOCKSIZE..index], &mut chain);
            };
            // copy the ciphertext into the temp_block, with a bitflip
            bitflip(&self.ciphertext[index..index + AES_BLOCKSIZE], &mut temp_block);

            // replaces the ciphertext with "plaintext"
            let mut d = GenericArray::clone_from_slice(&mut temp_block);
            self.dec_cipher.decrypt_block(&mut d);
            for (&src, dst) in d.iter().zip(temp_block.iter_mut()) {
                *dst = src;
            }

            // now XOR against the IV into the final output block. We use a "temp" block so we
            // guarantee an even block size for AES, but in fact, the output block does not
            // have to be an even multiple of our block size!
            for (dst, (&src, &iv)) in block.iter_mut().zip(temp_block.iter().zip(chain.iter())) {
                *dst = src ^ iv;
            }
            index += AES_BLOCKSIZE;
            bytes_processed += AES_BLOCKSIZE;
        }
        bytes_processed
    }

    /// Encrypts a portion of a bitstream starting at "from"; manages the IV lookup for the encryption process
    ///
    /// "from" is relative to ciphertext start, and chosen to match the "from" of a decrypt operation.
    /// When using this function to encrypt the very first block, the "from" offset should be negative.
    ///
    /// ASSUME: the chain is unbroken, e.g. this is being called successively on correctly encrypted, chained
    /// blocks, that have been written to FLASH such that we can refer to the older blocks for the current
    /// chaining value. It is up to the caller to manage the linear order of the calls.
    /// ASSUME: `from` + `self.ct_absolute_offset` is a multiple of an erase block
    /// returns the actual number of bytes processed
    /// NOTE: input plaintext can be changed by this function -- the first block is modified based on the
    /// requested encryption type
    pub fn encrypt_sector(&self, from: i32, input_plaintext: &mut [u8], output_sector: &mut [u8]) -> usize {
        assert!(
            output_sector.len() & (AES_BLOCKSIZE - 1) == 0,
            "output length must be a multiple of AES block size"
        );
        assert!(
            input_plaintext.len() & (AES_BLOCKSIZE - 1) == 0,
            "input length must be a multiple of AES block size"
        );
        assert!(
            (from + self.ct_absolute_offset as i32) & (spinor::SPINOR_ERASE_SIZE as i32 - 1) == 0,
            "request address must line up with an erase block boundary"
        );

        if from > 0 {
            assert!(input_plaintext.len() == output_sector.len(), "input and output length must match");
        } else {
            assert!(
                input_plaintext.len() == output_sector.len() - self.ct_absolute_offset,
                "output length must have space for the plaintext header"
            );
        }

        let mut out_start_offset = 0;
        if from < 0 {
            // A bit of a nightmare, but we have to patch over the efuse/bbram selection settings
            // on the fly, because a user might be using bbram, but updates are by default set to use efuse.
            //
            // the plaintext header already has:
            //   - one copy of the fuse and config settings
            //   - padding
            //   - IV for cipher
            //   - length of ciphertext region
            // slightly into the ciphertext region is another set of bits that re-confirm the key settings.
            // we need to flip the bits that control bbram vs aes key boot as part of the re-encryption
            // process.
            //
            // So: 1. check to see what type of boot we are doing
            //     2. update the header bits to match it.
            //     3. update the ciphertext (in two places) to match.
            //
            // The instruction that selects eFuse/BBRAM is 'Control Register 0' (00101)
            // It has the in-bitstream hex code of 0x3000a001 and is set to 0x80000040 for efuse boot and
            // 0x00000040 for bbram boot. The byte to patch is at offset 88 decimal in the bitstream headers
            // generated by our tools, but it would be more reliable to search for the instruction and patch
            // the byte after it. The `mask` register (00110) is also set, hex code 0x3000c001,
            // set to 0x80000040 to allow for the control register write to "take"

            // copy the plaintext header over to the destination, as-is
            for (&src, dst) in self.bitstream[..self.ct_absolute_offset]
                .iter()
                .zip(output_sector[..self.ct_absolute_offset].iter_mut())
            {
                *dst = src;
            }
            out_start_offset = self.ct_absolute_offset;

            // now, make sure that the new settings match the settings indicated by the oracle.
            // 1. patch the plaintext header
            let mut pos = 0;
            let mut patchcount = 0;
            while pos < self.ct_absolute_offset {
                let cwd = u32::from_be_bytes(output_sector[pos..pos + 4].try_into().unwrap());
                if (cwd == BITSTREAM_CTL0_CMD) || (cwd == BITSTREAM_MASK_CMD) {
                    pos += 4;
                    // the bit we want to patch is in MSB of the value, just patch it directly since
                    // the pos pointer is at this byte.
                    log::debug!("output_sector: {:x?}", &output_sector[pos..pos + 4]);
                    match self.enc_to_key {
                        FpgaKeySource::Bbram => output_sector[pos] &= 0x7f,
                        FpgaKeySource::Efuse => output_sector[pos] |= 0x80,
                    }
                    log::debug!("patched {:x?}", &output_sector[pos..pos + 4]);
                    patchcount += 1;
                    if patchcount >= 2 {
                        break; // short circuit the checking when we're done
                    }
                }
                pos += 4;
            }

            // 2. set the commands that will eventually be encrypted, by patching the input plaintext
            pos = 64; // start from after the hmac header
            while pos < input_plaintext.len() {
                let cwd = u32::from_be_bytes(input_plaintext[pos..pos + 4].try_into().unwrap());
                // the mask is always 0xffff_ffff for the bitstreams as generated by vivado...so need to
                // modify that
                if cwd == BITSTREAM_CTL0_CMD_FLIP {
                    pos += 4;
                    // the bit we want to patch is in MSB of the value, just patch it directly since
                    // the pos pointer is at this byte.
                    log::debug!("patching ct header orig:    {:x}", input_plaintext[pos + 3]);
                    log::debug!("{:x?}", &input_plaintext[pos..pos + 4]);
                    if cwd == BITSTREAM_CTL0_CMD_FLIP {
                        match self.enc_to_key {
                            FpgaKeySource::Bbram => input_plaintext[pos + 3] &= 0xfe,
                            FpgaKeySource::Efuse => input_plaintext[pos + 3] |= 0x01, /* bit 31 is at byte
                                                                                       * 3 "lsb" in the
                                                                                       * flipped version */
                        }
                    }
                    log::debug!("{:x?}", &input_plaintext[pos..pos + 4]);
                    break;
                }
                pos += 4;
            }
        }
        let mut chain: [u8; AES_BLOCKSIZE] = self.iv;
        if from > 0 {
            bitflip(&self.ciphertext[from as usize - AES_BLOCKSIZE..from as usize], &mut chain);
        }

        // 3. search for the encryption key source setting only in the last blocks
        if from > ((self.type2_count as i32 * 4 + self.type2_ciphertext_offset as i32) & 0x7FFF_F000) - 0x1000
        {
            // maybe that complex math above would be better replaced with the the constant 0x21_5000. it
            // never changes...? I guess it might be helpful if someone ported this code to a new
            // FPGA type.
            log::debug!("type2 count: {} offset: {}", self.type2_count, self.type2_ciphertext_offset);
            log::debug!("searching for second key setting starting from 0x{:x}", from);
            let mut bytes = input_plaintext.chunks_mut(4).into_iter();
            loop {
                if let Some(b) = bytes.next() {
                    let word = u32::from_be_bytes(b[..4].try_into().unwrap());
                    if word == BITSTREAM_CTL0_CMD_FLIP {
                        log::debug!("patching the second key setting inside block starting at 0x{:x}", from);
                        if let Some(val) = bytes.next() {
                            log::debug!("{:x?}", val);
                            match self.enc_to_key {
                                FpgaKeySource::Bbram => val[3] &= 0xfe,
                                FpgaKeySource::Efuse => val[3] |= 0x01, /* bit 31 is at byte 3 "lsb" in the
                                                                         * flipped version */
                            }
                            log::debug!("{:x?}", val);
                        } else {
                            log::error!("didn't decrypt enough memory to find the ctl0 encrypted settings");
                        }
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        let mut bytes_processed = 0;
        let mut temp_block = [0; AES_BLOCKSIZE];
        for (iblock, oblock) in input_plaintext
            .chunks(AES_BLOCKSIZE)
            .into_iter()
            .zip(output_sector[out_start_offset..].chunks_mut(AES_BLOCKSIZE).into_iter())
        {
            // XOR against IV/chain value
            for (dst, (&src, &iv)) in temp_block.iter_mut().zip(iblock.iter().zip(chain.iter())) {
                *dst = src ^ iv;
            }
            let mut e = GenericArray::clone_from_slice(&mut temp_block);
            self.enc_cipher.encrypt_block(&mut e);
            for (&src, dst) in e.iter().zip(temp_block.iter_mut()) {
                *dst = src;
            }
            // redo the IV based on the previous output. no bitflipping of the IV.
            for (&src, dst) in temp_block.iter().zip(chain.iter_mut()) {
                *dst = src;
            }

            // copy the ciphertext into the output block, with a bitflip
            bitflip(&temp_block, oblock);
            bytes_processed += AES_BLOCKSIZE;
        }
        bytes_processed
    }

    pub fn ciphertext_len(&self) -> usize { self.ciphertext.len() }
}
